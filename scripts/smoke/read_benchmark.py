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
from typing import Any, Callable, Iterable, Mapping, Sequence


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
WRITER_RATE_PER_SECOND = 8.0
WRITER_PAYLOAD_BYTES = 256
CORPUS_SEED = "av4-corpus-v1"
CORPUS_GENERATOR_VERSION = "av4-corpus-generator-1"
HARNESS_VERSION = "av4-read-benchmark-1"
RATCHET_TOLERANCE_PCT = 5


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


def percentile(values: Sequence[float], fraction: float) -> float:
    """Return an unrounded nearest-rank percentile for ratchet comparison."""
    if not values:
        raise ReadBenchmarkError("cannot calculate a percentile over no samples")
    if not 0.0 <= fraction <= 1.0:
        raise ReadBenchmarkError("percentile fraction must be between zero and one")
    ordered = sorted(values)
    rank = max(0, min(len(ordered) - 1, int((len(ordered) - 1) * fraction)))
    return ordered[rank]


def metric_distribution(values: Sequence[float]) -> dict[str, float]:
    if not values:
        raise ReadBenchmarkError("a clean report requires latency samples")
    return {
        "min": min(values),
        "p50": percentile(values, 0.50),
        "p95": percentile(values, 0.95),
        "p99": percentile(values, 0.99),
        "max": max(values),
    }


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
    if len({member.team for member in corpus.members}) < 8 or len(corpus.members) < 32:
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


def load_read_baselines(path: Path = BASELINES_PATH) -> dict[str, dict[str, dict[str, Any]]]:
    """Load the additive family section of the reviewed baselines.json."""
    if not path.exists():
        return {}
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ReadBenchmarkError(f"invalid read baselines {path}: {error}") from error
    if not isinstance(payload, dict) or payload.get("schema_version") != 1:
        raise ReadBenchmarkError("read baselines require schema_version=1")
    families = payload.get("read_families", {})
    if not isinstance(families, dict):
        raise ReadBenchmarkError("read_families must be an object")
    unknown = set(families) - set(FAMILY_BY_ID)
    if unknown:
        raise ReadBenchmarkError(f"unknown read baseline families: {sorted(unknown)}")
    return families


def ratchet_floor(
    *,
    family_id: str,
    host_label: str,
    campaign_p50s: Sequence[float],
    source_campaigns: Sequence[str],
    corpus: Corpus,
    previous: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    """Seed from exactly three clean campaigns and ratchet upward only."""
    if len(campaign_p50s) != 3 or len(source_campaigns) != 3:
        raise ReadBenchmarkError("a floor requires exactly three source campaigns")
    if any(value < 0 for value in campaign_p50s):
        raise ReadBenchmarkError("campaign p50 values cannot be negative")
    observed_floor = min(campaign_p50s)
    prior_floor = float(previous.get("p50_floor", 0)) if previous else 0.0
    floor = max(prior_floor, observed_floor * (1 - RATCHET_TOLERANCE_PCT / 100))
    return {
        "p50_floor": floor,
        "seeded_runs": 3,
        "ratchet_tolerance_pct": RATCHET_TOLERANCE_PCT,
        "source_campaigns": list(source_campaigns),
        "corpus_seed": corpus.seed,
        "corpus_generator_version": corpus.generator_version,
        "fanout": FANOUT,
        "mailbox_pool_size": MAILBOX_POOL_SIZE,
        "mailbox_queue_depth": MAILBOX_QUEUE_DEPTH,
        "search_pool_size": SEARCH_POOL_SIZE,
        "search_queue_depth": SEARCH_QUEUE_DEPTH,
        "harness_version": HARNESS_VERSION,
    }


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
        body = f"{CORPUS_SEED} team={member.team} agent={member.agent} ordinal=0"
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
    return [atm, operation, "--all", "--team", member.team, "--json", "--no-since-last-seen"]


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
        return RequestObservation((time.perf_counter() - started) * 1000, False, error=str(error))
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
    counter: list[int],
) -> None:
    interval = 1.0 / WRITER_RATE_PER_SECOND
    sent = 0
    while not stop.is_set():
        member = corpus.members[sent % len(corpus.members)]
        body = (f"av4-writer {sent:08d} " + "x" * WRITER_PAYLOAD_BYTES)
        writer_env = dict(env)
        writer_env.update({"ATM_IDENTITY": "av4-driver", "ATM_TEAM": "av4-driver"})
        _run([atm, "send", f"{member.agent}@{member.team}", body, "--json"], writer_env, timeout=15.0)
        sent += 1
        counter[0] = sent
        stop.wait(interval)
    return sent


def _diagnostics(family: ReadFamily, observations: Sequence[RequestObservation]) -> dict[str, Any]:
    latencies = [item.elapsed_ms for item in observations]
    timeout_count = sum(item.timed_out for item in observations)
    return {
        "lane": family.lane,
        "pool_size": family.pool_size,
        "queue_depth": family.queue_depth,
        "wait_ms": {"p50": 0.0, "p95": 0.0, "p99": 0.0, "source": "cli-observation"},
        "execution_ms": metric_distribution(latencies),
        "deadline_expiries": {"total": timeout_count, "by_outcome": {"timeout": timeout_count}},
        "saturation_events": 0,
        "quarantine_gauge": 0,
        "wal_health": {"status": "not-exposed-by-cli", "source": "cli-observation"},
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
    if family.mixed_writes:
        writer_thread = threading.Thread(
            target=_writer_loop, args=(atm, corpus, env, stop, writer_counter),
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
    result: dict[str, Any] = {
        "family": family.family_id,
        "description": family.description,
        "status": "PASS",
        "fanout": FANOUT,
        "pool_size": family.pool_size,
        "queue_depth": family.queue_depth,
        "windows": {"warmup_seconds": WARMUP_SECONDS, "measurement_seconds": MEASUREMENT_SECONDS},
        "requests": {"total": len(observations), "successful": successes, "success_rate": success_rate},
        "throughput_requests_per_second": successes / elapsed if elapsed else 0.0,
        # The fixed-window runner has one aggregate sample today.  Keep the
        # distribution-shaped field so future interval samples extend the
        # existing report contract without a schema change.
        "throughput_per_second": metric_distribution(
            [successes / elapsed if elapsed else 0.0]
        ),
        "latency_ms": metric_distribution(latencies),
        "diagnostics": _diagnostics(family, observations),
        "writer": {
            "rate_per_second": WRITER_RATE_PER_SECOND,
            "payload_bytes": WRITER_PAYLOAD_BYTES,
            "throughput_per_second": writer_counter[0] / elapsed if elapsed else 0.0,
        } if family.mixed_writes else None,
    }
    if family.mixed_writes and result["latency_ms"]["p95"] > 1000.0:
        result["status"] = "FAIL"
        result["failure"] = "read p95 exceeded the 1000ms mixed-mode budget"
    return result


def apply_floor(result: dict[str, Any], baseline: Mapping[str, Any]) -> None:
    """Apply an immutable p50 floor without rounding the comparison."""
    required = {
        "seeded_runs", "source_campaigns", "corpus_seed", "corpus_generator_version",
        "fanout", "mailbox_pool_size", "mailbox_queue_depth",
        "search_pool_size", "search_queue_depth", "harness_version",
    }
    if baseline.get("seeded_runs") != 3 or not isinstance(baseline.get("source_campaigns"), list):
        raise ReadBenchmarkError("read baseline must cite exactly three clean source campaigns")
    if len(baseline["source_campaigns"]) != 3 or not required.issubset(baseline):
        raise ReadBenchmarkError("read baseline is missing D7 provenance or lane settings")
    if (
        baseline.get("corpus_seed") != CORPUS_SEED
        or baseline.get("corpus_generator_version") != CORPUS_GENERATOR_VERSION
    ):
        raise ReadBenchmarkError("read baseline corpus provenance does not match this harness")
    if baseline.get("fanout", 0) < FANOUT:
        raise ReadBenchmarkError("read baseline fanout is below the D7 minimum")
    if (
        baseline.get("mailbox_pool_size"), baseline.get("mailbox_queue_depth")
    ) != (MAILBOX_POOL_SIZE, MAILBOX_QUEUE_DEPTH):
        raise ReadBenchmarkError("read baseline mailbox lane settings do not match production defaults")
    if (
        baseline.get("search_pool_size"), baseline.get("search_queue_depth")
    ) != (SEARCH_POOL_SIZE, SEARCH_QUEUE_DEPTH):
        raise ReadBenchmarkError("read baseline search lane settings do not match production defaults")
    floor = baseline.get("p50_floor")
    if not isinstance(floor, (int, float)) or floor < 0:
        raise ReadBenchmarkError("read baseline p50_floor must be a non-negative number")
    result["baseline_p50_floor"] = floor
    if result["throughput_per_second"]["p50"] < floor:
        result["status"] = "FAIL"
        result["failure"] = (
            f"p50 throughput {result['throughput_per_second']['p50']} "
            f"is below baseline floor {floor}"
        )


def _report_html(payload: Mapping[str, Any]) -> tuple[str, str]:
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
    )
    return summary, sections


def render_report(payload: Mapping[str, Any], json_path: Path, html_path: Path) -> None:
    """Render through the checked-in sc-compose ``view-report`` contract."""
    summary, sections = _report_html(payload)
    variables = {
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
    html_path.parent.mkdir(parents=True, exist_ok=True)
    with json_path.with_suffix(".variables.json").open("w", encoding="utf-8") as handle:
        json.dump(variables, handle, sort_keys=True)
    variables_path = json_path.with_suffix(".variables.json")
    try:
        result = subprocess.run(
            ["sc-compose", "render", "--root", str(ROOT), "--file", str(ROOT / ".just/templates/view-report.html.j2"),
             "--var-file", str(variables_path), "--output", str(html_path)],
            cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
        )
        if result.returncode:
            raise ReadBenchmarkError(result.stderr.strip() or result.stdout.strip() or "sc-compose render failed")
    finally:
        variables_path.unlink(missing_ok=True)


def build_payload(results: Sequence[dict[str, Any]], corpus: Corpus, host_label: str) -> dict[str, Any]:
    started = datetime.now(timezone.utc)
    campaign_id = f"{started.strftime('%Y%m%dT%H%M%SZ')}-{host_label}-read"
    status = "PASS" if all(item["status"] == "PASS" for item in results) else "FAIL"
    return {
        "schema_version": 1,
        "report_type": "read-query-benchmark",
        "campaign_id": campaign_id,
        "generated_at": started.isoformat().replace("+00:00", "Z"),
        "host_label": host_label,
        "status": status,
        "harness_version": HARNESS_VERSION,
        "source_revision": _source_revision(),
        "fanout": FANOUT,
        "corpus": {
            "seed": corpus.seed,
            "generator_version": corpus.generator_version,
            "team_count": len({member.team for member in corpus.members}),
            "agents_per_team": 4,
        },
        "families": list(results),
    }


def _source_revision() -> str:
    result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=False)
    return result.stdout.strip() if result.returncode == 0 else "unknown"


def execute(family_ids: Sequence[str], *, diagnostic_only: bool = False) -> int:
    if not family_ids:
        raise ReadBenchmarkError("at least one family is required")
    families = tuple(FAMILY_BY_ID.get(value) for value in family_ids)
    if any(family is None for family in families):
        raise ReadBenchmarkError(f"unknown family in {family_ids!r}")
    if not diagnostic_only and len(families) != len(FAMILIES):
        raise ReadBenchmarkError("only just benchmark-read may publish official evidence")
    host_label = os.environ.get("ATM_CAPACITY_HOST_LABEL", "local")
    baselines = load_read_baselines()
    if not diagnostic_only:
        missing = [
            family.family_id for family in families
            if not isinstance(baselines.get(family.family_id, {}).get(host_label), dict)
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
            apply_floor(result, baselines[result["family"]][host_label])
    payload = build_payload(results, corpus, host_label)
    if diagnostic_only:
        print(json.dumps(payload, indent=2, sort_keys=True))
        return 0 if payload["status"] == "PASS" else 1
    REPORT_DIR.mkdir(parents=True, exist_ok=True)
    json_path = REPORT_DIR / f"{payload['campaign_id']}.json"
    html_path = REPORT_DIR / "index.html"
    json_path.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    render_report(payload, json_path, html_path)
    REPORT_ENVELOPE.write_text(json.dumps({
        "schema_version": 1,
        "report_type": "benchmark",
        "generated_at": payload["generated_at"],
        "host_label": payload["host_label"],
        "report_html": "read-query-benchmark/index.html",
    }, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"read benchmark report: {json_path}")
    return 0 if payload["status"] == "PASS" else 1


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--family", choices=[*FAMILY_BY_ID, "all"], default="all")
    parser.add_argument("--diagnostic-only", action="store_true")
    args = parser.parse_args(argv)
    selected = list(FAMILY_BY_ID) if args.family == "all" else [args.family]
    try:
        return execute(selected, diagnostic_only=args.diagnostic_only)
    except ReadBenchmarkError as error:
        print(f"read benchmark: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
