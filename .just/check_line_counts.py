#!/usr/bin/env python3
from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
import sys

from lint_common import classify_rust_test_scope
from lint_common import discover_repo_root
from lint_common import is_code_line
from lint_common import load_lint_config


@dataclass(frozen=True)
class LineLimitConfig:
    max_total_lines: int | None = None
    max_production_lines: int | None = 1000
    max_scoped_code_lines: int | None = None
    exclusions: dict[str, str] | None = None


@dataclass(frozen=True)
class FileCounts:
    crate_name: str
    path: str
    total_lines: int
    production_lines: int
    test_lines: int

    @property
    def scoped_code_lines(self) -> int:
        return self.production_lines + self.test_lines


@dataclass(frozen=True)
class CrateTotals:
    crate_name: str
    total_lines: int
    production_lines: int
    test_lines: int

    @property
    def scoped_code_lines(self) -> int:
        return self.production_lines + self.test_lines


def classify_lines(lines: list[str]) -> tuple[int, int]:
    test_scope = classify_rust_test_scope(lines)
    production_lines = 0
    test_lines = 0
    for line, in_test_scope in zip(lines, test_scope, strict=True):
        if not is_code_line(line):
            continue
        if in_test_scope:
            test_lines += 1
        else:
            production_lines += 1
    return production_lines, test_lines


def load_config(repo_root: Path) -> LineLimitConfig:
    config = load_lint_config(repo_root).get("line_counts", {})
    if not isinstance(config, dict):
        raise SystemExit("[line_counts] must be a TOML table")

    def parse_limit(name: str) -> int | None:
        value = config.get(name)
        if value in (None, 0):
            return None
        if not isinstance(value, int) or value < 0:
            raise SystemExit(f"[line_counts].{name} must be a non-negative integer")
        return value

    exclusions = config.get("exclusions", {})
    if not isinstance(exclusions, dict):
        raise SystemExit("[line_counts.exclusions] must be a TOML table")

    return LineLimitConfig(
        max_total_lines=parse_limit("max_total_lines"),
        max_production_lines=parse_limit("max_production_lines"),
        max_scoped_code_lines=parse_limit("max_scoped_code_lines"),
        exclusions={str(path): str(reason) for path, reason in exclusions.items()},
    )


def collect_file_counts(repo_root: Path, config: LineLimitConfig) -> list[FileCounts]:
    results: list[FileCounts] = []
    for path in sorted((repo_root / "crates").rglob("*.rs")):
        rel = path.relative_to(repo_root)
        rel_posix = rel.as_posix()
        if "/tests/" in rel_posix:
            continue
        if "/src/" not in rel_posix:
            continue
        if config.exclusions and rel_posix in config.exclusions:
            continue

        lines = path.read_text(encoding="utf-8").splitlines()
        production_lines, test_lines = classify_lines(lines)
        results.append(
            FileCounts(
                crate_name=rel.parts[1],
                path=rel_posix,
                total_lines=len(lines),
                production_lines=production_lines,
                test_lines=test_lines,
            )
        )
    return results


def crate_totals(counts: list[FileCounts]) -> list[CrateTotals]:
    totals: dict[str, CrateTotals] = {}
    for count in counts:
        previous = totals.get(
            count.crate_name,
            CrateTotals(
                crate_name=count.crate_name,
                total_lines=0,
                production_lines=0,
                test_lines=0,
            ),
        )
        totals[count.crate_name] = CrateTotals(
            crate_name=count.crate_name,
            total_lines=previous.total_lines + count.total_lines,
            production_lines=previous.production_lines + count.production_lines,
            test_lines=previous.test_lines + count.test_lines,
        )
    return [totals[name] for name in sorted(totals)]


def format_table(counts: list[FileCounts]) -> list[str]:
    if not counts:
        return ["no eligible source files found"]

    crate_width = max(len("crate"), max(len(item.crate_name) for item in counts))
    file_width = max(len("file"), max(len(Path(item.path).relative_to(f"crates/{item.crate_name}").as_posix()) for item in counts))
    total_width = max(len("total"), max(len(str(item.total_lines)) for item in counts))
    prod_width = max(len("prod"), max(len(str(item.production_lines)) for item in counts))
    test_width = max(len("test"), max(len(str(item.test_lines)) for item in counts))
    scoped_width = max(len("prod+test"), max(len(str(item.scoped_code_lines)) for item in counts))

    header = (
        f"{'crate':<{crate_width}}  "
        f"{'file':<{file_width}}  "
        f"{'total':>{total_width}}  "
        f"{'prod':>{prod_width}}  "
        f"{'test':>{test_width}}  "
        f"{'prod+test':>{scoped_width}}"
    )
    divider = "-" * len(header)
    rows = [header, divider]

    grouped: dict[str, list[FileCounts]] = {}
    for item in counts:
        grouped.setdefault(item.crate_name, []).append(item)

    for crate_name in sorted(grouped):
        crate_rows = grouped[crate_name]
        for item in crate_rows:
            file_name = Path(item.path).relative_to(f"crates/{crate_name}").as_posix()
            rows.append(
                f"{crate_name:<{crate_width}}  "
                f"{file_name:<{file_width}}  "
                f"{item.total_lines:>{total_width}}  "
                f"{item.production_lines:>{prod_width}}  "
                f"{item.test_lines:>{test_width}}  "
                f"{item.scoped_code_lines:>{scoped_width}}"
            )
        totals = crate_totals(crate_rows)[0]
        rows.append(
            f"{crate_name:<{crate_width}}  "
            f"{'TOTAL':<{file_width}}  "
            f"{totals.total_lines:>{total_width}}  "
            f"{totals.production_lines:>{prod_width}}  "
            f"{totals.test_lines:>{test_width}}  "
            f"{totals.scoped_code_lines:>{scoped_width}}"
        )
        rows.append("")

    return rows[:-1] if rows and rows[-1] == "" else rows


def evaluate_limits(counts: list[FileCounts], config: LineLimitConfig) -> list[str]:
    failures: list[str] = []
    for item in counts:
        if config.max_total_lines is not None and item.total_lines > config.max_total_lines:
            failures.append(
                f"{item.path}: total={item.total_lines} exceeds limit {config.max_total_lines}"
            )
        if (
            config.max_production_lines is not None
            and item.production_lines > config.max_production_lines
        ):
            failures.append(
                f"{item.path}: prod={item.production_lines} exceeds limit {config.max_production_lines}"
            )
        if (
            config.max_scoped_code_lines is not None
            and item.scoped_code_lines > config.max_scoped_code_lines
        ):
            failures.append(
                f"{item.path}: prod+test={item.scoped_code_lines} exceeds limit {config.max_scoped_code_lines}"
            )
    return failures


def limit_summary(config: LineLimitConfig) -> str:
    parts: list[str] = []
    if config.max_total_lines is not None:
        parts.append(f"total<={config.max_total_lines}")
    if config.max_production_lines is not None:
        parts.append(f"prod<={config.max_production_lines}")
    if config.max_scoped_code_lines is not None:
        parts.append(f"prod+test<={config.max_scoped_code_lines}")
    return ", ".join(parts) if parts else "no active limits"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Check Rust file size limits by crate.")
    parser.add_argument("--root", help="Repo root to inspect.")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    repo_root = discover_repo_root(args.root)
    config = load_config(repo_root)
    counts = collect_file_counts(repo_root, config)
    failures = evaluate_limits(counts, config)

    if failures:
        print("RULE-003 violation: source file size limits exceeded.")
        print(f"active limits: {limit_summary(config)}")
        print("files over limit:")
        for failure in failures:
            print(failure)
        print("")
        print("per-crate line counts:")
        for line in format_table(counts):
            print(line)
        print("")
        if config.exclusions:
            print("temporary exclusions:")
            for path, reason in config.exclusions.items():
                print(f"- {path} ({reason})")
        print(f"errors: {len(failures)}")
        return 1

    print("RULE-003 check passed: source file size limits satisfied.")
    print(f"active limits: {limit_summary(config)}")
    print("per-crate line counts:")
    for line in format_table(counts):
        print(line)
    if config.exclusions:
        print("")
        print("temporary exclusions:")
        for path, reason in config.exclusions.items():
            print(f"- {path} ({reason})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
