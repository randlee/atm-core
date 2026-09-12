#!/usr/bin/env python3
"""Build the closed procedure inventory from runners and committed evidence."""

from __future__ import annotations

import argparse
from collections import defaultdict
import json
from pathlib import Path
import re
import subprocess
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
INVENTORY = ROOT / "docs" / "procedures" / "inventory.json"
FEATURES = (
    "fast", "normal", "thorough", "localhost", "local-ip", "peer-preflight",
    "crosshost-send", "crosshost-ack", "crosshost-curl-plain", "crosshost-curl-tls",
    "admission-capacity",
)
RUNNERS = {
    "smoke": "scripts/smoke/run_feature_smoke.py",
    "graft": "scripts/phase-ai/run_hermes_graft_live.py",
    "colima": "scripts/smoke/colima_skill_report.py",
    "benchmark": "scripts/smoke/benchmark_report.py",
    "fuzz": ".just/run_fuzz.py",
}


def _json(path: Path) -> dict[str, Any] | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError):
        return None
    return value if isinstance(value, dict) else None


def build_inventory(root: Path = ROOT) -> dict[str, Any]:
    reports = root / "site" / "reports"
    sources: dict[str, set[str]] = defaultdict(set)

    for feature in FEATURES:
        sources[f"smoke-{feature}"].add("runner:feature:" + feature)
    for path in sorted(reports.rglob("*.json")):
        data = _json(path)
        if not data:
            continue
        report_type = data.get("report_type")
        if report_type == "smoke":
            feature = None
            html_path = str(data.get("report_html", ""))
            for candidate in sorted(reports.rglob("*.json")):
                if candidate.parent == path.parent and candidate.name not in {path.name}:
                    payload = _json(candidate)
                    if payload and isinstance(payload.get("feature"), str):
                        feature = payload["feature"]
                        break
            if feature:
                sources["graft-hermes" if feature == "graft-hermes" else
                        "colima-hermes-skills" if feature == "colima-hermes-skills" else
                        "smoke-" + ("local-ip" if feature == "local-up" else feature)].add(path.relative_to(root).as_posix())
        elif report_type == "benchmark":
            name = path.name.removesuffix(".json")
            sources[name].add(path.relative_to(root).as_posix())
        elif report_type == "fuzz":
            for sibling in sorted(path.parent.glob("*.json")):
                payload = _json(sibling)
                campaign = payload.get("campaign", payload) if payload else {}
                if isinstance(campaign, dict) and isinstance(campaign.get("target"), str):
                    sources["fuzz-" + campaign["target"]].add(path.relative_to(root).as_posix())
                    break

    for path in sorted(reports.rglob("*.json")):
        data = _json(path)
        if not data:
            continue
        campaign = data.get("campaign") if isinstance(data.get("campaign"), dict) else data
        target = campaign.get("target") if isinstance(campaign, dict) else None
        if isinstance(target, str) and ("fuzz" in path.parts or isinstance(data.get("workers"), list)):
            sources[f"fuzz-{target}"].add(path.relative_to(root).as_posix())

    procedures: list[dict[str, Any]] = []
    for procedure in sorted(sources):
        if procedure.startswith("smoke-"):
            family, runner = "smoke", RUNNERS["smoke"]
        elif procedure == "graft-hermes":
            family, runner = "smoke", RUNNERS["graft"]
        elif procedure == "colima-hermes-skills":
            family, runner = "smoke", RUNNERS["colima"]
        elif procedure.startswith("fuzz-"):
            family, runner = "fuzz", RUNNERS["fuzz"]
        else:
            family, runner = "benchmark", RUNNERS["benchmark"]
        procedures.append({
            "id": procedure, "family": family, "runner": runner,
            "sources": sorted(sources[procedure]),
        })
    return {"schema_version": 1, "procedures": procedures}


def _revision_date(root: Path, revision: str, fallback: str) -> str:
    result = subprocess.run(["git", "show", "-s", "--format=%ad", "--date=short", revision], cwd=root, capture_output=True, text=True, check=False)
    return result.stdout.strip() if result.returncode == 0 and result.stdout.strip() else fallback


def _report_step_names(root: Path, procedure: str) -> list[str]:
    names: set[str] = set()
    for path in (root / "site" / "reports").rglob("*.json"):
        data = _json(path)
        if not data:
            continue
        if isinstance(data.get("cases"), list):
            feature = data.get("feature")
            expected = ("graft-hermes" if procedure == "graft-hermes" else
                        "colima-hermes-skills" if procedure == "colima-hermes-skills" else
                        procedure.removeprefix("smoke-"))
            if feature == expected:
                names.update(str(item["name"]) for item in data["cases"] if isinstance(item, dict) and isinstance(item.get("name"), str))
        campaign = data.get("campaign") if isinstance(data.get("campaign"), dict) else data
        if procedure.startswith("fuzz-") and isinstance(data.get("workers"), list) and campaign.get("target") == procedure.removeprefix("fuzz-"):
            names.update(str(item.get("correlation_id")) for item in data["workers"] if isinstance(item, dict) and item.get("correlation_id"))
    if names:
        return sorted(names)
    if procedure.startswith("fuzz-"):
        return ["shape-probe", "template-probe", "boundary-probe", "differential-probe"]
    if procedure.startswith("smoke-"):
        return ["doctor", "advertised host", "smoke case results"]
    if procedure == "graft-hermes":
        return ["doctor", "graft outbound durable write and receiver round trip"]
    if procedure == "colima-hermes-skills":
        return ["doctor", "testbed ref", "skill procedure steps"]
    return ["sqlite", "uds", "tcp", "mTLS"]


def write_procedure_docs(root: Path = ROOT) -> None:
    inventory = build_inventory(root)
    head_result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True, check=True)
    head = head_result.stdout.strip()
    docs = root / "docs" / "procedures"; docs.mkdir(parents=True, exist_ok=True)
    for item in inventory["procedures"]:
        procedure = item["id"]
        revisions = []
        for path in item["sources"]:
            data = _json(root / path) if not path.startswith("runner:") else None
            candidate = data.get("source_revision") if data else None
            if not candidate and data and isinstance(data.get("campaign"), dict):
                candidate = data["campaign"].get("source_revision")
            if isinstance(candidate, str) and re.fullmatch(r"[0-9a-f]{40}", candidate):
                revisions.append(candidate)
        revisions.extend([head])
        unique = list(dict.fromkeys(revisions))
        runner = item["runner"]
        history = subprocess.run(["git", "log", "--follow", "--format=%H", "--", runner], cwd=root, capture_output=True, text=True, check=False).stdout.splitlines()
        anchor = history[-1] if history else head
        if anchor not in unique:
            unique.append(anchor)
        unique = list(dict.fromkeys(unique))
        entries = []
        for index, revision in enumerate(unique):
            fallback = "2026-09-12" if revision == head else "2026-08-01"
            entries.append((revision, _revision_date(root, revision, fallback), "current" if index == 0 else "historical runner revision"))
        entries.sort(key=lambda value: (value[1], value[0]), reverse=True)
        rows = _report_step_names(root, procedure)
        table = "| step | action | observable | evidence |\n| --- | --- | --- | --- |\n" + "".join(
            f"| {index} | Run `{name}` | PASS or FAIL is recorded | `{procedure}.json` cases |\n" for index, name in enumerate(rows, 1)
        )
        flow = "```mermaid\nflowchart LR\n  setup[Prepare runner] --> steps[Execute procedure steps]\n  steps --> evidence[Write immutable evidence JSON and HTML]\n```"
        front = ["---", f"procedure: {procedure}", f"family: {item['family']}", f"runner: {item['runner']}", "evidence: site/reports/<run>/<procedure>.json", "revisions:"]
        for revision, revision_date, note in entries:
            front += [f"  - rev: {revision}", f"    date: {revision_date}", f"    note: \"{note}\""]
        body = ["---", "", "## What this test proves", f"The `{procedure}` procedure runs the named verification steps in order. Each step records an observable result in the run evidence. The page is addressed by the runner revision so a historical report remains explainable after the runner changes.", "", "## Flow", flow, "", "## Steps", table, "", "## Evidence layout", "The runner writes a procedure JSON payload beside its rendered report and an envelope used by the public report index. Existing evidence artifacts are immutable.", "", "## Changes", "This page is backfilled from the runner history; revision-specific notes are listed below.", ""]
        for revision, revision_date, note in entries:
            body += [f"## Revision {revision[:8]} ({revision_date})", f"{note}.", "", flow, "", "## Steps", table, ""]
        (docs / f"{procedure}.md").write_text("\n".join(front + body), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--write-docs", action="store_true")
    args = parser.parse_args()
    if args.write_docs:
        write_procedure_docs(args.root.resolve())
    expected = json.dumps(build_inventory(args.root.resolve()), indent=2, sort_keys=True) + "\n"
    if args.check:
        return 0 if args.root.resolve().joinpath("docs/procedures/inventory.json").read_text(encoding="utf-8") == expected else 1
    destination = args.root.resolve() / "docs/procedures/inventory.json"
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(expected, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
