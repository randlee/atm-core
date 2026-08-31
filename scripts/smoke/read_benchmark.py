#!/usr/bin/env python3
"""Run the additive AV.4 read/query benchmark families.

The send-message benchmark remains the owner of the existing v4 transport
schema.  This module registers the read families beside it and uses the same
public account, report-index envelope, and sc-compose report template.  It
never opens SQLite directly: all workload traffic goes through the ``atm``
CLI, which keeps the benchmark on the product read path.
"""
from __future__ import annotations

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from datetime import datetime, timezone
import html
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time
from typing import Any, Mapping, Sequence

from scripts.smoke.benchmark_baselines import BenchmarkBaselineError, load_baselines
from scripts.smoke.benchmark_report import BenchmarkReportError, compose, regenerate_index, render_envelope
from scripts.smoke.benchmark_policy import classify_status
from scripts.smoke.benchmark_schema import (
    BaselineEntry,
    BaselineSet,
    distribution,
    require_non_decreasing_baselines,
)


ROOT = Path(__file__).resolve().parents[2]
REPORTS_ROOT = ROOT / "site" / "reports"
REPORT_DIR = REPORTS_ROOT / "read-query-benchmark"
BASELINES_PATH = REPORT_DIR / "baselines.json"
REPORT_ENVELOPE = REPORTS_ROOT / "read-query-benchmark.json"

# D7 workload contract.  These values are intentionally constants: a caller
# cannot increase the pools to make a floor easier to satisfy.
FANOUT = 32
MAILBOX_POOL_SIZE = 4
MAILBOX_QUEUE_DEPTH = 16
SEARCH_POOL_SIZE = 2
SEARCH_QUEUE_DEPTH = 8
WARMUP_SECONDS = 2.0
MEASUREMENT_SECONDS = 5.0
SUCCESS_RATE_FLOOR = 0.999
# D7 mixed-mode budget: the AV.1a contract requires a bounded p95 while the
# runtime metric is not yet exported. Keep the source next to the value so a
# future AV.1b change can replace both in one reviewed edit.
MIXED_READ_P95_BUDGET_MS = 1000.0
MIXED_READ_P95_BUDGET_SOURCE = "AV.1a D5 interim p95 ceiling; replace when reader metrics are exported"
WRITER_RATE_PER_SECOND = 8.0
WRITER_PAYLOAD_BYTES = 256
CORPUS_SEED = "av4-corpus-v1"
CORPUS_GENERATOR_VERSION = "av4-corpus-generator-1"
CORPUS_MESSAGES_PER_MEMBER = 1
CORPUS_PAYLOAD_BYTES = 256
CORPUS_SKEW_PROFILE = "uniform:one-message-per-member"
HARNESS_VERSION = "av4-read-benchmark-1"
RATCHET_TOLERANCE_PCT = 5
# Flip this checked-in marker to True only in the reviewed merge-forward that
# contains AV.1b's reader-handler cutover and exported D5 metrics.  Diagnostic
# runs remain available while it is False; official/seeding runs fail closed.
AV1B_CUTOVER_LANDED = False


class ReadBenchmarkError(RuntimeError):
    """A read benchmark cannot produce valid evidence."""


@dataclass(frozen=True)
class ReadFamily:
    """One additive family registration and its fixed lane contract."""

    family_id: str
    description: str
    lane: str
    pool_size: int
    queue_depth: int
    query: bool = False
    mixed_writes: bool = False


FAMILIES: tuple[ReadFamily, ...] = (
    ReadFamily(
        "read-fanout",
        "Concurrent read, peek, and list against the seeded corpus",
        "mailbox",
        MAILBOX_POOL_SIZE,
        MAILBOX_QUEUE_DEPTH,
    ),
    ReadFamily(
        "query-fts",
        "Concurrent FTS search and filtered-list queries",
        "search",
        SEARCH_POOL_SIZE,
        SEARCH_QUEUE_DEPTH,
        query=True,
    ),
    ReadFamily(
        "read-under-write-load",
        "Read fan-out while fixed-rate writers sustain load",
        "mailbox",
        MAILBOX_POOL_SIZE,
        MAILBOX_QUEUE_DEPTH,
        mixed_writes=True,
    ),
)
FAMILY_BY_ID = {family.family_id: family for family in FAMILIES}


@dataclass(frozen=True)
class CorpusMember:
    team: str
    agent: str


@dataclass(frozen=True)
class Corpus:
    seed: str
    generator_version: str
    members: tuple[CorpusMember, ...]


@dataclass(frozen=True)
class RequestObservation:
    elapsed_ms: float
    success: bool
    timed_out: bool = False
    error: str | None = None


def deterministic_corpus(seed: str = CORPUS_SEED) -> Corpus:
    """Build the fixed >=8 team x >=4 agent corpus without randomness."""
    if not seed or not isinstance(seed, str):
        raise ReadBenchmarkError("corpus seed must be a non-empty string")
    members = tuple(
        CorpusMember(f"av4-team-{team:02d}", f"av4-agent-{team:02d}-{agent:02d}")
        for team in range(1, 9)
        for agent in range(1, 5)
    )
    return Corpus(seed, CORPUS_GENERATOR_VERSION, members)


def _fixed_body(prefix: str, size: int) -> str:
    """Pad deterministic payloads to the recorded byte size."""
    encoded_prefix = prefix.encode("utf-8")
    if len(encoded_prefix) > size:
        raise ReadBenchmarkError(f"payload prefix exceeds configured size {size}")
    return prefix + "x" * (size - len(encoded_prefix))


def validate_workload_contract(
    *,
    family: ReadFamily,
    corpus: Corpus,
    fanout: int,
    warmup_seconds: float,
    measurement_seconds: float,
    success_rate: float | None = None,
    partial_artifact: bool = False,
) -> None:
    """Fail closed on every D7 contract omission before floor publication."""
    if family.family_id not in FAMILY_BY_ID:
        raise ReadBenchmarkError(f"unknown read benchmark family {family.family_id!r}")
    team_counts = Counter(member.team for member in corpus.members)
    if len(team_counts) < 8 or len(corpus.members) < 32 or min(team_counts.values(), default=0) < 4:
        raise ReadBenchmarkError("corpus must contain at least 8 teams and 4 agents per team")
    if not corpus.seed or not corpus.generator_version:
        raise ReadBenchmarkError("corpus seed and generator version are required")
    if fanout < FANOUT:
        raise ReadBenchmarkError(f"fanout must be at least {FANOUT}")
    if warmup_seconds <= 0 or measurement_seconds <= 0:
        raise ReadBenchmarkError("warm-up and measurement windows must both be positive")
    if partial_artifact:
        raise ReadBenchmarkError("partial benchmark artifacts are not publishable")
    if success_rate is not None and success_rate < SUCCESS_RATE_FLOOR:
        raise ReadBenchmarkError(
            f"success rate {success_rate:.6f} is below {SUCCESS_RATE_FLOOR:.4f}"
        )


def _run(command: Sequence[str], env: Mapping[str, str], timeout: float = 30.0) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            list(command), cwd=ROOT, env=dict(env), capture_output=True, text=True,
            encoding="utf-8", errors="replace", timeout=timeout, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ReadBenchmarkError(f"benchmark command failed to start: {' '.join(command)}: {error}") from error


def _atm_binary() -> str:
    configured = os.environ.get("ATM_READ_BENCHMARK_ATM", "").strip()
    if configured:
        return configured
    candidate = ROOT / "target" / "release" / ("atm.exe" if os.name == "nt" else "atm")
    if candidate.is_file():
        return str(candidate)
    found = shutil.which("atm")
    if found:
        return found
    raise ReadBenchmarkError("release-built atm is required; run cargo build --release first")


def prepare_corpus(atm: str, env: Mapping[str, str], corpus: Corpus) -> None:
    """Create and seed the corpus through public CLI commands only."""
    driver_team = "av4-driver"
    driver_agent = "av4-driver"
    add = [atm, "teams", "add-member", driver_team, driver_agent, "--json"]
    result = _run(add, env)
    if result.returncode:
        raise ReadBenchmarkError(result.stderr.strip() or "could not create AV.4 driver")
    for member in corpus.members:
        add_member = [atm, "teams", "add-member", member.team, member.agent, "--json"]
        result = _run(add_member, env)
        if result.returncode:
            raise ReadBenchmarkError(result.stderr.strip() or f"could not create {member}")
        body = _fixed_body(
            f"{CORPUS_SEED} team={member.team} agent={member.agent} ordinal=0",
            CORPUS_PAYLOAD_BYTES,
        )
        send = [atm, "send", f"{member.agent}@{member.team}", body, "--json"]
        result = _run(send, env)
        if result.returncode:
            raise ReadBenchmarkError(result.stderr.strip() or f"could not seed {member}")


def _operation_command(atm: str, family: ReadFamily, member: CorpusMember) -> list[str]:
    if family.query:
        return [
            atm, "search", CORPUS_SEED, "--team", member.team,
            "--agent", member.agent, "--limit", "100", "--json",
        ]
    # Derive the operation from the deterministic member suffix rather than
    # Python's process-randomized ``hash()``.
    operation_index = int(member.agent[-2:]) % 3
    operation = ("peek", "list", "read")[operation_index]
    command = [atm, operation, "--all", "--team", member.team, "--json"]
    # ``--no-since-last-seen`` is a read/peek selector; ``atm list`` has no
    # equivalent flag and rejects unknown arguments. Keep the family matrix
    # command-valid while preserving the no-mutation read contract where the
    # option exists.
    if operation != "list":
        command.append("--no-since-last-seen")
    return command


def _observe(
    atm: str,
    family: ReadFamily,
    member: CorpusMember,
    base_env: Mapping[str, str],
) -> RequestObservation:
    env = dict(base_env)
    env.update({"ATM_IDENTITY": member.agent, "ATM_TEAM": member.team})
    started = time.perf_counter()
    try:
        result = _run(_operation_command(atm, family, member), env, timeout=15.0)
    except ReadBenchmarkError as error:
        detail = str(error)
        return RequestObservation(
            (time.perf_counter() - started) * 1000,
            False,
            timed_out="timed out" in detail.lower(),
            error=detail,
        )
    elapsed = (time.perf_counter() - started) * 1000
    if result.returncode == 0:
        return RequestObservation(elapsed, True)
    detail = result.stderr.strip() or result.stdout.strip() or f"exit {result.returncode}"
    return RequestObservation(elapsed, False, error=detail)


def _run_window(
    atm: str,
    family: ReadFamily,
    corpus: Corpus,
    env: Mapping[str, str],
    duration: float,
    *,
    writer_stop: threading.Event | None = None,
) -> tuple[list[RequestObservation], float]:
    """Run a fixed window with the normative fan-out and no retries."""
    observations: list[RequestObservation] = []
    started = time.perf_counter()
    deadline = started + duration
    with ThreadPoolExecutor(max_workers=FANOUT, thread_name_prefix="atm-read") as executor:
        while time.perf_counter() < deadline:
            futures = [
                executor.submit(_observe, atm, family, corpus.members[index % len(corpus.members)], env)
                for index in range(FANOUT)
            ]
            for future in as_completed(futures):
                observations.append(future.result())
    if writer_stop is not None:
        writer_stop.set()
    return observations, time.perf_counter() - started


def _writer_loop(
    atm: str, corpus: Corpus, env: Mapping[str, str], stop: threading.Event,
    counter: list[int], errors: list[str],
) -> None:
    interval = 1.0 / WRITER_RATE_PER_SECOND
    sent = 0
    while not stop.is_set():
        member = corpus.members[sent % len(corpus.members)]
        body = _fixed_body(f"av4-writer {sent:08d} ", WRITER_PAYLOAD_BYTES)
        writer_env = dict(env)
        writer_env.update({"ATM_IDENTITY": "av4-driver", "ATM_TEAM": "av4-driver"})
        try:
            result = _run(
                [atm, "send", f"{member.agent}@{member.team}", body, "--json"],
                writer_env,
                timeout=15.0,
            )
            if result.returncode:
                errors.append(result.stderr.strip() or result.stdout.strip() or f"exit {result.returncode}")
        except ReadBenchmarkError as error:
            errors.append(str(error))
        sent += 1
        counter[0] = sent
        stop.wait(interval)


def _diagnostics(family: ReadFamily, observations: Sequence[RequestObservation]) -> dict[str, Any]:
    latencies = [item.elapsed_ms for item in observations]
    timeout_count = sum(item.timed_out for item in observations)
    error_count = sum(not item.success for item in observations)
    def unavailable() -> dict[str, Any]:
        return {
            "value": None,
            "status": "unimplemented",
            "source": "cli-not-exposed",
            "reason": "the CLI does not expose this AV.1a D5 runtime gauge yet",
        }
    return {
        "lane": family.lane,
        "pool_size": family.pool_size,
        "queue_depth": family.queue_depth,
        "in_flight": unavailable(),
        "wait_ms": unavailable(),
        "execution_ms": distribution([float(value) for value in latencies]),
        "deadline_expiries": {
            "observed_cli_timeouts": timeout_count,
            "expired_in_queue": unavailable(),
            "interrupted_while_active": unavailable(),
            "quarantined": unavailable(),
            "other_cli_errors": error_count - timeout_count,
        },
        "saturation_events": unavailable(),
        "quarantined_worker_gauge": unavailable(),
        "retired_replaced_workers": unavailable(),
        "quarantine_exhausted_rejections": unavailable(),
        "wal_health": {
            "last_checkpoint_outcome": unavailable(),
            "current_wal_frame_count": unavailable(),
        },
    }


def run_family(atm: str, family: ReadFamily, corpus: Corpus, env: Mapping[str, str]) -> dict[str, Any]:
    validate_workload_contract(
        family=family, corpus=corpus, fanout=FANOUT,
        warmup_seconds=WARMUP_SECONDS, measurement_seconds=MEASUREMENT_SECONDS,
    )
    _run_window(atm, family, corpus, env, WARMUP_SECONDS)
    stop = threading.Event()
    writer_thread: threading.Thread | None = None
    writer_counter = [0]
    writer_errors: list[str] = []
    if family.mixed_writes:
        writer_thread = threading.Thread(
            target=_writer_loop, args=(atm, corpus, env, stop, writer_counter, writer_errors),
            name="atm-read-writer", daemon=True,
        )
        writer_thread.start()
    observations, elapsed = _run_window(
        atm, family, corpus, env, MEASUREMENT_SECONDS, writer_stop=stop if family.mixed_writes else None,
    )
    if writer_thread is not None:
        writer_thread.join(timeout=15.0)
    successes = sum(item.success for item in observations)
    success_rate = successes / len(observations) if observations else 0.0
    validate_workload_contract(
        family=family, corpus=corpus, fanout=FANOUT,
        warmup_seconds=WARMUP_SECONDS, measurement_seconds=MEASUREMENT_SECONDS,
        success_rate=success_rate,
    )
    latencies = [item.elapsed_ms for item in observations]
    throughput = successes / elapsed if elapsed else 0.0
    result: dict[str, Any] = {
        "family": family.family_id,
        "description": family.description,
        "status": classify_status(
            lifecycle_complete=bool(observations),
            messages_requested=len(observations),
            messages_admitted=successes,
            messages_durable=successes,
            p50_admissions_per_second=throughput,
            baseline_p50_floor=0.0,
        ),
        "fanout": FANOUT,
        "pool_size": family.pool_size,
        "queue_depth": family.queue_depth,
        "windows": {"warmup_seconds": WARMUP_SECONDS, "measurement_seconds": MEASUREMENT_SECONDS},
        "requests": {"total": len(observations), "successful": successes, "success_rate": success_rate},
        "throughput_requests_per_second": throughput,
        # The fixed-window runner has one aggregate sample today.  Keep the
        # distribution-shaped field so future interval samples extend the
        # existing report contract without a schema change.
        "throughput_per_second": distribution([throughput]),
        "latency_ms": distribution([float(value) for value in latencies]),
        "diagnostics": _diagnostics(family, observations),
        "mixed_read_p95_budget_ms": MIXED_READ_P95_BUDGET_MS if family.mixed_writes else None,
        "mixed_read_p95_budget_source": MIXED_READ_P95_BUDGET_SOURCE if family.mixed_writes else None,
        "writer": {
            "rate_per_second": WRITER_RATE_PER_SECOND,
            "payload_bytes": WRITER_PAYLOAD_BYTES,
            "throughput_per_second": writer_counter[0] / elapsed if elapsed else 0.0,
        } if family.mixed_writes else None,
    }
    if family.mixed_writes and result["latency_ms"]["p95"] > MIXED_READ_P95_BUDGET_MS:
        result["status"] = "FAIL"
        result["failure"] = "read p95 exceeded the 1000ms mixed-mode budget"
    if family.mixed_writes and writer_errors:
        result["status"] = "FAIL"
        result["failure"] = f"writer errors observed: {writer_errors[0]}"
        result["writer"]["errors"] = writer_errors
    return result


def apply_floor(result: dict[str, Any], baseline: BaselineEntry) -> None:
    """Apply the shared reviewed baseline floor without rounding the comparison."""
    if baseline.target not in FAMILY_BY_ID:
        raise ReadBenchmarkError(f"baseline target is not a read family: {baseline.target!r}")
    if baseline.seeded_runs != 3 or baseline.source_campaigns is None:
        raise ReadBenchmarkError("read baseline must cite exactly three clean source campaigns")
    if len(baseline.source_campaigns) != 3 or any(not campaign.strip() for campaign in baseline.source_campaigns):
        raise ReadBenchmarkError("read baseline must cite exactly three non-empty source campaigns")
    if baseline.corpus_seed != CORPUS_SEED or baseline.corpus_generator_version != CORPUS_GENERATOR_VERSION:
        raise ReadBenchmarkError("read baseline corpus provenance does not match this harness")
    if baseline.harness_version != HARNESS_VERSION:
        raise ReadBenchmarkError("read baseline harness provenance does not match this harness")
    if (
        baseline.fanout is None
        or baseline.fanout < FANOUT
        or (baseline.mailbox_pool_size, baseline.mailbox_queue_depth)
        != (MAILBOX_POOL_SIZE, MAILBOX_QUEUE_DEPTH)
        or (baseline.search_pool_size, baseline.search_queue_depth)
        != (SEARCH_POOL_SIZE, SEARCH_QUEUE_DEPTH)
    ):
        raise ReadBenchmarkError("read baseline lane settings do not match this harness")
    result["baseline_p50_floor"] = baseline.p50_floor
    if result["throughput_per_second"]["p50"] < baseline.p50_floor:
        result["status"] = "FAIL"
        result["failure"] = (
            f"p50 throughput {result['throughput_per_second']['p50']} "
            f"is below baseline floor {baseline.p50_floor}"
        )


def _check_baseline_revision(previous: BaselineSet, current: BaselineSet) -> None:
    """Delegate ratchet validation to the repository-wide schema contract."""
    try:
        require_non_decreasing_baselines(previous, current)
    except ValueError as error:
        raise ReadBenchmarkError(str(error)) from error


def _report_variables(
    payload: Mapping[str, Any], html_path: Path, json_path: Path,
) -> dict[str, Any]:
    """Build safe HTML fragments consumed by the existing sc-compose template."""
    esc = lambda value: html.escape(str(value), quote=True)
    summary = (
        f"<p>Read/query campaign <code>{esc(payload['campaign_id'])}</code> on "
        f"<code>{esc(payload['host_label'])}</code>.</p>"
        f"<p>Corpus seed <code>{esc(payload['corpus']['seed'])}</code>; generator "
        f"<code>{esc(payload['corpus']['generator_version'])}</code>; fan-out {payload['fanout']}.</p>"
    )
    rows: list[str] = []
    for result in payload["families"]:
        latency = result.get("latency_ms", {})
        throughput = result.get("throughput_per_second", {})
        rows.append(
            "<tr>"
            f"<td>{esc(result['family'])}</td><td>{esc(result['status'])}</td>"
            f"<td>{throughput.get('p50', 'n/a')}</td>"
            f"<td>{latency.get('p50', 'n/a')}</td><td>{latency.get('p95', 'n/a')}</td>"
            f"<td>{latency.get('p99', 'n/a')}</td><td>{result['requests']['success_rate']:.6f}</td>"
            f"<td>{result['throughput_requests_per_second']:.3f}</td>"
            "</tr>"
        )
    sections = (
        "<section><h2>Family metrics</h2><table><thead><tr>"
        "<th>Family</th><th>Status</th><th>p50 requests/s</th><th>p50 latency ms</th><th>p95</th><th>p99</th>"
        "<th>Success rate</th><th>Requests/s</th></tr></thead><tbody>"
        + "".join(rows)
        + "</tbody></table></section>"
        "<section><h2>Reader-lane diagnostics</h2><pre>"
        + esc(json.dumps(
            {item["family"]: item["diagnostics"] for item in payload["families"]},
            indent=2, sort_keys=True,
        ))
        + "</pre></section>"
        "<section><h2>D5 implementation status</h2>"
        "<p>Reader-lane runtime gauges are explicitly marked unimplemented "
        "until the AV.1b reader cutover publishes them. Null values are not "
        "zero measurements.</p></section>"
    )
    return {
        "output_path": str(html_path),
        "json_output_path": str(json_path),
        "title": "ATM read/query benchmark",
        "subtitle": "AV.4 additive read benchmark families",
        "generated_at": payload["generated_at"],
        "status": payload["status"],
        "status_class": "status-pass" if payload["status"] == "PASS" else "status-error",
        "summary_html": summary,
        "sections_html": sections,
        "footer_html": "Generated from the existing sc-compose view-report template.",
    }


def build_payload(results: Sequence[dict[str, Any]], corpus: Corpus, host_label: str) -> dict[str, Any]:
    started = datetime.now(timezone.utc)
    campaign_id = f"{started.strftime('%Y%m%dT%H%M%SZ')}-{host_label}-read"
    status = "PASS" if all(item["status"] == "PASS" for item in results) else "FAIL"
    return {
        "schema_version": 1,
        "report_type": "read-query-benchmark",
        "campaign_id": campaign_id,
        # Each family has a stable derived campaign identity.  Just's three
        # subordinate targets compose into this envelope, so provenance keeps
        # all three IDs instead of collapsing them into one opaque run.
        "campaign_ids": [f"{campaign_id}-{item['family']}" for item in results],
        "generated_at": started.isoformat().replace("+00:00", "Z"),
        "host_label": host_label,
        "status": status,
        "harness_version": HARNESS_VERSION,
        "source_revision": _source_revision(),
        "fanout": FANOUT,
        "mixed_mode_budget": {
            "p95_latency_ms": MIXED_READ_P95_BUDGET_MS,
            "source": MIXED_READ_P95_BUDGET_SOURCE,
        },
        "corpus": {
            "seed": corpus.seed,
            "generator_version": corpus.generator_version,
            "message_count": len(corpus.members) * CORPUS_MESSAGES_PER_MEMBER,
            "messages_per_member": CORPUS_MESSAGES_PER_MEMBER,
            "message_size_distribution": {
                "kind": "fixed",
                "payload_bytes": CORPUS_PAYLOAD_BYTES,
            },
            "team_count": len({member.team for member in corpus.members}),
            "agents_per_team": 4,
            "skew_profile": CORPUS_SKEW_PROFILE,
        },
        "families": list(results),
    }


def _source_revision() -> str:
    result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=False)
    return result.stdout.strip() if result.returncode == 0 else "unknown"


def require_av1b_cutover() -> None:
    """Keep official floors off the serialized path until AV.1b is live."""
    if not AV1B_CUTOVER_LANDED:
        raise ReadBenchmarkError(
            "official read campaign is gated until AV.1b reader cutover lands; "
            "flip AV1B_CUTOVER_LANDED in the reviewed merge-forward"
        )


def execute(family_ids: Sequence[str], *, diagnostic_only: bool = False) -> int:
    if not family_ids:
        raise ReadBenchmarkError("at least one family is required")
    families = tuple(FAMILY_BY_ID.get(value) for value in family_ids)
    if any(family is None for family in families):
        raise ReadBenchmarkError(f"unknown family in {family_ids!r}")
    if not diagnostic_only and len(families) != len(FAMILIES):
        raise ReadBenchmarkError("only just benchmark-read may publish official evidence")
    if not diagnostic_only:
        require_av1b_cutover()
    host_label = os.environ.get("ATM_CAPACITY_HOST_LABEL", "local")
    try:
        baselines = load_baselines(BASELINES_PATH)
    except BenchmarkBaselineError as error:
        raise ReadBenchmarkError(str(error)) from error
    previous_path = BASELINES_PATH.with_name("baselines.previous.json")
    if previous_path.exists():
        try:
            _check_baseline_revision(load_baselines(previous_path), baselines)
        except BenchmarkBaselineError as error:
            raise ReadBenchmarkError(str(error)) from error
    if not diagnostic_only:
        missing = [
            family.family_id
            for family in families
            if not any(
                entry.host_label == host_label and entry.target == family.family_id
                for entry in baselines.entries
            )
        ]
        if missing:
            raise ReadBenchmarkError(
                "official read campaign has no reviewed three-run baseline for "
                f"host_label={host_label!r}: {', '.join(missing)}"
            )
    # The manifest check is deliberately before resolving or invoking any CLI.
    from scripts.smoke.benchmark_account import require_benchmark_account

    account = require_benchmark_account()
    atm = _atm_binary()
    env = dict(os.environ)
    env.update({"ATM_IDENTITY": "av4-driver", "ATM_TEAM": "av4-driver"})
    corpus = deterministic_corpus()
    prepare_corpus(atm, env, corpus)
    results = [run_family(atm, family, corpus, env) for family in families]
    if not diagnostic_only:
        for result in results:
            apply_floor(result, baselines.entry_for(host_label, result["family"]))
    payload = build_payload(results, corpus, host_label)
    if not diagnostic_only:
        payload["baseline_revision"] = baselines.revision
    if diagnostic_only:
        print(json.dumps(payload, indent=2, sort_keys=True))
        return 0 if payload["status"] == "PASS" else 1
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    json_path = REPORT_DIR / f"{payload['campaign_id']}.json"
    html_path = REPORT_DIR / "index.html"
    family_artifacts: list[Path] = []
    artifact_paths = [json_path, html_path]
    try:
        payload["partial_artifact"] = False
        json_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        for result in results:
            family_id = result["family"]
            family_json = REPORT_DIR / f"{payload['campaign_id']}-{family_id}.json"
            family_html = REPORT_DIR / f"{payload['campaign_id']}-{family_id}.xhtml"
            family_payload = {**payload, "campaign_id": f"{payload['campaign_id']}-{family_id}", "families": [result]}
            family_json.write_text(
                json.dumps(family_payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
            )
            compose(
                ROOT / ".just/templates/view-report.html.j2",
                _report_variables(family_payload, family_html, family_json),
                family_html,
            )
            family_artifacts.extend((family_json, family_html))
        compose(
            ROOT / ".just/templates/view-report.html.j2",
            _report_variables(payload, html_path, json_path),
            html_path,
        )
        artifact_paths.extend(family_artifacts)
        payload["partial_artifact"] = any(not path.is_file() for path in artifact_paths)
        if payload["partial_artifact"]:
            json_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            validate_workload_contract(
                family=FAMILIES[0], corpus=corpus, fanout=FANOUT,
                warmup_seconds=WARMUP_SECONDS, measurement_seconds=MEASUREMENT_SECONDS,
                partial_artifact=True,
            )
        render_envelope(
            (),
            report_root=REPORTS_ROOT,
            report_name="read-query-benchmark",
            generated_at=payload["generated_at"],
            host_label=payload["host_label"],
            report_html="read-query-benchmark/index.html",
        )
        regenerate_index()
        check = subprocess.run(
            ["just", "reports-index", "--check"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        if check.returncode:
            raise ReadBenchmarkError(
                check.stderr.strip() or check.stdout.strip() or "reports-index check failed"
            )
    except Exception:
        for path in artifact_paths + family_artifacts:
            path.unlink(missing_ok=True)
        REPORT_ENVELOPE.unlink(missing_ok=True)
        raise
    print(f"read benchmark report: {json_path}")
    return 0 if payload["status"] == "PASS" else 1


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--family", choices=[*FAMILY_BY_ID])
    parser.add_argument("--families", nargs="+", choices=[*FAMILY_BY_ID])
    parser.add_argument("--diagnostic-only", action="store_true")
    args = parser.parse_args(argv)
    selected = args.families or ([args.family] if args.family else list(FAMILY_BY_ID))
    try:
        return execute(selected, diagnostic_only=args.diagnostic_only)
    except (ReadBenchmarkError, BenchmarkReportError) as error:
        print(f"read benchmark: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
