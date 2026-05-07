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


LINT_NAME = "manifests"
REQUIRED_WORKSPACE_FIELDS = (
    "edition",
    "rust-version",
    "authors",
    "license",
    "repository",
    "homepage",
)


@dataclass(frozen=True)
class ManifestViolation:
    location: str
    message: str

    def render(self) -> str:
        return f"{self.location}: {self.message}"


def tomllib_load(path: Path) -> dict:
    import tomllib

    return tomllib.loads(path.read_text(encoding="utf-8"))


def workspace_version(repo_root: Path) -> str:
    manifest = tomllib_load(repo_root / "Cargo.toml")
    package = manifest.get("workspace", {}).get("package", {})
    version = package.get("version")
    if not isinstance(version, str):
        raise SystemExit("workspace.package.version missing from Cargo.toml")
    return version


def expected_package_version(manifest: dict, workspace_version_value: str, manifest_label: str) -> str:
    package = manifest.get("package", {})
    if not isinstance(package, dict):
        raise SystemExit(f"{manifest_label} missing [package] table")

    version_value = package.get("version")
    if isinstance(version_value, str) and version_value.strip():
        return version_value
    if isinstance(version_value, dict) and version_value.get("workspace") is True:
        return workspace_version_value
    raise SystemExit(
        f"{manifest_label} must define [package].version either as a non-empty string or version.workspace = true"
    )


def member_manifests(repo_root: Path) -> list[Path]:
    return workspace_manifest_paths(repo_root)


def relative_manifest_display(manifest_path: Path, repo_root: Path) -> str:
    return manifest_path.relative_to(repo_root).as_posix()


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


def collect_manifest_violations(repo_root: Path) -> list[ManifestViolation]:
    violations: list[ManifestViolation] = []
    version = workspace_version(repo_root)
    manifests = member_manifests(repo_root)
    expected_versions: dict[Path, str] = {}

    for manifest_path in manifests:
        manifest = tomllib_load(manifest_path)
        rel_manifest = relative_manifest_display(manifest_path, repo_root)
        expected_versions[manifest_path.parent.resolve()] = expected_package_version(
            manifest,
            version,
            rel_manifest,
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

        for section_name, dependencies in dependency_sections(manifest):
            for dependency_name, dependency in dependencies.items():
                if not isinstance(dependency, dict):
                    continue
                dependency_path = dependency.get("path")
                if not isinstance(dependency_path, str):
                    continue
                resolved_path = (manifest_path.parent / dependency_path).resolve()
                expected_dependency_version = expected_versions.get(resolved_path)
                if expected_dependency_version is None:
                    continue
                dependency_path = dependency.get("path")
                pinned_version = dependency.get("version")
                if pinned_version != expected_dependency_version:
                    violations.append(
                        ManifestViolation(
                            f"{rel_manifest} [{section_name}.{dependency_name}]",
                            f'path dependency version must match target crate version "{expected_dependency_version}"',
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
