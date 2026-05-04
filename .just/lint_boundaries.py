#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import re
import sys

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import monotonic_now
from lint_common import print_report


LINT_NAME = "boundaries"
RUSQLITE_ALLOWED_MANIFEST = "crates/atm-rusqlite/Cargo.toml"
RUSQLITE_ALLOWED_SOURCE_ROOT = "crates/atm-rusqlite"
ATM_CORE_MANIFEST = "crates/atm-core/Cargo.toml"
ATM_RUSQLITE_PACKAGE = "atm-rusqlite"
ATM_RUSQLITE_ALLOWED_SECTIONS = {"dev-dependencies"}
SOURCE_IMPORT_PATTERNS = (
    re.compile(r"\brusqlite::"),
    re.compile(r"\buse\s+rusqlite\b"),
    re.compile(r"\bextern\s+crate\s+rusqlite\b"),
)


@dataclass(frozen=True)
class BoundaryViolation:
    location: str
    message: str

    def render(self) -> str:
        return f"{self.location}: {self.message}"


def dependency_sections(manifest: dict) -> list[tuple[str, dict]]:
    sections: list[tuple[str, dict]] = []
    for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
        dependencies = manifest.get(section_name)
        if isinstance(dependencies, dict):
            sections.append((section_name, dependencies))

    targets = manifest.get("target", {})
    if isinstance(targets, dict):
        for target_name, target in targets.items():
            if not isinstance(target, dict):
                continue
            for section_name in ("dependencies", "dev-dependencies", "build-dependencies"):
                dependencies = target.get(section_name)
                if isinstance(dependencies, dict):
                    sections.append((f"target.{target_name}.{section_name}", dependencies))

    return sections


def dependency_package_name(dependency_name: str, dependency: object) -> str:
    if isinstance(dependency, str):
        return dependency_name
    if isinstance(dependency, dict):
        package_name = dependency.get("package")
        if isinstance(package_name, str):
            return package_name
    return dependency_name


def workspace_manifests(repo_root: Path) -> list[Path]:
    return sorted((repo_root / "crates").glob("*/Cargo.toml"))


def rust_sources(repo_root: Path) -> list[Path]:
    return sorted((repo_root / "crates").glob("**/*.rs"))


def collect_boundary_violations(repo_root: Path) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    for manifest_path in workspace_manifests(repo_root):
        manifest = tomllib_load(manifest_path)
        rel_manifest = str(manifest_path.relative_to(repo_root))
        for section_name, dependencies in dependency_sections(manifest):
            for dependency_name, dependency in dependencies.items():
                package_name = dependency_package_name(dependency_name, dependency)
                if package_name == "rusqlite" and rel_manifest != RUSQLITE_ALLOWED_MANIFEST:
                    violations.append(
                        BoundaryViolation(
                            f"{rel_manifest} [{section_name}]",
                            "only crates/atm-rusqlite may depend on rusqlite",
                        )
                    )

                if (
                    rel_manifest == ATM_CORE_MANIFEST
                    and package_name == ATM_RUSQLITE_PACKAGE
                    and section_name not in ATM_RUSQLITE_ALLOWED_SECTIONS
                ):
                    violations.append(
                        BoundaryViolation(
                            f"{rel_manifest} [{section_name}]",
                            "atm-core may reference atm-rusqlite only in dev-dependencies",
                        )
                    )

    allowed_source_root = repo_root / RUSQLITE_ALLOWED_SOURCE_ROOT
    for source_path in rust_sources(repo_root):
        if allowed_source_root in source_path.parents:
            continue
        rel_source = str(source_path.relative_to(repo_root))
        text = source_path.read_text(encoding="utf-8")
        for line_number, line in enumerate(text.splitlines(), start=1):
            if any(pattern.search(line) for pattern in SOURCE_IMPORT_PATTERNS):
                violations.append(
                    BoundaryViolation(
                        f"{rel_source}:{line_number}",
                        "only crates/atm-rusqlite source may import rusqlite directly",
                    )
                )

    return violations


def tomllib_load(path: Path) -> dict:
    import tomllib

    return tomllib.loads(path.read_text(encoding="utf-8"))


def build_summary(violations: list[BoundaryViolation]) -> str:
    if not violations:
        return "boundary rules satisfied"
    return f"boundary rules violated ({len(violations)} findings)"


def run(repo_root: Path) -> int:
    started_at = datetime.now(timezone.utc)
    started_monotonic = monotonic_now()
    violations = collect_boundary_violations(repo_root)
    duration_seconds = monotonic_now() - started_monotonic
    findings = [violation.render() for violation in violations]
    transcript_lines = findings or ["no boundary violations found"]
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=not violations,
        summary=build_summary(violations),
        findings=findings,
        transcript_lines=transcript_lines,
        started_at=started_at,
        duration_seconds=duration_seconds,
    )
    print_report(report, repo_root=repo_root, preview_limit=4, direct_threshold=4)
    return 0 if report.passed else 1


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Check crate and source boundary rules.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    return run(repo_root)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
