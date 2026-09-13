#!/usr/bin/env python3
"""Build the closed procedure inventory from runners and committed evidence."""

from __future__ import annotations

import argparse
from collections import defaultdict
import json
from pathlib import Path
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


def _smoke_feature(path: Path, data: dict[str, Any]) -> str | None:
    """Read a result's own feature or the immutable run-directory suffix."""
    feature = data.get("feature")
    if isinstance(feature, str):
        return feature
    run_name = path.parent.name
    for candidate in sorted(FEATURES, key=len, reverse=True):
        if run_name.endswith("-" + candidate):
            return candidate
    return None


def _procedure_for_feature(feature: str) -> str:
    if feature in {"graft-hermes", "colima-hermes-skills"}:
        return feature
    return "smoke-" + ("local-ip" if feature == "local-up" else feature)


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
            feature = _smoke_feature(path, data)
            if feature:
                sources["graft-hermes" if feature == "graft-hermes" else
                        "colima-hermes-skills" if feature == "colima-hermes-skills" else
                        "smoke-" + ("local-ip" if feature == "local-up" else feature)].add(path.relative_to(root).as_posix())
        elif (
            isinstance(data.get("feature"), str)
            and isinstance(data.get("cases"), list)
            and isinstance(data.get("run_id"), str)
        ):
            sources[_procedure_for_feature(data["feature"])].add(path.relative_to(root).as_posix())
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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    expected = json.dumps(build_inventory(args.root.resolve()), indent=2, sort_keys=True) + "\n"
    if args.check:
        return 0 if args.root.resolve().joinpath("docs/procedures/inventory.json").read_text(encoding="utf-8") == expected else 1
    destination = args.root.resolve() / "docs/procedures/inventory.json"
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(expected, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
