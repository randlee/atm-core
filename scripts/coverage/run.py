#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import json
import os
import platform as host_platform_module
import subprocess
import sys
import tempfile
import time

from jinja2 import Environment, FileSystemLoader, StrictUndefined


TRACKED_PLATFORMS = ("mac", "win")


@dataclass(frozen=True)
class CoveragePaths:
    reports_root: Path
    latest_markdown: Path
    timestamped_markdown: Path
    timestamped_json: Path


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def ensure_parent(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)


def write_text_atomic(path: Path, text: str) -> None:
    ensure_parent(path)
    temp_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            "w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as handle:
            handle.write(text)
            temp_path = Path(handle.name)
        os.replace(temp_path, path)
    finally:
        if temp_path is not None:
            temp_path.unlink(missing_ok=True)


def timestamp_slug(now: datetime | None = None) -> str:
    moment = now or datetime.now(timezone.utc)
    return moment.strftime("%Y-%m-%d-%H-%M-%S")


def current_commit(root: Path | None = None) -> str:
    working_root = root or repo_root()
    result = subprocess.run(
        ["git", "-C", str(working_root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def coverage_paths(platform_slug: str, stamp: str) -> CoveragePaths:
    root = repo_root()
    reports_root = root / "reports" / "coverage"
    return CoveragePaths(
        reports_root=reports_root,
        latest_markdown=reports_root / f"{platform_slug}.md",
        timestamped_markdown=reports_root / f"{stamp}-{platform_slug}.md",
        timestamped_json=reports_root / f"{stamp}-{platform_slug}.json",
    )


def detect_platform() -> str:
    system = host_platform_module.system()
    if system == "Darwin":
        return "mac"
    if system == "Windows":
        return "win"
    raise RuntimeError(
        "coverage reporting currently supports only macOS and Windows host runs for tracked latest artifacts; Linux tracked-latest coverage is deferred/unsupported in the current Phase Z line"
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run explicit local coverage reporting.")
    parser.add_argument("--write-artifacts", action="store_true")
    parser.add_argument("--timestamp", default=None, help="Reuse an explicit timestamp slug.")
    return parser.parse_args()


def run_coverage_export(root: Path, output_path: Path, target_dir: Path) -> None:
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(target_dir)
    subprocess.run(
        [
            "cargo",
            "llvm-cov",
            "--workspace",
            "--json",
            "--summary-only",
            "--output-path",
            str(output_path),
        ],
        cwd=root,
        env=env,
        check=True,
    )


def aggregate_metric(sum_count: int, sum_covered: int) -> dict[str, float | int]:
    percent = 100.0 if sum_count == 0 else (sum_covered / sum_count) * 100.0
    return {
        "count": sum_count,
        "covered": sum_covered,
        "percent": round(percent, 2),
    }


def crate_name_for_filename(filename: str) -> str | None:
    parts = Path(filename).parts
    if "crates" in parts:
        index = parts.index("crates")
        if index + 1 < len(parts):
            return parts[index + 1]
    return None


def build_payload(export: dict[str, object], *, platform_slug: str, started_at: datetime, duration_secs: float) -> dict[str, object]:
    data = export["data"][0]
    files = data["files"]
    totals = data["totals"]

    crate_totals: dict[str, dict[str, int]] = {}
    for file_record in files:
        crate = crate_name_for_filename(file_record["filename"])
        if crate is None:
            continue
        bucket = crate_totals.setdefault(
            crate,
            {
                "line_count": 0,
                "line_covered": 0,
                "function_count": 0,
                "function_covered": 0,
            },
        )
        summary = file_record["summary"]
        bucket["line_count"] += summary["lines"]["count"]
        bucket["line_covered"] += summary["lines"]["covered"]
        bucket["function_count"] += summary["functions"]["count"]
        bucket["function_covered"] += summary["functions"]["covered"]

    crates = []
    for crate_name in sorted(crate_totals):
        bucket = crate_totals[crate_name]
        line_metric = aggregate_metric(bucket["line_count"], bucket["line_covered"])
        function_metric = aggregate_metric(bucket["function_count"], bucket["function_covered"])
        crates.append(
            {
                "name": crate_name,
                "line_percent": line_metric["percent"],
                "function_percent": function_metric["percent"],
                "line_count": line_metric["count"],
                "line_covered": line_metric["covered"],
                "function_count": function_metric["count"],
                "function_covered": function_metric["covered"],
            }
        )

    return {
        "platform": platform_slug,
        "coverage_level": "local-explicit",
        "timestamp": started_at.isoformat(),
        "timestamp_display": started_at.isoformat(),
        "commit": current_commit(),
        "duration_secs": round(duration_secs, 2),
        "collector": "cargo llvm-cov",
        "status": "passed",
        "summary": {
            "line_percent": round(totals["lines"]["percent"], 2),
            "function_percent": round(totals["functions"]["percent"], 2),
            "line_count": totals["lines"]["count"],
            "line_covered": totals["lines"]["covered"],
            "function_count": totals["functions"]["count"],
            "function_covered": totals["functions"]["covered"],
        },
        "crates": crates,
        "notes": [
            "tracked latest report is overwritten only for the host platform that executed the run",
            "other tracked platform reports remain unchanged unless missing, in which case a placeholder is created",
        ],
    }


def placeholder_payload(platform_slug: str, commit: str) -> dict[str, object]:
    return {
        "platform": platform_slug,
        "coverage_level": "local-explicit",
        "timestamp": None,
        "timestamp_display": "N/A",
        "commit": commit,
        "duration_secs": 0.0,
        "collector": "cargo llvm-cov",
        "status": "not-run-on-this-platform",
        "summary": None,
        "crates": [],
        "notes": [
            f"no {platform_slug} host coverage run has updated this tracked latest report on the current branch yet"
        ],
    }


def render_markdown(payload: dict[str, object]) -> str:
    root = repo_root()
    env = Environment(
        loader=FileSystemLoader(str(root / "templates" / "coverage-report")),
        undefined=StrictUndefined,
        trim_blocks=True,
        lstrip_blocks=True,
    )
    template = env.get_template(f"{payload['platform']}.md.j2")
    return template.render(report=payload).rstrip() + "\n"


def write_json(path: Path, payload: object) -> None:
    write_text_atomic(path, json.dumps(payload, indent=2) + "\n")


def should_refresh_placeholder(path: Path) -> bool:
    if not path.exists():
        return True
    existing = path.read_text(encoding="utf-8")
    return "`not-run-on-this-platform`" in existing or "`PENDING`" in existing


def main() -> int:
    args = parse_args()
    root = repo_root()
    platform_slug = detect_platform()
    started_at = datetime.now(timezone.utc)
    stamp = args.timestamp or timestamp_slug(started_at)
    started = time.perf_counter()

    with tempfile.TemporaryDirectory(prefix="atm-coverage-run.") as temp_dir:
        temp_root = Path(temp_dir)
        export_path = temp_root / "coverage.json"
        run_coverage_export(root, export_path, temp_root / "target")
        export = json.loads(export_path.read_text(encoding="utf-8"))

    payload = build_payload(
        export,
        platform_slug=platform_slug,
        started_at=started_at,
        duration_secs=time.perf_counter() - started,
    )
    markdown = render_markdown(payload)

    if args.write_artifacts:
        actual_paths = coverage_paths(platform_slug, stamp)
        ensure_parent(actual_paths.latest_markdown)
        write_text_atomic(actual_paths.latest_markdown, markdown)
        write_text_atomic(actual_paths.timestamped_markdown, markdown)
        write_json(actual_paths.timestamped_json, payload)

        commit = payload["commit"]
        for other_platform in TRACKED_PLATFORMS:
            if other_platform == platform_slug:
                continue
            other_paths = coverage_paths(other_platform, stamp)
            if not should_refresh_placeholder(other_paths.latest_markdown):
                continue
            write_text_atomic(
                other_paths.latest_markdown,
                render_markdown(placeholder_payload(other_platform, commit)),
            )
    else:
        print(markdown, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
