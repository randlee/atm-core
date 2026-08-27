#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import asdict
from dataclasses import dataclass
from datetime import datetime
from datetime import timezone
from pathlib import Path

from release_artifacts import installed_doc_members
from release_artifacts import installed_doc_source_files
from release_artifacts import installed_docs_source_root
from release_artifacts import load_manifest


REQUIRED_RELEASE_FILES = (
    "release/publish-artifacts.toml",
    "scripts/release_gate.sh",
    "scripts/release_artifacts.py",
    "docs/release-inventory-schema.json",
    "release/RELEASE-NOTES-TEMPLATE.md",
)
REQUIRED_RELEASE_BINARIES = ("atm", "atm-daemon")
INVENTORY_REQUIRED_TOP = ("releaseVersion", "releaseTag", "releaseCommit", "generatedAt", "items")
INVENTORY_REQUIRED_ITEM = ("artifact", "version", "sourceRef", "publishTarget", "verifyCommands", "required")
CHECK_DEP_CURRENCY_ENV = "ATMD_CHECK_DEP_CURRENCY"
GITHUB_ISSUE_ENV = "ATMD_GH_AUTOFIX_ISSUES"
PHASE_AE_STAGED_INSTALL_ROOT = Path("target/phase-ae/staged-install-root")
WYVERN_REPOSITORY = "randlee/wyvern"
WYVERN_PIN_FILES = (
    Path("scripts/send-to/atm-send-to.sh"),
    Path("scripts/send-to/atm-send-to.ps1"),
)
WYVERN_PIN_PATTERN = re.compile(r"(?:WYVERN_PIN|wyvernPin)\s*[=:]\s*[\"'](\d+\.\d+\.\d+)[\"']")
SEMVER_PATTERN = re.compile(r"(?<!\d)v?(\d+)\.(\d+)\.(\d+)(?!\d)")
SC_ECOSYSTEM_CARGO_DEPENDENCIES = (
    "sc-composer",
    "sc-observability",
    "sc-observability-types",
)
ECOSYSTEM_FIX_FORWARD_ENV = "ATMD_ECOSYSTEM_FIX_FORWARD"
ECOSYSTEM_KNOWN_GOOD_ENV = "ATMD_ECOSYSTEM_KNOWN_GOOD"
ECOSYSTEM_EVIDENCE_ENV = "ATMD_ECOSYSTEM_EVIDENCE"
AQ6_EVIDENCE_REGISTER = Path("docs/plans/phase-aq/evidence/AQ6/ecosystem-preflight.md")
ECOSYSTEM_CARGO_PIN_FILES = {
    "sc-composer": (
        (Path("crates/atm-template-sc-compose/Cargo.toml"), "sc-composer"),
        (Path("crates/atm-template-sc-compose/Cargo.toml"), "sc-sha"),
    ),
    "sc-observability": (
        (Path("Cargo.toml"), "sc-observability"),
        (Path("Cargo.toml"), "sc-observability-types"),
    ),
    "sc-observability-types": (
        (Path("Cargo.toml"), "sc-observability"),
        (Path("Cargo.toml"), "sc-observability-types"),
    ),
}


@dataclass
class Finding:
    check: str
    severity: str
    summary: str
    detail: str = ""
    command: list[str] | None = None
    exit_code: int | None = None

    @property
    def blocks(self) -> bool:
        return self.severity == "error"


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def workspace_version(root: Path) -> str:
    cargo = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    version = cargo.get("workspace", {}).get("package", {}).get("version")
    if not isinstance(version, str) or not version.strip():
        raise SystemExit("workspace.package.version missing from Cargo.toml")
    return version


def current_ref(root: Path) -> str:
    completed = subprocess.run(
        ["git", "rev-parse", "--abbrev-ref", "HEAD"],
        cwd=root,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if completed.returncode != 0:
        return "UNKNOWN"
    return completed.stdout.strip() or "UNKNOWN"


def run_capture(cmd: list[str], *, cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )


def append_completed_findings(
    findings: list[Finding],
    check: str,
    completed: subprocess.CompletedProcess[str],
    success_summary: str,
    failure_summary: str,
) -> None:
    if completed.returncode == 0:
        if completed.stdout.strip():
            sys.stdout.write(completed.stdout)
        if completed.stderr.strip():
            sys.stderr.write(completed.stderr)
        return
    if completed.stdout.strip():
        sys.stdout.write(completed.stdout)
    if completed.stderr.strip():
        sys.stderr.write(completed.stderr)
    findings.append(
        Finding(
            check=check,
            severity="error",
            summary=failure_summary,
            detail=(completed.stderr or completed.stdout).strip(),
            command=completed.args if isinstance(completed.args, list) else None,
            exit_code=completed.returncode,
        )
    )


def validate_support_files(root: Path, findings: list[Finding]) -> None:
    missing = [path for path in REQUIRED_RELEASE_FILES if not (root / path).exists()]
    if missing:
        findings.append(
            Finding(
                check="support-files",
                severity="error",
                summary="missing required release support files",
                detail=", ".join(missing),
            )
        )


def validate_lint(root: Path, findings: list[Finding]) -> None:
    if shutil.which("just") is not None:
        lint_cmd = ["just", "lint"]
        failure_summary = "just lint failed"
    else:
        # CI images or minimal local environments may not have `just` installed.
        lint_cmd = ["python3", ".just/run_lint.py", "all"]
        failure_summary = "lint runner failed"
    completed = run_capture(lint_cmd, cwd=root)
    append_completed_findings(
        findings,
        "lint",
        completed,
        "lint passed",
        failure_summary,
    )


def validate_cli_surface(root: Path, findings: list[Finding]) -> None:
    """Run the feature-gated ATM CLI compatibility contract explicitly.

    The inspector command is deliberately omitted from shipped binaries, so
    Cargo's ordinary workspace test invocation skips this integration target.
    Release validation must opt into the feature or released CLI drift would
    evade the phase-end gate.
    """
    completed = run_capture(
        [
            "cargo",
            "test",
            "-p",
            "agent-team-mail",
            "--features",
            "cli-surface-dump",
            "--test",
            "cli_surface",
        ],
        cwd=root,
    )
    append_completed_findings(
        findings,
        "cli-surface",
        completed,
        "CLI surface contract passed",
        "CLI surface contract failed",
    )


def validate_staged_install_docs(
    root: Path,
    findings: list[Finding],
    *,
    manifest_path: Path,
    staged_install_root: Path,
    release_version: str,
) -> None:
    manifest = load_manifest(manifest_path)
    entrypoint = staged_install_root / manifest["installed_docs"]["entrypoint"]
    if not entrypoint.is_file():
        findings.append(
            Finding(
                check="installed-docs-entrypoint",
                severity="error",
                summary="installed docs entrypoint is missing from the staged install root",
                detail=str(entrypoint.relative_to(root)),
            )
        )

    missing = [member for member in installed_doc_members(manifest_path) if not (staged_install_root / member).is_file()]
    if missing:
        findings.append(
            Finding(
                check="installed-docs-membership",
                severity="error",
                summary="staged install root is missing installed doc members",
                detail=", ".join(member.as_posix() for member in missing),
            )
        )

    verify_cmd = [
        "python3",
        "scripts/verify_user_docs.py",
        "--source-root",
        relpath_display(installed_docs_source_root(manifest_path), root),
        "--release-version",
        release_version,
    ]
    verify_cmd.extend(["--installed-root", str(staged_install_root / "share/doc/atm")])
    completed = run_capture(verify_cmd, cwd=root)
    append_completed_findings(
        findings,
        "installed-docs-verifier",
        completed,
        "installed docs verifier passed",
        "installed docs verifier failed",
    )


def validate_manifest(
    root: Path,
    findings: list[Finding],
    *,
    staged_install_root: Path | None,
    release_version: str,
) -> None:
    manifest_path = root / "release" / "publish-artifacts.toml"
    resolved_staged_install_root = ensure_staged_install_docs(
        root,
        manifest_path=manifest_path,
        staged_install_root=staged_install_root,
    )
    commands = (
        (
            "manifest-coverage",
            [
                "python3",
                "scripts/release_artifacts.py",
                "validate-manifest",
                "--manifest",
                "release/publish-artifacts.toml",
                "--workspace-toml",
                "Cargo.toml",
            ],
            "manifest coverage validation failed",
        ),
        (
            "preflight-modes",
            [
                "python3",
                "scripts/release_artifacts.py",
                "validate-preflight-checks",
                "--manifest",
                "release/publish-artifacts.toml",
                "--workspace-toml",
                "Cargo.toml",
            ],
            "preflight mode validation failed",
        ),
        (
            "publish-order",
            [
                "python3",
                "scripts/release_artifacts.py",
                "validate-publish-order",
                "--manifest",
                "release/publish-artifacts.toml",
                "--workspace-toml",
                "Cargo.toml",
            ],
            "publish-order validation failed",
        ),
    )
    for check, cmd, summary in commands:
        completed = run_capture(cmd, cwd=root)
        append_completed_findings(findings, check, completed, f"{check} passed", summary)
    validate_staged_install_docs(
        root,
        findings,
        manifest_path=manifest_path,
        staged_install_root=resolved_staged_install_root,
        release_version=release_version,
    )


def validate_release_binaries(root: Path, findings: list[Finding]) -> None:
    completed = run_capture(
        [
            "python3",
            "scripts/release_artifacts.py",
            "validate-release-binaries",
            "--manifest",
            "release/publish-artifacts.toml",
            *sum((["--required", binary] for binary in REQUIRED_RELEASE_BINARIES), []),
        ],
        cwd=root,
    )
    append_completed_findings(
        findings,
        "release-binaries",
        completed,
        "required release binaries validated",
        "required release binaries missing from manifest",
    )


def validate_publish_surface(
    root: Path,
    version: str,
    findings: list[Finding],
    *,
    enforce_release_version: bool,
) -> None:
    if enforce_release_version:
        unpublished = run_capture(
            [
                "python3",
                "scripts/release_artifacts.py",
                "check-version-unpublished",
                "--manifest",
                "release/publish-artifacts.toml",
                "--version",
                version,
            ],
            cwd=root,
        )
        append_completed_findings(
            findings,
            "publish-version-unpublished",
            unpublished,
            "release version is unpublished",
            "release version already published",
        )
    else:
        findings.append(
            Finding(
                check="publish-version-unpublished",
                severity="warning",
                summary="release version publication check skipped outside explicit release-candidate mode",
            )
        )

    modes = {
        "full": [
            "python3",
            "scripts/release_artifacts.py",
            "list-preflight",
            "--manifest",
            "release/publish-artifacts.toml",
            "--mode",
            "full",
        ],
        "locked": [
            "python3",
            "scripts/release_artifacts.py",
            "list-preflight",
            "--manifest",
            "release/publish-artifacts.toml",
            "--mode",
            "locked",
        ],
    }
    crates_by_mode: dict[str, list[str]] = {}
    for mode, cmd in modes.items():
        completed = run_capture(cmd, cwd=root)
        if completed.returncode != 0:
            append_completed_findings(
                findings,
                f"publish-surface-{mode}-list",
                completed,
                f"{mode} preflight list generated",
                f"{mode} preflight list generation failed",
            )
            crates_by_mode[mode] = []
            continue
        crates_by_mode[mode] = [line.strip() for line in completed.stdout.splitlines() if line.strip()]

    for crate in crates_by_mode.get("full", []):
        for cmd, check_name, summary in (
            (
                ["cargo", "package", "-p", crate, "--locked", "--no-verify"],
                f"cargo-package-{crate}",
                f"`cargo package` failed for {crate}",
            ),
            (
                ["cargo", "publish", "--dry-run", "-p", crate, "--locked", "--no-verify"],
                f"cargo-publish-dry-run-{crate}",
                f"`cargo publish --dry-run` failed for {crate}",
            ),
        ):
            completed = run_capture(cmd, cwd=root)
            append_completed_findings(findings, check_name, completed, f"{check_name} passed", summary)

    for crate in crates_by_mode.get("locked", []):
        completed = run_capture(["cargo", "check", "-p", crate, "--locked"], cwd=root)
        append_completed_findings(
            findings,
            f"cargo-check-{crate}",
            completed,
            f"cargo check passed for {crate}",
            f"`cargo check --locked` failed for {crate}",
        )


def validate_inventory(root: Path, version: str, findings: list[Finding]) -> None:
    tag = f"v{version}"
    commit_result = run_capture(["git", "rev-parse", "HEAD"], cwd=root)
    if commit_result.returncode != 0:
        append_completed_findings(
            findings,
            "inventory-commit",
            commit_result,
            "release commit resolved",
            "release commit resolution failed",
        )
        return
    commit = commit_result.stdout.strip()
    with tempfile.TemporaryDirectory(prefix="atm-release-inventory-") as tmpdir:
        output = Path(tmpdir) / "release-inventory.json"
        completed = run_capture(
            [
                "python3",
                "scripts/release_artifacts.py",
                "emit-inventory",
                "--manifest",
                "release/publish-artifacts.toml",
                "--version",
                version,
                "--tag",
                tag,
                "--commit",
                commit,
                "--source-ref",
                f"refs/heads/{current_ref(root)}",
                "--generated-at",
                utc_now(),
                "--output",
                str(output),
            ],
            cwd=root,
        )
        if completed.returncode != 0:
            append_completed_findings(
                findings,
                "inventory-generate",
                completed,
                "release inventory generated",
                "release inventory generation failed",
            )
            return
        inventory = json.loads(output.read_text(encoding="utf-8"))

    missing_top = [field for field in INVENTORY_REQUIRED_TOP if field not in inventory]
    if missing_top:
        findings.append(
            Finding(
                check="inventory-shape",
                severity="error",
                summary="inventory missing required top-level fields",
                detail=", ".join(missing_top),
            )
        )
        return
    items = inventory.get("items", [])
    if not isinstance(items, list) or not items:
        findings.append(
            Finding(
                check="inventory-shape",
                severity="error",
                summary="inventory.items must be a non-empty list",
            )
        )
        return
    item_errors: list[str] = []
    for idx, item in enumerate(items):
        if not isinstance(item, dict):
            item_errors.append(f"items[{idx}] must be an object")
            continue
        for field in INVENTORY_REQUIRED_ITEM:
            if field not in item:
                item_errors.append(f"items[{idx}] missing {field}")
    if item_errors:
        findings.append(
            Finding(
                check="inventory-shape",
                severity="error",
                summary="inventory shape validation failed",
                detail="; ".join(item_errors),
            )
        )


def validate_cargo_lock_drift(root: Path, findings: list[Finding], *, enforce_release_window: bool) -> None:
    if not enforce_release_window:
        findings.append(
            Finding(
                check="cargo-lock-drift",
                severity="warning",
                summary="Cargo.lock drift check skipped outside explicit release-candidate mode",
            )
        )
        return
    verify_ref = run_capture(["git", "rev-parse", "--verify", "origin/main"], cwd=root)
    if verify_ref.returncode != 0:
        append_completed_findings(
            findings,
            "cargo-lock-origin-main",
            verify_ref,
            "origin/main available",
            "origin/main is not available for Cargo.lock drift comparison",
        )
        return
    completed = run_capture(["git", "diff", "--name-only", "origin/main", "--", "Cargo.lock"], cwd=root)
    if completed.returncode != 0:
        append_completed_findings(
            findings,
            "cargo-lock-drift",
            completed,
            "Cargo.lock drift check passed",
            "Cargo.lock drift check failed",
        )
        return
    if completed.stdout.strip():
        findings.append(
            Finding(
                check="cargo-lock-drift",
                severity="error",
                summary="Cargo.lock drift detected against origin/main",
                detail=completed.stdout.strip(),
                command=["git", "diff", "--name-only", "origin/main", "--", "Cargo.lock"],
            )
        )


def workspace_package_names(root: Path) -> set[str]:
    cargo = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    members = cargo.get("workspace", {}).get("members", [])
    names: set[str] = set()
    for member in members:
        manifest = root / member / "Cargo.toml"
        if not manifest.exists():
            continue
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        name = data.get("package", {}).get("name")
        if isinstance(name, str) and name:
            names.add(name)
    return names


def direct_registry_dependencies(root: Path) -> dict[str, str]:
    internal = workspace_package_names(root)
    manifests = [root / "Cargo.toml"]
    cargo = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    for member in cargo.get("workspace", {}).get("members", []):
        manifest = root / member / "Cargo.toml"
        if manifest.exists():
            manifests.append(manifest)

    deps: dict[str, str] = {}

    def collect_from_table(table: object) -> None:
        if not isinstance(table, dict):
            return
        for dep_name, spec in table.items():
            if dep_name in internal:
                continue
            package_name = dep_name
            version: str | None = None
            if isinstance(spec, str):
                version = spec
            elif isinstance(spec, dict):
                if spec.get("workspace") is True or "path" in spec:
                    continue
                package_name = str(spec.get("package", dep_name))
                if package_name in internal:
                    continue
                raw = spec.get("version")
                if isinstance(raw, str):
                    version = raw
            if isinstance(version, str) and version.strip():
                deps.setdefault(package_name, version.strip())

    for manifest in manifests:
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        collect_from_table(data.get("dependencies"))
        collect_from_table(data.get("build-dependencies"))
        # Workspace-level dependency declarations are direct registry
        # dependencies too. Member manifests commonly inherit these with
        # `workspace = true`, so omitting this table makes the currency sweep
        # blind to the workspace's primary pins.
        workspace = data.get("workspace")
        if isinstance(workspace, dict):
            collect_from_table(workspace.get("dependencies"))
            collect_from_table(workspace.get("build-dependencies"))
        for target_data in data.get("target", {}).values():
            if isinstance(target_data, dict):
                collect_from_table(target_data.get("dependencies"))
                collect_from_table(target_data.get("build-dependencies"))
    return deps


def latest_registry_version(root: Path, crate: str) -> str | None:
    completed = run_capture(["cargo", "search", crate, "--limit", "1"], cwd=root)
    if completed.returncode != 0:
        return None
    pattern = re.compile(rf"^{re.escape(crate)} = \"([^\"]+)\"")
    for line in completed.stdout.splitlines():
        match = pattern.match(line.strip())
        if match:
            return match.group(1)
    return None


def maybe_file_dep_currency_issue(root: Path, stale: list[tuple[str, str, str]]) -> str | None:
    if os.environ.get(GITHUB_ISSUE_ENV) != "1":
        return None
    if not stale:
        return None
    completed = run_capture(["gh", "--version"], cwd=root)
    if completed.returncode != 0:
        return None
    title = f"Release preflight stale dependency findings on {current_ref(root)}"
    body_lines = [
        "Publisher preflight found stale direct dependencies that should be reviewed:",
        "",
    ]
    for dep, current, latest in stale:
        body_lines.append(f"- `{dep}` current `{current}` latest `{latest}`")
    body_lines.extend(
        [
            "",
            f"Detected by `scripts/validate_release.py` with `{CHECK_DEP_CURRENCY_ENV}=1`.",
        ]
    )
    completed = run_capture(
        ["gh", "issue", "create", "--title", title, "--body", "\n".join(body_lines)],
        cwd=root,
    )
    if completed.returncode == 0:
        return completed.stdout.strip() or None
    return None


def normalized_dependency_version(version: str) -> str:
    """Return the registry-comparable form of a Cargo dependency version."""
    return version.strip().lstrip("=").strip()


def semantic_version(value: str) -> tuple[int, int, int] | None:
    match = SEMVER_PATTERN.search(value)
    if match is None:
        return None
    return tuple(int(part) for part in match.groups())


def latest_wyvern_version(root: Path) -> str | None:
    """Read the newest non-draft Wyvern release through the GitHub CLI."""
    completed = run_capture(
        [
            "gh",
            "release",
            "list",
            "--repo",
            WYVERN_REPOSITORY,
            "--limit",
            "1",
            "--json",
            "tagName",
        ],
        cwd=root,
    )
    if completed.returncode == 0:
        try:
            releases = json.loads(completed.stdout)
        except json.JSONDecodeError:
            releases = None
    else:
        # `gh release list` uses the GraphQL API and can be rate-limited even
        # when the repository's REST endpoint remains available.
        completed = run_capture(
            ["gh", "api", f"repos/{WYVERN_REPOSITORY}/releases/latest"],
            cwd=root,
        )
        if completed.returncode != 0:
            return None
        try:
            releases = [json.loads(completed.stdout)]
        except json.JSONDecodeError:
            releases = None
    if isinstance(releases, list):
        for release in releases:
            tag = release.get("tagName", release.get("tag_name")) if isinstance(release, dict) else None
            if isinstance(tag, str):
                version = semantic_version(tag)
                if version is not None:
                    return ".".join(str(part) for part in version)
    return next(
        (
            ".".join(str(part) for part in version)
            for version in (semantic_version(line) for line in completed.stdout.splitlines())
            if version is not None
        ),
        None,
    )


def ecosystem_evidence_path(root: Path) -> Path:
    configured = os.environ.get(ECOSYSTEM_EVIDENCE_ENV)
    if configured is None:
        return root / AQ6_EVIDENCE_REGISTER
    path = Path(configured)
    return path if path.is_absolute() else root / path


def configured_known_good_pin(dependency: str) -> str | None:
    raw = os.environ.get(ECOSYSTEM_KNOWN_GOOD_ENV)
    if not raw:
        return None
    try:
        values = json.loads(raw)
    except json.JSONDecodeError:
        return None
    value = values.get(dependency) if isinstance(values, dict) else None
    return value if isinstance(value, str) and semantic_version(value) is not None else None


def replace_cargo_exact_pin(path: Path, dependency: str, version: str) -> bool:
    text = path.read_text(encoding="utf-8")
    pattern = re.compile(
        rf'(?m)^(?P<prefix>[ \t]*{re.escape(dependency)}[ \t]*=[ \t]*")'
        rf"=?\d+\.\d+\.\d+(?P<suffix>\"[^\n]*)$"
    )
    updated, count = pattern.subn(rf"\g<prefix>={version}\g<suffix>", text, count=1)
    if count != 1:
        raise ValueError(f"could not find one exact Cargo pin for {dependency} in {path}")
    if updated == text:
        return False
    path.write_text(updated, encoding="utf-8")
    return True


def replace_wyvern_pin(path: Path, version: str) -> bool:
    text = path.read_text(encoding="utf-8")
    pattern = re.compile(
        r'(?m)(?P<prefix>(?:WYVERN_PIN|wyvernPin)[ \t]*[=:][ \t]*["\'])'
        r"\d+\.\d+\.\d+(?P<suffix>[\"'][^\n]*)"
    )
    updated, count = pattern.subn(rf"\g<prefix>{version}\g<suffix>", text, count=1)
    if count != 1:
        raise ValueError(f"could not find one WYVERN_PIN in {path}")
    if updated == text:
        return False
    path.write_text(updated, encoding="utf-8")
    return True


def pin_back_ecosystem_dependency(
    root: Path,
    dependency: str,
    last_known_good: str,
    latest: str,
    issue_url: str | None,
    *,
    evidence_register: Path | None = None,
) -> list[Path]:
    """Pin a regressed latest release back and append the required evidence."""
    good_version = semantic_version(last_known_good)
    latest_version = semantic_version(latest)
    if good_version is None or latest_version is None:
        raise ValueError(f"pin-back versions must be semantic versions: {last_known_good}, {latest}")
    if good_version == latest_version:
        raise ValueError(f"pin-back version for {dependency} must differ from regressed latest {latest}")

    changed: list[Path] = []
    if dependency == "wyvern":
        for relative_path in WYVERN_PIN_FILES:
            path = root / relative_path
            if not path.is_file():
                raise ValueError(f"missing Wyvern pin file: {relative_path}")
            if replace_wyvern_pin(path, last_known_good):
                changed.append(relative_path)
    else:
        pin_files = ECOSYSTEM_CARGO_PIN_FILES.get(dependency)
        if pin_files is None:
            raise ValueError(f"unsupported sc-ecosystem dependency: {dependency}")
        for relative_path, cargo_dependency in pin_files:
            path = root / relative_path
            if not path.is_file():
                raise ValueError(f"missing Cargo pin file: {relative_path}")
            if replace_cargo_exact_pin(path, cargo_dependency, last_known_good):
                if relative_path not in changed:
                    changed.append(relative_path)

    register = evidence_register or ecosystem_evidence_path(root)
    register.parent.mkdir(parents=True, exist_ok=True)
    issue_reference = issue_url or "not filed; set ATMD_GH_AUTOFIX_ISSUES=1 to file it"
    files = ", ".join(f"`{path}`" for path in changed) or "already at the requested pin"
    with register.open("a", encoding="utf-8") as stream:
        stream.write(
            "\n## Fix-forward pin-back\n\n"
            f"- dependency: `{dependency}`\n"
            f"- regressed latest: `{latest}`\n"
            f"- pinned back to last known-good: `{last_known_good}`\n"
            f"- changed files: {files}\n"
            f"- tracking issue: {issue_reference}\n"
        )
    return changed


def handle_ecosystem_regression(
    root: Path,
    findings: list[Finding],
    dependency: str,
    current: str,
    latest: str,
    failure: str,
) -> None:
    issue_url = maybe_file_dep_currency_issue(root, [(dependency, current, latest)])
    if os.environ.get(ECOSYSTEM_FIX_FORWARD_ENV) != "1":
        append_ecosystem_finding(
            findings,
            f"{dependency} latest release regressed its integration contract",
            f"{failure}; set {ECOSYSTEM_FIX_FORWARD_ENV}=1 with a {ECOSYSTEM_KNOWN_GOOD_ENV} map "
            "to pin back in fix-forward mode, or fix the latest release forward.",
        )
        return

    last_known_good = configured_known_good_pin(dependency)
    if last_known_good is None:
        append_ecosystem_finding(
            findings,
            f"{dependency} regression cannot be pinned back",
            f"{failure}; provide {ECOSYSTEM_KNOWN_GOOD_ENV} with the last-known-good version.",
        )
        return
    try:
        changed = pin_back_ecosystem_dependency(
            root,
            dependency,
            last_known_good,
            latest,
            issue_url,
        )
    except (OSError, ValueError) as error:
        append_ecosystem_finding(
            findings,
            f"{dependency} regression pin-back failed",
            f"{failure}; {error}",
        )
        return
    files = ", ".join(str(path) for path in changed) or "no file changes needed"
    issue = issue_url or "no issue URL (set ATMD_GH_AUTOFIX_ISSUES=1)"
    append_ecosystem_finding(
        findings,
        f"{dependency} latest release regressed; pinned back to {last_known_good}",
        f"{failure}; changed {files}; tracking issue: {issue}. Fix forward before release closure.",
    )


def wyvern_pins(root: Path) -> dict[Path, str]:
    pins: dict[Path, str] = {}
    for relative_path in WYVERN_PIN_FILES:
        path = root / relative_path
        if not path.is_file():
            continue
        match = WYVERN_PIN_PATTERN.search(path.read_text(encoding="utf-8"))
        if match is not None:
            pins[relative_path] = match.group(1)
    return pins


def append_ecosystem_finding(
    findings: list[Finding],
    summary: str,
    detail: str = "",
    command: list[str] | None = None,
) -> None:
    findings.append(
        Finding(
            check="sc-ecosystem-preflight",
            severity="error",
            summary=summary,
            detail=detail,
            command=command,
        )
    )


def run_ecosystem_command(
    root: Path,
    findings: list[Finding],
    command: list[str],
    summary: str,
) -> bool:
    completed = run_capture(command, cwd=root)
    if completed.returncode == 0:
        return True
    append_ecosystem_finding(
        findings,
        summary,
        (completed.stderr or completed.stdout).strip(),
        command,
    )
    return False


def validate_ecosystem_currency(
    root: Path,
    findings: list[Finding],
    *,
    dry_run: bool = False,
) -> None:
    """Block a release when an sc-ecosystem pin or its integration proof is stale."""
    stale: list[tuple[str, str, str]] = []
    unresolved: list[str] = []
    latest_versions: dict[str, str] = {}
    direct = direct_registry_dependencies(root)
    for dependency in SC_ECOSYSTEM_CARGO_DEPENDENCIES:
        current = direct.get(dependency)
        if current is None:
            append_ecosystem_finding(
                findings,
                f"missing required sc-ecosystem dependency pin: {dependency}",
                f"Declare {dependency} in the workspace before running release preflight.",
            )
            continue
        latest = latest_registry_version(root, dependency)
        if latest is None:
            unresolved.append(dependency)
            continue
        latest_versions[dependency] = latest
        comparable_current = normalized_dependency_version(current)
        if latest != comparable_current:
            stale.append((dependency, current, latest))

    if unresolved:
        append_ecosystem_finding(
            findings,
            "sc-ecosystem registry releases could not be resolved",
            ", ".join(unresolved),
        )
    if stale:
        detail = "; ".join(
            f"{dependency}: current {current} (pin {normalized_dependency_version(current)}), latest {latest}"
            for dependency, current, latest in stale
        )
        issue_url = maybe_file_dep_currency_issue(root, stale)
        if issue_url:
            detail += f"; tracking issue: {issue_url}"
        append_ecosystem_finding(
            findings,
            "sc-ecosystem dependency pin is not the latest release",
            detail,
        )

    pins = wyvern_pins(root)
    if len(pins) != len(WYVERN_PIN_FILES) or len(set(pins.values())) != 1:
        append_ecosystem_finding(
            findings,
            "Wyvern pin is missing or inconsistent",
            "Both Send-To entry points must carry the same exact WYVERN_PIN.",
        )
        wyvern_pin = next(iter(pins.values()), None)
    else:
        wyvern_pin = next(iter(pins.values()))

    latest_wyvern = latest_wyvern_version(root)
    if latest_wyvern is None:
        append_ecosystem_finding(
            findings,
            "could not resolve the latest Wyvern release",
            f"Run `gh release list --repo {WYVERN_REPOSITORY} --limit 1` with GitHub access.",
        )
    elif wyvern_pin is not None and latest_wyvern != wyvern_pin:
        issue_url = maybe_file_dep_currency_issue(
            root,
            [("wyvern", wyvern_pin, latest_wyvern)],
        )
        detail = f"current pin {wyvern_pin}, latest release {latest_wyvern}"
        if issue_url:
            detail += f"; tracking issue: {issue_url}"
        append_ecosystem_finding(
            findings,
            "Wyvern pin is not the latest release",
            detail,
        )

    wyvern_binary = os.environ.get("ATM_SEND_TO_WYVERN_BIN", "wyvern")
    if shutil.which(wyvern_binary) is None:
        append_ecosystem_finding(
            findings,
            "Wyvern is required on PATH for ecosystem preflight",
            "install wyvern before running preflight; AQ5 runtime lanes remain Wyvern-optional.",
        )
    elif wyvern_pin is not None and not dry_run:
        asset = Path(
            os.environ.get(
                "ATM_SEND_TO_WYVERN_ASSET",
                str(root / "scripts" / "send-to" / "pick-member.html"),
            )
        )
        probe = [
            sys.executable,
            str(root / "scripts" / "send-to" / "probe_wyvern.py"),
            "--pin",
            wyvern_pin,
            "--asset",
            str(asset),
        ]
        if not run_ecosystem_command(root, findings, probe, "Wyvern version/schema preflight probe failed"):
            handle_ecosystem_regression(
                root,
                findings,
                "wyvern",
                wyvern_pin,
                latest_wyvern or wyvern_pin,
                "Wyvern version/schema preflight probe failed",
            )

    if dry_run or stale or unresolved:
        return

    compose_binary = os.environ.get("ATMD_SC_COMPOSE_BIN", "sc-compose")
    if shutil.which(compose_binary) is None:
        append_ecosystem_finding(
            findings,
            "sc-compose is required for ecosystem preflight",
            "install the pinned sc-compose release before running preflight.",
        )
    else:
        compose_tests_passed = run_ecosystem_command(
            root,
            findings,
            ["cargo", "test", "-p", "atm-template-sc-compose"],
            "sc-compose adapter integration tests failed",
        )
        if not compose_tests_passed:
            handle_ecosystem_regression(
                root,
                findings,
                "sc-composer",
                direct["sc-composer"],
                latest_versions["sc-composer"],
                "sc-compose adapter integration tests failed",
            )
        observability_tests_passed = run_ecosystem_command(
            root,
            findings,
            ["cargo", "test", "-p", "agent-team-mail"],
            "sc-observability ATM integration tests failed",
        )
        if not observability_tests_passed:
            handle_ecosystem_regression(
                root,
                findings,
                "sc-observability",
                direct["sc-observability"],
                latest_versions["sc-observability"],
                "sc-observability ATM integration tests failed",
            )
        vars_file = root / "docs" / "plans" / "phase-aq" / "fixtures" / "sc-compose-preflight-vars.json"
        for template in (
            root / ".claude" / "skills" / "codex-orchestration" / "dev-template.xml.j2",
            root / ".claude" / "skills" / "plan-hardening" / "01-plan-scope-review.xml.j2",
        ):
            template_passed = run_ecosystem_command(
                root,
                findings,
                [
                    compose_binary,
                    "render",
                    "--file",
                    str(template),
                    "--var-file",
                    str(vars_file),
                    "--check-render",
                ],
                f"sc-compose canonical template smoke failed: {template.relative_to(root)}",
            )
            if not template_passed:
                handle_ecosystem_regression(
                    root,
                    findings,
                    "sc-composer",
                    direct["sc-composer"],
                    latest_versions["sc-composer"],
                    f"sc-compose canonical template smoke failed: {template.relative_to(root)}",
                )

    # The fixture validator is intentionally independent of Wyvern so the AQ5
    # fallback lanes remain runnable on hosts without the optional binary.
    fixture_test = root / ".just" / "tests" / "test_send_to_surface.py"
    run_ecosystem_command(
        root,
        findings,
        [sys.executable, str(fixture_test)],
        "AQ5 picker fixture regression suite failed",
    )


def validate_generic_dependency_currency(root: Path, findings: list[Finding]) -> None:
    if os.environ.get(CHECK_DEP_CURRENCY_ENV) != "1":
        findings.append(
            Finding(
                check="dependency-currency",
                severity="warning",
                summary=f"dependency currency check skipped; set {CHECK_DEP_CURRENCY_ENV}=1 to enable it",
            )
        )
        return

    stale: list[tuple[str, str, str]] = []
    unresolved: list[str] = []
    for dep, current in sorted(direct_registry_dependencies(root).items()):
        latest = latest_registry_version(root, dep)
        if latest is None:
            unresolved.append(dep)
            continue
        if latest != normalized_dependency_version(current):
            stale.append((dep, current, latest))

    if unresolved:
        findings.append(
            Finding(
                check="dependency-currency",
                severity="warning",
                summary="dependency currency check could not resolve some crates via cargo search",
                detail=", ".join(unresolved),
            )
        )
    if stale:
        detail = "; ".join(f"{dep}: current {current}, latest {latest}" for dep, current, latest in stale)
        findings.append(
            Finding(
                check="dependency-currency",
                severity="warning",
                summary="stale direct dependency versions detected",
                detail=detail,
            )
        )
        maybe_file_dep_currency_issue(root, stale)


def validate_dependency_currency(
    root: Path,
    findings: list[Finding],
    *,
    ecosystem_dry_run: bool = False,
) -> None:
    """Run the legacy advisory sweep and the blocking sc-ecosystem gate."""
    validate_generic_dependency_currency(root, findings)
    validate_ecosystem_currency(root, findings, dry_run=ecosystem_dry_run)


def validate_phase_ad_readiness(root: Path, findings: list[Finding]) -> None:
    readiness_path = root / "docs" / "plans" / "phase-AD" / "readiness.md"
    smoke_normal = root / "reports" / "smoke" / "smoke.md"
    smoke_thorough = root / "reports" / "smoke" / "smoke-thorough.md"
    message_received_boundary_toml = (
        root / "boundaries" / "atm-core" / "message-received-hook-emitter.toml"
    )
    graft_receiver_boundary_toml = (
        root / "boundaries" / "atm-graft" / "message-received-hook.toml"
    )
    boundary_inventory = root / "docs" / "atm-core" / "boundaries.md"
    graft_boundary_inventory = root / "docs" / "atm-graft" / "boundaries.md"

    missing = [
        path
        for path in (
            readiness_path,
            smoke_normal,
            smoke_thorough,
            message_received_boundary_toml,
            graft_receiver_boundary_toml,
            boundary_inventory,
            graft_boundary_inventory,
        )
        if not path.exists()
    ]
    if missing:
        findings.append(
            Finding(
                check="phase-ad-readiness",
                severity="error",
                summary="phase AD readiness artifacts are missing",
                detail=", ".join(str(path.relative_to(root)) for path in missing),
            )
        )
        return

    readiness_text = readiness_path.read_text(encoding="utf-8")
    boundary_text = boundary_inventory.read_text(encoding="utf-8")
    graft_boundary_text = graft_boundary_inventory.read_text(encoding="utf-8")
    required_readiness_markers = (
        "# Phase AD Readiness",
        "`AD.25`",
        "`AD.29`",
        "`AD.30`",
        "`reports/smoke/smoke.md`",
        "`reports/smoke/smoke-thorough.md`",
    )
    missing_markers = [
        marker for marker in required_readiness_markers if marker not in readiness_text
    ]
    if missing_markers:
        findings.append(
            Finding(
                check="phase-ad-readiness",
                severity="error",
                summary="phase AD readiness document is incomplete",
                detail=", ".join(missing_markers),
            )
        )
    if "release verdict:" not in readiness_text:
        findings.append(
            Finding(
                check="phase-ad-readiness",
                severity="error",
                summary="phase AD readiness document is missing the release verdict marker",
                detail="docs/plans/phase-AD/readiness.md must state the current release verdict, even when AD.30 still owns the final close/not-close decision",
            )
        )

    if "## MessageReceivedHookEmitter" not in boundary_text:
        findings.append(
            Finding(
                check="phase-ad-readiness",
                severity="error",
                summary="MessageReceivedHookEmitter boundary inventory entry is missing",
                detail="docs/atm-core/boundaries.md does not contain the MessageReceivedHookEmitter heading",
            )
        )
    if "## Message Received Hook" not in graft_boundary_text:
        findings.append(
            Finding(
                check="phase-ad-readiness",
                severity="error",
                summary="GraftReceiveHook boundary inventory entry is missing",
                detail="docs/atm-graft/boundaries.md does not contain the Message Received Hook heading",
            )
        )

    expected_sprint_statuses = {
        "AD.1": "complete",
        "AD.2": "complete",
        "AD.3": "complete",
        "AD.4": "complete",
        "AD.5": "complete",
        "AD.6": "complete",
        "AD.7": "complete",
        "AD.8": "complete",
        "AD.9": "complete",
        "AD.10": "complete",
        "AD.11": "complete",
    }
    for sprint_id, expected_status in expected_sprint_statuses.items():
        sprint_path = root / "docs" / "plans" / "phase-AD" / f"sprint-{sprint_id.replace('.', '')}.md"
        actual_status = phase_ad_frontmatter_value(sprint_path, "status")
        if actual_status != expected_status:
            findings.append(
                Finding(
                    check="phase-ad-readiness",
                    severity="error",
                    summary=f"{sprint_id} status does not match the readiness contract",
                    detail=f"{sprint_path.relative_to(root)} has status={actual_status!r}; expected {expected_status!r}",
                )
            )

    message_received_boundary_state = tomllib.loads(
        message_received_boundary_toml.read_text(encoding="utf-8")
    ).get(
        "status", {}
    ).get("state")
    if message_received_boundary_state != "active":
        findings.append(
            Finding(
                check="phase-ad-readiness",
                severity="error",
                summary="MessageReceivedHookEmitter boundary state does not match the readiness contract",
                detail=(
                    "boundaries/atm-core/message-received-hook-emitter.toml has "
                    f"state={message_received_boundary_state!r}; expected 'active'"
                ),
            )
        )

    graft_receiver_boundary_state = tomllib.loads(
        graft_receiver_boundary_toml.read_text(encoding="utf-8")
    ).get("status", {}).get("state")
    if graft_receiver_boundary_state != "active":
        findings.append(
            Finding(
                check="phase-ad-readiness",
                severity="error",
                summary="GraftReceiveHook boundary state does not match the readiness contract",
                detail=(
                    "boundaries/atm-graft/message-received-hook.toml has "
                    f"state={graft_receiver_boundary_state!r}; expected 'active'"
                ),
            )
        )


def phase_ad_frontmatter_value(path: Path, key: str) -> str | None:
    text = path.read_text(encoding="utf-8")
    frontmatter_match = re.match(r"---\n(.*?)\n---", text, re.DOTALL)
    if frontmatter_match is None:
        return None
    key_match = re.search(rf"^{re.escape(key)}:\s*(.+)$", frontmatter_match.group(1), re.MULTILINE)
    if key_match is None:
        return None
    return key_match.group(1).strip()


def relpath_display(path: Path, root: Path) -> str:
    return Path(os.path.relpath(path.resolve(), root.resolve())).as_posix()


def ensure_staged_install_docs(
    root: Path,
    *,
    manifest_path: Path,
    staged_install_root: Path | None,
) -> Path:
    resolved_root = staged_install_root or (root / PHASE_AE_STAGED_INSTALL_ROOT)
    manifest = load_manifest(manifest_path)
    source_root = installed_docs_source_root(manifest_path)
    install_root = resolved_root / manifest["installed_docs"]["install_root"]
    if install_root.exists():
        shutil.rmtree(install_root)
    install_root.mkdir(parents=True, exist_ok=True)
    for source_path in installed_doc_source_files(manifest_path):
        destination = install_root / source_path.relative_to(source_root)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_path, destination)
    return resolved_root


def release_notes_installed_docs_check(root: Path) -> tuple[Path, bool]:
    release_notes_path = root / "release" / "release-notes.md"
    if not release_notes_path.is_file():
        return release_notes_path, False
    text = release_notes_path.read_text(encoding="utf-8")
    return release_notes_path, "share/doc/atm/" in text and "share/doc/atm/README.md" in text


def ensure_phase_ae_proof_prereqs(root: Path, findings: list[Finding]) -> None:
    release_notes_path, release_notes_ok = release_notes_installed_docs_check(root)
    if not release_notes_path.is_file():
        findings.append(
            Finding(
                check="phase-ae-proof-release-notes",
                severity="error",
                summary="release notes are missing for the installed-doc proof",
                detail=str(release_notes_path.relative_to(root)),
            )
        )
    elif not release_notes_ok:
        findings.append(
            Finding(
                check="phase-ae-proof-release-notes",
                severity="error",
                summary="release notes do not describe the installed doc location",
                detail="release/release-notes.md must mention both share/doc/atm/ and share/doc/atm/README.md",
            )
        )


def is_phase_ae_doc_finding(check: str) -> bool:
    return check.startswith("installed-docs-") or check.startswith("phase-ae-proof-")


def write_phase_ae_installed_docs_proof(
    root: Path,
    *,
    version: str,
    proof_output: Path,
    staged_install_root: Path | None,
    findings: list[Finding],
) -> None:
    root = root.resolve()
    manifest_path = root / "release" / "publish-artifacts.toml"
    ensure_phase_ae_proof_prereqs(root, findings)
    resolved_staged_install_root = ensure_staged_install_docs(
        root,
        manifest_path=manifest_path,
        staged_install_root=staged_install_root,
    )
    manifest = load_manifest(manifest_path)
    source_root = installed_docs_source_root(manifest_path)
    install_root = resolved_staged_install_root / manifest["installed_docs"]["install_root"]
    source_members = installed_doc_source_files(manifest_path)
    installed_members = installed_doc_members(manifest_path)
    mismatched_members: list[str] = []
    for member in installed_members:
        source_path = source_root / member.relative_to(manifest["installed_docs"]["install_root"])
        installed_path = resolved_staged_install_root / member
        if source_path.read_bytes() != installed_path.read_bytes():
            mismatched_members.append(member.as_posix())
    if mismatched_members:
        findings.append(
            Finding(
                check="phase-ae-proof-membership",
                severity="error",
                summary="installed docs do not byte-match the repo-owned source tree",
                detail=", ".join(mismatched_members),
            )
        )

    release_notes_path, release_notes_ok = release_notes_installed_docs_check(root)
    doc_blockers = [finding for finding in findings if finding.blocks and is_phase_ae_doc_finding(finding.check)]
    proof_status = "passed" if not doc_blockers else "failed"
    lines = [
        "# Phase AE Installed Docs Proof",
        "",
        f"- status: `{proof_status}`",
        f"- generated at: `{utc_now()}`",
        f"- reviewed release version: {version}",
        f"- source doc root: `{relpath_display(source_root, root)}`",
        f"- staged install doc root: `{relpath_display(install_root, root)}`",
        f"- installed entrypoint: `{manifest['installed_docs']['entrypoint'].as_posix()}`",
        f"- release notes check: `{'passed' if release_notes_ok else 'failed'}` (`{relpath_display(release_notes_path, root)}`)",
        f"- installed-doc verifier: `{proof_status}`",
        "",
        "## Verified Installed Corpus",
        "",
    ]
    for member in installed_members:
        lines.append(f"- `{member.as_posix()}`")
    lines.extend(
        [
            "",
            "## Source Corpus Members",
            "",
        ]
    )
    for source_path in source_members:
        lines.append(f"- `{relpath_display(source_path, root)}`")
    lines.extend(
        [
            "",
            "## Validation Inputs",
            "",
            "- `python3 scripts/validate_release.py validate --proof-output reports/smoke/phase-AE-installed-docs-proof.md`",
            "- `scripts/verify_user_docs.py` on the repo-owned source corpus and the staged installed copy",
            "- `release/release-notes.md` installed-doc location references",
        ]
    )
    proof_output.parent.mkdir(parents=True, exist_ok=True)
    proof_output.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_findings(root: Path, version: str, findings_path: Path, findings: list[Finding]) -> None:
    payload = {
        "generatedAt": utc_now(),
        "branch": current_ref(root),
        "version": version,
        "status": "fail" if any(f.blocks for f in findings) else "pass",
        "findings": [asdict(f) | {"blocks": f.blocks} for f in findings],
    }
    findings_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Run the retained release validation suite")
    parser.add_argument(
        "target",
        nargs="?",
        default="all",
        choices=(
            "all",
            "validate",
            "lint",
            "cli-surface",
            "support-files",
            "manifest",
            "publish-surface",
            "release-binaries",
            "inventory",
            "cargo-lock-drift",
            "dependency-currency",
            "ecosystem-preflight",
            "phase-ad-readiness",
        ),
    )
    parser.add_argument("--version", help="Release version to validate; defaults to workspace.package.version")
    parser.add_argument("--findings", default="release-findings.json", help="Path to findings JSON output")
    parser.add_argument(
        "--staged-install-root",
        help="Optional deterministic staged install root to validate installed docs against",
    )
    parser.add_argument(
        "--proof-output",
        help="Optional markdown path for the Phase AE installed-doc proof artifact",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Resolve ecosystem releases and pins without running integration commands",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    root = repo_root()
    explicit_version = args.version is not None
    version = args.version or workspace_version(root)
    staged_install_root = Path(args.staged_install_root).resolve() if args.staged_install_root else None
    proof_output = Path(args.proof_output).resolve() if args.proof_output else None
    if version != workspace_version(root):
        raise SystemExit(
            f"release version mismatch: expected workspace version {workspace_version(root)}, got {version}"
        )

    findings: list[Finding] = []
    actions = {
        "support-files": lambda: validate_support_files(root, findings),
        "lint": lambda: validate_lint(root, findings),
        "cli-surface": lambda: validate_cli_surface(root, findings),
        "manifest": lambda: validate_manifest(
            root,
            findings,
            staged_install_root=staged_install_root,
            release_version=version,
        ),
        "publish-surface": lambda: validate_publish_surface(
            root,
            version,
            findings,
            enforce_release_version=explicit_version,
        ),
        "release-binaries": lambda: validate_release_binaries(root, findings),
        "inventory": lambda: validate_inventory(root, version, findings),
        "cargo-lock-drift": lambda: validate_cargo_lock_drift(
            root,
            findings,
            enforce_release_window=explicit_version,
        ),
        "dependency-currency": lambda: validate_dependency_currency(
            root,
            findings,
            ecosystem_dry_run=args.dry_run,
        ),
        "ecosystem-preflight": lambda: validate_ecosystem_currency(
            root,
            findings,
            dry_run=args.dry_run,
        ),
        "phase-ad-readiness": lambda: validate_phase_ad_readiness(root, findings),
    }

    findings_path = root / args.findings
    try:
        effective_target = "all" if args.target == "validate" else args.target
        if effective_target == "all":
            for target in (
                "support-files",
                "lint",
                "cli-surface",
                "manifest",
                "publish-surface",
                "release-binaries",
                "inventory",
                "cargo-lock-drift",
                "dependency-currency",
                "phase-ad-readiness",
            ):
                print(f"== validate {target} ==")
                actions[target]()
        else:
            actions[effective_target]()
    finally:
        if proof_output is not None:
            write_phase_ae_installed_docs_proof(
                root,
                version=version,
                proof_output=proof_output,
                staged_install_root=staged_install_root,
                findings=findings,
            )
        write_findings(root, version, findings_path, findings)
        print(f"wrote findings: {findings_path}")

    blockers = [finding for finding in findings if finding.blocks]
    if blockers:
        print("release validation blockers:")
        for finding in blockers:
            print(f"- [{finding.check}] {finding.summary}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
