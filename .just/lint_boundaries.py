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
RUSQLITE_ALLOWED_MANIFEST = Path("crates/atm-rusqlite/Cargo.toml")
RUSQLITE_ALLOWED_SOURCE_ROOT = Path("crates/atm-rusqlite")
ATM_CORE_MANIFEST = Path("crates/atm-core/Cargo.toml")
ATM_RUSQLITE_PACKAGE = "atm-rusqlite"
ATM_RUSQLITE_ALLOWED_SECTIONS = {"dev-dependencies"}
BOUNDARY_DOC_GLOB = "docs/*/boundaries.md"
YAML_FENCE_START = "```yaml"
YAML_FENCE_END = "```"
SOURCE_IMPORT_PATTERNS = (
    re.compile(r"\brusqlite::"),
    re.compile(r"\buse\s+rusqlite\b"),
    re.compile(r"\bextern\s+crate\s+rusqlite\b"),
)
REQUIRED_BOUNDARY_FIELDS = (
    ("boundary_id",),
    ("owner_package",),
    ("owner_crate_path",),
    ("name",),
    ("implementation", "visibility"),
    ("implementation", "constructor"),
    ("dependencies", "forbidden_edges"),
    ("references", "scope"),
    ("references", "forbidden"),
    ("enforcement", "lint_rules"),
    ("status", "state"),
)
ENFORCED_RECORD_STATES = {"active"}


@dataclass(frozen=True)
class BoundaryViolation:
    location: str
    message: str

    def render(self) -> str:
        return f"{self.location}: {self.message}"


@dataclass(frozen=True)
class BoundaryRecord:
    boundary_id: str
    owner_package: str
    owner_crate_path: str
    status_state: str
    forbidden_edges: tuple[str, ...]
    source_path: Path
    start_line: int
    raw: dict[str, object]

    @property
    def is_enforced(self) -> bool:
        return self.status_state.lower() in ENFORCED_RECORD_STATES


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


def boundary_docs(repo_root: Path) -> list[Path]:
    return sorted(repo_root.glob(BOUNDARY_DOC_GLOB))


def tomllib_load(path: Path) -> dict:
    import tomllib

    return tomllib.loads(path.read_text(encoding="utf-8"))


def yaml_scalar(value: str) -> object:
    stripped = value.strip()
    if stripped == "null":
        return None
    if stripped == "[]":
        return []
    if stripped.lower() == "true":
        return True
    if stripped.lower() == "false":
        return False
    return stripped


def leading_spaces(text: str) -> int:
    return len(text) - len(text.lstrip(" "))


def next_content_line(lines: list[tuple[int, str]], start_index: int) -> tuple[int, str] | None:
    for index in range(start_index, len(lines)):
        line_number, text = lines[index]
        if text.strip():
            return line_number, text
    return None


def parse_yaml_list(lines: list[tuple[int, str]], start_index: int, indent: int) -> tuple[list[object], int]:
    items: list[object] = []
    index = start_index
    while index < len(lines):
        _line_number, text = lines[index]
        if not text.strip():
            index += 1
            continue
        current_indent = leading_spaces(text)
        if current_indent < indent:
            break
        if current_indent > indent:
            raise ValueError(f"unexpected indentation in list item: {text!r}")
        stripped = text.strip()
        if not stripped.startswith("- "):
            break
        items.append(yaml_scalar(stripped[2:]))
        index += 1
    return items, index


def parse_yaml_mapping(lines: list[tuple[int, str]], start_index: int, indent: int) -> tuple[dict[str, object], int]:
    mapping: dict[str, object] = {}
    index = start_index
    while index < len(lines):
        _line_number, text = lines[index]
        if not text.strip():
            index += 1
            continue

        current_indent = leading_spaces(text)
        if current_indent < indent:
            break
        if current_indent > indent:
            raise ValueError(f"unexpected indentation in mapping: {text!r}")

        stripped = text.strip()
        if stripped.startswith("- "):
            raise ValueError(f"unexpected list item in mapping: {text!r}")
        if ":" not in stripped:
            raise ValueError(f"expected key/value pair: {text!r}")

        key, remainder = stripped.split(":", 1)
        remainder = remainder.strip()
        index += 1

        if remainder:
            mapping[key] = yaml_scalar(remainder)
            continue

        next_line = next_content_line(lines, index)
        if next_line is None:
            mapping[key] = {}
            continue

        _next_line_number, next_text = next_line
        next_indent = leading_spaces(next_text)
        if next_indent <= current_indent:
            mapping[key] = {}
            continue

        if next_text.strip().startswith("- "):
            value, index = parse_yaml_list(lines, index, next_indent)
        else:
            value, index = parse_yaml_mapping(lines, index, next_indent)
        mapping[key] = value

    return mapping, index


def parse_simple_yaml_document(text: str) -> dict[str, object]:
    lines = [(line_number, line) for line_number, line in enumerate(text.splitlines(), start=1)]
    mapping, _ = parse_yaml_mapping(lines, 0, 0)
    return mapping


def extract_yaml_blocks(path: Path) -> list[tuple[int, str]]:
    blocks: list[tuple[int, str]] = []
    lines = path.read_text(encoding="utf-8").splitlines()
    in_block = False
    start_line = 0
    buffer: list[str] = []
    for line_number, line in enumerate(lines, start=1):
        stripped = line.strip()
        if not in_block and stripped == YAML_FENCE_START:
            in_block = True
            start_line = line_number + 1
            buffer = []
            continue
        if in_block and stripped == YAML_FENCE_END:
            blocks.append((start_line, "\n".join(buffer)))
            in_block = False
            buffer = []
            continue
        if in_block:
            buffer.append(line)
    return blocks


def nested_get(data: dict[str, object], path: tuple[str, ...]) -> object | None:
    current: object = data
    for segment in path:
        if not isinstance(current, dict):
            return None
        current = current.get(segment)
    return current


def validate_boundary_record_data(data: dict[str, object]) -> list[str]:
    errors: list[str] = []
    for path in REQUIRED_BOUNDARY_FIELDS:
        value = nested_get(data, path)
        if value is None:
            errors.append(f"missing required field: {'.'.join(path)}")
            continue
        if isinstance(value, str) and not value.strip():
            errors.append(f"missing required field: {'.'.join(path)}")

    public_trait = nested_get(data, ("public", "trait"))
    public_facade = nested_get(data, ("public", "facade"))
    if public_trait is None and public_facade is None:
        errors.append("missing required field: public.(trait|facade)")
    return errors


def boundary_record_from_data(
    *,
    data: dict[str, object],
    source_path: Path,
    start_line: int,
) -> BoundaryRecord:
    forbidden_edges = nested_get(data, ("dependencies", "forbidden_edges"))
    if not isinstance(forbidden_edges, list):
        forbidden_edges = []
    status_state = nested_get(data, ("status", "state"))
    if not isinstance(status_state, str):
        status_state = "planned"
    boundary_id = nested_get(data, ("boundary_id",))
    owner_package = nested_get(data, ("owner_package",))
    owner_crate_path = nested_get(data, ("owner_crate_path",))
    if not isinstance(boundary_id, str) or not isinstance(owner_package, str) or not isinstance(owner_crate_path, str):
        raise ValueError("boundary record missing required identifying fields")
    return BoundaryRecord(
        boundary_id=boundary_id,
        owner_package=owner_package,
        owner_crate_path=owner_crate_path,
        status_state=status_state,
        forbidden_edges=tuple(str(edge) for edge in forbidden_edges),
        source_path=source_path,
        start_line=start_line,
        raw=data,
    )


def parse_boundary_records(repo_root: Path) -> tuple[list[BoundaryRecord], list[BoundaryViolation]]:
    records: list[BoundaryRecord] = []
    violations: list[BoundaryViolation] = []
    for doc_path in boundary_docs(repo_root):
        rel_doc = doc_path.relative_to(repo_root).as_posix()
        for start_line, yaml_text in extract_yaml_blocks(doc_path):
            try:
                data = parse_simple_yaml_document(yaml_text)
            except ValueError as error:
                violations.append(BoundaryViolation(f"{rel_doc}:{start_line}", f"invalid boundary YAML: {error}"))
                continue

            record_errors = validate_boundary_record_data(data)
            if record_errors:
                for error in record_errors:
                    violations.append(BoundaryViolation(f"{rel_doc}:{start_line}", error))
                continue

            try:
                records.append(
                    boundary_record_from_data(
                        data=data,
                        source_path=doc_path.relative_to(repo_root),
                        start_line=start_line,
                    )
                )
            except ValueError as error:
                violations.append(BoundaryViolation(f"{rel_doc}:{start_line}", str(error)))
    return records, violations


def crate_manifest_aliases(manifest_path: Path) -> set[str]:
    manifest = tomllib_load(manifest_path)
    package = manifest.get("package", {})
    package_name = package.get("name")
    aliases = {manifest_path.parent.name}
    if isinstance(package_name, str):
        aliases.add(package_name)
    return aliases


def manifest_by_alias(repo_root: Path) -> dict[str, Path]:
    aliases: dict[str, Path] = {}
    for manifest_path in workspace_manifests(repo_root):
        for alias in crate_manifest_aliases(manifest_path):
            aliases[alias] = manifest_path
    return aliases


def collect_forbidden_edge_violations(
    repo_root: Path,
    records: list[BoundaryRecord],
) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    alias_map = manifest_by_alias(repo_root)
    for record in records:
        if not record.is_enforced:
            continue
        for edge in record.forbidden_edges:
            if "->" not in edge:
                rel_doc = record.source_path.as_posix()
                violations.append(
                    BoundaryViolation(
                        f"{rel_doc}:{record.start_line}",
                        f"{record.boundary_id} has invalid forbidden edge {edge!r}",
                    )
                )
                continue
            left_alias, right_alias = (part.strip() for part in edge.split("->", 1))
            left_manifest = alias_map.get(left_alias)
            if left_manifest is None:
                continue
            manifest = tomllib_load(left_manifest)
            rel_manifest_path = left_manifest.relative_to(repo_root)
            rel_manifest = rel_manifest_path.as_posix()
            for section_name, dependencies in dependency_sections(manifest):
                for dependency_name, dependency in dependencies.items():
                    package_name = dependency_package_name(dependency_name, dependency)
                    dependency_aliases = {dependency_name, package_name}
                    if right_alias in dependency_aliases:
                        violations.append(
                            BoundaryViolation(
                                f"{rel_manifest} [{section_name}]",
                                f"{record.boundary_id} forbids edge {left_alias} -> {right_alias}",
                            )
                        )
    return violations


def collect_boundary_violations(repo_root: Path) -> list[BoundaryViolation]:
    violations: list[BoundaryViolation] = []
    boundary_records, record_violations = parse_boundary_records(repo_root)
    violations.extend(record_violations)
    violations.extend(collect_forbidden_edge_violations(repo_root, boundary_records))

    for manifest_path in workspace_manifests(repo_root):
        manifest = tomllib_load(manifest_path)
        rel_manifest_path = manifest_path.relative_to(repo_root)
        rel_manifest = rel_manifest_path.as_posix()
        for section_name, dependencies in dependency_sections(manifest):
            for dependency_name, dependency in dependencies.items():
                package_name = dependency_package_name(dependency_name, dependency)
                if package_name == "rusqlite" and rel_manifest_path != RUSQLITE_ALLOWED_MANIFEST:
                    violations.append(
                        BoundaryViolation(
                            f"{rel_manifest} [{section_name}]",
                            "only crates/atm-rusqlite may depend on rusqlite",
                        )
                    )

                if (
                    rel_manifest_path == ATM_CORE_MANIFEST
                    and package_name == ATM_RUSQLITE_PACKAGE
                    and section_name not in ATM_RUSQLITE_ALLOWED_SECTIONS
                ):
                    violations.append(
                        BoundaryViolation(
                            f"{rel_manifest} [{section_name}]",
                            "atm-core may reference atm-rusqlite only in dev-dependencies",
                        )
                    )

    allowed_source_root = (repo_root / RUSQLITE_ALLOWED_SOURCE_ROOT).resolve()
    for source_path in rust_sources(repo_root):
        if allowed_source_root in source_path.resolve().parents:
            continue
        rel_source = source_path.relative_to(repo_root).as_posix()
        text = source_path.read_text(encoding="utf-8")
        for line_number, line in enumerate(text.splitlines(), start=1):
            if any(pattern.search(line) for pattern in SOURCE_IMPORT_PATTERNS):
                violations.append(
                    BoundaryViolation(
                        f"{rel_source}:{line_number}",
                        "only crates/atm-rusqlite source may import rusqlite directly",
                    )
                )

    return dedupe_violations(violations)


def dedupe_violations(violations: list[BoundaryViolation]) -> list[BoundaryViolation]:
    unique: dict[tuple[str, str], BoundaryViolation] = {}
    for violation in violations:
        unique[(violation.location, violation.message)] = violation
    return list(unique.values())


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
