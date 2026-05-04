#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
import argparse
from pathlib import Path
import shutil
import subprocess
import sys

from lint_common import discover_repo_root
from lint_common import load_lint_config
from lint_common import workspace_crate_section_lines
from lint_common import workspace_crates


ERROR_SECTIONS = {"unlinked_files", "empty_files"}


@dataclass(frozen=True)
class ShearSection:
    name: str
    header: str
    body_lines: tuple[str, ...]
    file_paths: tuple[str, ...]


@dataclass(frozen=True)
class ShearPolicyFinding:
    section_name: str
    file_path: str
    reason: str


def build_command(repo_root: Path) -> list[str]:
    return ["cargo-shear"]


def parse_sections(stdout: str) -> list[ShearSection]:
    sections: list[ShearSection] = []
    current_name: str | None = None
    current_body: list[str] = []
    current_header = ""

    for line in stdout.splitlines():
        if line.startswith("shear/"):
            if current_name is not None:
                sections.append(
                    ShearSection(
                        name=current_name,
                        header=current_header,
                        body_lines=tuple(current_body),
                        file_paths=tuple(extract_file_paths(current_body)),
                    )
                )
            current_name = line.split("/", 1)[1].strip()
            current_header = line.strip()
            current_body = []
            continue
        if current_name is not None:
            current_body.append(line)

    if current_name is not None:
        sections.append(
            ShearSection(
                name=current_name,
                header=current_header,
                body_lines=tuple(current_body),
                file_paths=tuple(extract_file_paths(current_body)),
            )
        )
    return sections


def extract_file_paths(body_lines: list[str] | tuple[str, ...]) -> list[str]:
    paths: list[str] = []
    for line in body_lines:
        stripped = line.strip()
        if not stripped.startswith("│ "):
            continue
        candidate = stripped[2:].strip()
        if not candidate:
            continue
        if "/" not in candidate and "\\" not in candidate:
            continue
        paths.append(candidate.replace("\\", "/"))
    return paths


def package_to_crate_dir(repo_root: Path) -> dict[str, str]:
    return {
        crate.package_name: Path(crate.manifest_path).parent.as_posix()
        for crate in workspace_crates(repo_root)
    }


def load_policy_config(repo_root: Path) -> dict[str, dict[str, str]]:
    config = load_lint_config(repo_root)
    cargo_shear = config.get("cargo_shear", {})
    if not isinstance(cargo_shear, dict):
        return {"allowed_empty_files": {}, "allowed_unlinked_files": {}}

    def table(name: str) -> dict[str, str]:
        value = cargo_shear.get(name, {})
        if not isinstance(value, dict):
            return {}
        normalized: dict[str, str] = {}
        for key, reason in value.items():
            if isinstance(key, str) and isinstance(reason, str):
                normalized[key.replace("\\", "/")] = reason
        return normalized

    return {
        "allowed_empty_files": table("allowed_empty_files"),
        "allowed_unlinked_files": table("allowed_unlinked_files"),
    }


def evaluate_policy(sections: list[ShearSection], policy: dict[str, dict[str, str]]) -> tuple[list[ShearPolicyFinding], list[str]]:
    findings: list[ShearPolicyFinding] = []
    downgraded: list[str] = []

    for section in sections:
        if section.name not in ERROR_SECTIONS:
            continue
        allowed = (
            policy["allowed_empty_files"]
            if section.name == "empty_files"
            else policy["allowed_unlinked_files"]
        )
        for file_path in section.file_paths:
            reason = allowed.get(file_path)
            if reason is not None:
                downgraded.append(
                    f"{section.name}: downgraded {file_path} ({reason})"
                )
                continue
            findings.append(
                ShearPolicyFinding(
                    section_name=section.name,
                    file_path=file_path,
                    reason=f"{section.name} must be fixed or explicitly downgraded in .just/lint-config.toml",
                )
            )
    return findings, downgraded


def render_policy_findings(findings: list[ShearPolicyFinding]) -> list[str]:
    rendered: list[str] = []
    for finding in findings:
        rendered.append(
            f"shear policy error: {finding.file_path} [{finding.section_name}] {finding.reason}"
        )
    return rendered


def annotate_sections(sections: list[ShearSection], repo_root: Path) -> list[str]:
    crate_map = package_to_crate_dir(repo_root)
    rendered: list[str] = []
    for section in sections:
        if not section.file_paths:
            continue
        crate_name = infer_package_name(section.body_lines)
        crate_dir = crate_map.get(crate_name, crate_name)
        for path in section.file_paths:
            rendered.append(f"shear note: {crate_dir}/{path} [{section.name}]")
    return rendered


def infer_package_name(body_lines: tuple[str, ...]) -> str:
    for line in body_lines:
        stripped = line.strip()
        if " in `" in stripped and stripped.endswith("`"):
            return stripped.split(" in `", 1)[1][:-1]
    return "unknown-package"


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Run cargo-shear for dependency pruning checks.")
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)

    if shutil.which("cargo-shear") is None:
        print("cargo-shear is not installed; install it to run this lint", file=sys.stderr)
        return 2

    for line in workspace_crate_section_lines(repo_root):
        print(line)

    completed = subprocess.run(
        build_command(repo_root),
        cwd=repo_root,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )

    stdout = completed.stdout
    sections = parse_sections(stdout)
    policy = load_policy_config(repo_root)
    policy_findings, downgraded = evaluate_policy(sections, policy)

    if completed.returncode != 0:
        if stdout:
            print(stdout, end="")
        if completed.stderr:
            print(completed.stderr, end="", file=sys.stderr)
        return completed.returncode

    if policy_findings:
        for line in render_policy_findings(policy_findings):
            print(line)
        for line in annotate_sections(sections, repo_root):
            if any(f.file_path in line for f in policy_findings):
                print(line)
        if downgraded:
            for line in downgraded:
                print(line)
        if stdout:
            print(stdout, end="")
        return 1

    if downgraded:
        for line in downgraded:
            print(line)
    if stdout:
        print(stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
