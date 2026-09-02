#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import sys

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import monotonic_now
from lint_common import print_report
from lint_common import workspace_crate_section_lines
from lint_common import workspace_manifest_paths
from check_version_sync import expected_package_version
from check_version_sync import validate_workspace_version


LINT_NAME = "manifests"
REQUIRED_WORKSPACE_FIELDS = (
    "edition",
    "rust-version",
    "authors",
    "license",
    "repository",
    "homepage",
)
DEPENDENCY_TABLES = frozenset(("dependencies", "dev-dependencies", "build-dependencies"))


@dataclass(frozen=True)
class ManifestViolation:
    location: str
    message: str

    def render(self) -> str:
        return f"{self.location}: {self.message}"


def tomllib_load(path: Path) -> dict:
    import tomllib

    return tomllib.loads(path.read_text(encoding="utf-8"))


def member_manifests(repo_root: Path) -> list[Path]:
    return workspace_manifest_paths(repo_root)


def relative_manifest_display(manifest_path: Path, repo_root: Path) -> str:
    return manifest_path.relative_to(repo_root).as_posix()


def iter_dependency_tables(table: dict, prefix: tuple[str, ...] = ()):
    """Yield every Cargo dependency table, including target-specific tables."""
    for key, value in table.items():
        if not isinstance(value, dict):
            continue
        path = (*prefix, key)
        if key in DEPENDENCY_TABLES:
            yield ".".join(path), value
        else:
            yield from iter_dependency_tables(value, path)


def dependency_centralization_violations(
    manifest: dict,
    rel_manifest: str,
    workspace_dependencies: dict,
) -> list[ManifestViolation]:
    """Require members to inherit dependencies declared in the workspace."""
    violations: list[ManifestViolation] = []
    for table_name, dependencies in iter_dependency_tables(manifest):
        for dependency_name, declaration in dependencies.items():
            if dependency_name not in workspace_dependencies:
                continue
            if isinstance(declaration, dict) and declaration.get("workspace") is True:
                continue
            violations.append(
                ManifestViolation(
                    f"{rel_manifest} [{table_name}].{dependency_name}",
                    "must use workspace = true for centralized dependency policy",
                )
            )
    return violations


def collect_manifest_violations(repo_root: Path) -> list[ManifestViolation]:
    violations: list[ManifestViolation] = []
    try:
        version = validate_workspace_version(repo_root)
    except SystemExit as error:
        return [ManifestViolation("version sync", str(error))]

    root_manifest = tomllib_load(repo_root / "Cargo.toml")
    workspace = root_manifest.get("workspace", {})
    workspace_dependencies = (
        workspace.get("dependencies", {}) if isinstance(workspace, dict) else {}
    )
    if not isinstance(workspace_dependencies, dict):
        workspace_dependencies = {}

    manifests = member_manifests(repo_root)
    for manifest_path in manifests:
        manifest = tomllib_load(manifest_path)
        rel_manifest = relative_manifest_display(manifest_path, repo_root)
        expected_version = expected_package_version(
            manifest,
            version,
            rel_manifest,
        )
        if expected_version != version:
            violations.append(
                ManifestViolation(
                    "version sync",
                    f"{rel_manifest} [package].version ({expected_version}) "
                    f"must equal expected workspace member version ({version})",
                )
            )
    for manifest_path in manifests:
        manifest = tomllib_load(manifest_path)
        rel_manifest = relative_manifest_display(manifest_path, repo_root)
        package = manifest.get("package", {})
        if not isinstance(package, dict):
            violations.append(ManifestViolation(rel_manifest, "missing [package] table"))
            continue

        for field in REQUIRED_WORKSPACE_FIELDS:
            field_value = package.get(field)
            if not (isinstance(field_value, dict) and field_value.get("workspace") is True):
                violations.append(
                    ManifestViolation(rel_manifest, f"set [package].{field}.workspace = true")
                )

        violations.extend(
            dependency_centralization_violations(
                manifest,
                rel_manifest,
                workspace_dependencies,
            )
        )

    return violations


def build_summary(violations: list[ManifestViolation]) -> str:
    if not violations:
        return "manifest policy satisfied"
    return f"manifest policy violated ({len(violations)} findings)"


def run(repo_root: Path) -> int:
    started_at = datetime.now(timezone.utc)
    started_monotonic = monotonic_now()
    violations = collect_manifest_violations(repo_root)
    duration_seconds = monotonic_now() - started_monotonic
    findings = [violation.render() for violation in violations]
    transcript_lines = workspace_crate_section_lines(repo_root)
    transcript_lines.extend(findings or ["no manifest violations found"])
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
    parser = argparse.ArgumentParser(description="Check Cargo manifest policy rules.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    return run(repo_root)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
