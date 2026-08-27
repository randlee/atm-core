"""Manifest and channel-contract parsing for the vendorable publish kit."""
from __future__ import annotations

import re
import tomllib
from pathlib import Path, PurePosixPath


CHANNEL_CONTRACTS_FILE = "publish-channel-contracts.toml"
ROOT_CHANNELS = frozenset({"crates_io", "github_release"})
SUPPORTED_SCHEMA_VERSION = 1
# Single source for the release Rust toolchain when the manifest does not
# declare [project].rust_toolchain; workflows read it via `build-plan`.
DEFAULT_RUST_TOOLCHAIN = "1.94.1"


def _require_keys(entry: dict, required: tuple[str, ...], label: str) -> None:
    missing = [key for key in required if key not in entry]
    if missing:
        joined = ", ".join(missing)
        raise SystemExit(f"{label} missing required keys: {joined}")


def load_channel_contracts(path: Path) -> dict[str, dict]:
    """Load the vendorable, non-secret protocol for every supported channel."""
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    channels = data.get("channels")
    if not isinstance(channels, dict):
        raise SystemExit(f"{path}: [channels] must be a table")
    for name, contract in channels.items():
        if not isinstance(contract, dict):
            raise SystemExit(f"{path}: [channels.{name}] must be a table")
        _require_keys(contract, ("stage", "agent"), f"{path}: [channels.{name}]")
        if contract["stage"] not in {"root", "post_release"}:
            raise SystemExit(f"{path}: [channels.{name}].stage must be root or post_release")
        if not isinstance(contract["agent"], str) or not contract["agent"]:
            raise SystemExit(f"{path}: [channels.{name}].agent must be a non-empty string")
    missing_roots = ROOT_CHANNELS - set(channels)
    if missing_roots:
        raise SystemExit(f"{path}: missing required root channel(s): {', '.join(sorted(missing_roots))}")
    return channels


def load_manifest(path: Path, *, with_channel_contracts: bool = False) -> dict:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    schema_version = data.get("schema_version", SUPPORTED_SCHEMA_VERSION)
    if schema_version != SUPPORTED_SCHEMA_VERSION:
        raise SystemExit(f"unsupported manifest schema_version: {schema_version!r}")
    # An empty crates list is valid: pure-Python consumers publish no Rust
    # crates, and every Cargo step gates on the manifest's crates.
    crates = data.get("crates", [])
    release_binaries = data.get("release_binaries", [])
    python_packages = data.get("python_packages", [])
    python_distributions = data.get("python_distributions", [])
    crates = sorted(crates, key=lambda item: (item["publish_order"], item["artifact"]))
    manifest = {
        "project": data.get("project", {}),
        "crates": crates,
        "release_binaries": release_binaries,
        "release_targets": data.get("release_targets", []),
        "python_packages": python_packages,
        "python_distributions": python_distributions,
        "channels": data.get("channels", {}),
    }
    if with_channel_contracts:
        manifest["channel_contracts"] = load_channel_contracts(
            path.parent / CHANNEL_CONTRACTS_FILE
        )
    return manifest


def manifest_workspace_toml(manifest: dict) -> str:
    """Return the manifest-declared Cargo workspace manifest path."""
    value = manifest["project"].get("workspace_toml", "Cargo.toml")
    if not isinstance(value, str) or not value:
        raise SystemExit("[project].workspace_toml must be a non-empty string")
    return value


def manifest_python_upload_tool(manifest: dict) -> str:
    """Return the PyPI uploader implied by the declared build systems.

    maturin's uploader handles wheels and sdists from any build system, so a
    manifest with at least one maturin distribution keeps maturin. A purely
    setuptools consumer has no Rust toolchain and uploads with twine. Empty
    when the manifest declares no Python distributions.
    """
    entries = _python_distribution_entries(manifest)
    if not entries:
        return ""
    if any(entry["build_system"] == "maturin" for entry in entries):
        return "maturin"
    return "twine"


def manifest_rust_toolchain(manifest: dict) -> str:
    """Return the manifest-declared release Rust toolchain."""
    value = manifest["project"].get("rust_toolchain", DEFAULT_RUST_TOOLCHAIN)
    if not isinstance(value, str) or not value:
        raise SystemExit("[project].rust_toolchain must be a non-empty string")
    return value


def workspace_members(workspace_toml: Path) -> set[str]:
    data = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))
    return set(data.get("workspace", {}).get("members", []))


def package_name(cargo_toml: Path) -> str:
    data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
    return data["package"]["name"]


def workspace_dependency_names(crate_toml: Path, workspace_toml: Path) -> set[str]:
    """Return dependencies that resolve to another package in this workspace."""
    workspace_root = workspace_toml.parent.resolve()
    workspace_data = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))
    workspace_deps = workspace_data.get("workspace", {}).get("dependencies", {})
    workspace_packages = {
        package_name(workspace_root / member / "Cargo.toml")
        for member in workspace_members(workspace_toml)
        if (workspace_root / member / "Cargo.toml").is_file()
    }
    crate_data = tomllib.loads(crate_toml.read_text(encoding="utf-8"))
    crate_dir = crate_toml.parent
    dependencies: set[str] = set()

    def resolve(name: str, spec: object) -> str | None:
        if isinstance(spec, str):
            return name if name in workspace_packages else None
        if not isinstance(spec, dict):
            return None
        if spec.get("workspace") is True:
            workspace_spec = workspace_deps.get(name, {})
            if isinstance(workspace_spec, dict):
                package = workspace_spec.get("package", name)
                if "path" in workspace_spec or package in workspace_packages:
                    return package
            return name if name in workspace_packages else None
        package = spec.get("package", name)
        if "path" in spec and (crate_dir / spec["path"]).resolve().is_relative_to(workspace_root):
            return package
        return package if package in workspace_packages else None

    def collect(table: object) -> None:
        if not isinstance(table, dict):
            return
        for name, spec in table.items():
            package = resolve(name, spec)
            if package:
                dependencies.add(package)

    collect(crate_data.get("dependencies", {}))
    collect(crate_data.get("build-dependencies", {}))
    for target in crate_data.get("target", {}).values():
        if isinstance(target, dict):
            collect(target.get("dependencies", {}))
            collect(target.get("build-dependencies", {}))
    return dependencies


def validate_publish_order(args: object) -> int:
    """Check that package publication order follows workspace dependencies."""
    manifest = load_manifest(Path(args.manifest))
    workspace_toml = Path(args.workspace_toml)
    publishable = [crate for crate in manifest["crates"] if crate["publish"]]
    order = {crate["package"]: crate["publish_order"] for crate in publishable}
    violations = []
    for crate in publishable:
        crate_toml = workspace_toml.parent / crate["cargo_toml"]
        for dependency in sorted(workspace_dependency_names(crate_toml, workspace_toml)):
            if dependency in order and order[crate["package"]] <= order[dependency]:
                violations.append(
                    f"{crate['package']} (publish_order={order[crate['package']]}) depends on "
                    f"{dependency} (publish_order={order[dependency]})"
                )
    if violations:
        raise SystemExit("publish_order violation(s):\n  - " + "\n  - ".join(violations))
    print("ok: publish_order matches the workspace dependency graph")
    return 0


def workspace_version(workspace_toml: Path) -> str:
    """Resolve the release version from the manifest-declared version source.

    [project].workspace_toml names the single version source for a consumer:
    a Cargo workspace manifest ([workspace.package].version) for Rust
    consumers, or a PEP 621 pyproject.toml ([project].version) for
    pure-Python consumers with no Cargo workspace.
    """
    data = tomllib.loads(workspace_toml.read_text(encoding="utf-8"))
    cargo_version = data.get("workspace", {}).get("package", {}).get("version")
    if isinstance(cargo_version, str) and cargo_version:
        return cargo_version
    project_version = data.get("project", {}).get("version")
    if isinstance(project_version, str) and project_version:
        return project_version
    raise SystemExit(
        f"{workspace_toml}: version source must declare [workspace.package].version "
        "(Cargo workspace) or [project].version (pyproject)"
    )


def _resolve_workspace_path(workspace_toml: Path, relative_path: str) -> Path:
    return workspace_toml.parent / relative_path


def _assert_workspace_inherited_version(workspace_toml: Path, relative_path: str, *, allow_literal_base: bool = False) -> None:
    path = _resolve_workspace_path(workspace_toml, relative_path)
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    value = data.get("package", {}).get("version")
    if isinstance(value, dict) and value.get("workspace") is True:
        return
    if allow_literal_base and value == workspace_version(workspace_toml).split("-", 1)[0]:
        return
    if not isinstance(value, dict) or value.get("workspace") is not True:
        raise SystemExit(
            f"{relative_path}: [package].version must inherit workspace.package.version or match workspace base"
        )


def _assert_python_package_version(
    workspace_toml: Path,
    relative_path: str,
    expected_version: str,
    *,
    cargo_manifest: str | None = None,
) -> None:
    path = _resolve_workspace_path(workspace_toml, relative_path)
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    actual_version = data.get("project", {}).get("version")
    dynamic_version = actual_version is None and "version" in data.get("project", {}).get("dynamic", [])
    if actual_version is None and "version" in data.get("project", {}).get("dynamic", []):
        if cargo_manifest is None:
            raise SystemExit(f"{relative_path}: dynamic version requires a Cargo manifest")
        cargo_data = tomllib.loads(_resolve_workspace_path(workspace_toml, cargo_manifest).read_text(encoding="utf-8"))
        actual_version = cargo_data.get("package", {}).get("version")
        if isinstance(actual_version, dict) and actual_version.get("workspace") is True:
            actual_version = workspace_version(workspace_toml)
    expected = expected_version.split("-", 1)[0] if dynamic_version else expected_version
    if actual_version != expected:
        raise SystemExit(
            f"{relative_path}: [project].version mismatch: "
            f"expected {expected}, got {actual_version!r}"
        )


def _python_project_version(pyproject_toml: Path) -> str:
    data = tomllib.loads(pyproject_toml.read_text(encoding="utf-8"))
    project = data.get("project", {})
    version = project.get("version")
    if version is None and "version" in project.get("dynamic", []):
        return ""
    if not isinstance(version, str):
        raise SystemExit(f"{pyproject_toml}: [project].version must be a string")
    return version


def _python_project_name(pyproject_toml: Path) -> str:
    data = tomllib.loads(pyproject_toml.read_text(encoding="utf-8"))
    project = data.get("project", {})
    name = project.get("name")
    if not isinstance(name, str):
        raise SystemExit(f"{pyproject_toml}: [project].name must be a string")
    return name


SUPPORTED_PYTHON_BUILD_SYSTEMS = frozenset({"maturin", "setuptools"})


def _python_distribution_build_system(distribution: dict) -> str:
    """Resolve one distribution's build system; unsupported shapes fail closed."""
    name = distribution.get("name", "?")
    cargo_manifest = distribution.get("cargo_manifest")
    build_system = distribution.get("build_system")
    if cargo_manifest and build_system:
        raise SystemExit(
            f"[[python_distributions]] {name}: must not set both cargo_manifest and build_system"
        )
    if cargo_manifest:
        return "maturin"
    if build_system not in SUPPORTED_PYTHON_BUILD_SYSTEMS:
        raise SystemExit(
            f"[[python_distributions]] {name}: unsupported build_system {build_system!r}; "
            "declare cargo_manifest (maturin) or build_system = \"setuptools\""
        )
    return build_system


def _python_distribution_entries(manifest: dict) -> list[dict]:
    """Return normalized Python distribution entries from the release manifest."""
    packages = {entry["package"]: entry for entry in manifest["python_packages"]}
    entries: list[dict] = []
    for distribution in manifest["python_distributions"]:
        package = packages[distribution["name"]]
        source = distribution["source"]
        entries.append(
            {
                "artifact": package["artifact"],
                "name": distribution["name"],
                "source": source,
                "pyproject": package["manifest"],
                "cargo_manifest": distribution.get("cargo_manifest"),
                "build_system": _python_distribution_build_system(distribution),
                "module_path": distribution.get(
                    "module_path", f"{source}/python/{package['module']}"
                ),
                "sdist": distribution["sdist"],
                "wheels": distribution["wheels"],
            }
        )
    return entries


def _python_distribution_expectations(manifest: dict) -> dict[str, dict[str, int]]:
    return {
        entry["name"]: {
            "wheel": len(entry["wheels"]),
            "sdist": int(entry["sdist"]),
        }
        for entry in _python_distribution_entries(manifest)
    }


def _require_project(manifest: dict) -> dict:
    project = manifest["project"]
    _require_keys(
        project,
        ("name", "archive_prefix", "description", "homepage", "license"),
        "[project]",
    )
    return project


def _renderer_archive_path(manifest: dict) -> str:
    value = _require_project(manifest).get("renderer_archive_path")
    if not isinstance(value, str) or not value:
        raise SystemExit("[project].renderer_archive_path must be a non-empty string")
    return value


def _release_targets_by_name(manifest: dict) -> dict[str, dict]:
    targets: dict[str, dict] = {}
    for index, target in enumerate(manifest["release_targets"], start=1):
        _require_keys(target, ("target", "os", "archive"), f"[[release_targets]] #{index}")
        name = target["target"]
        if name in targets:
            raise SystemExit(f"duplicate release target: {name}")
        targets[name] = target
    if not targets:
        raise SystemExit("manifest must define [[release_targets]]")
    return targets


def _channel_config(manifest: dict, channel_name: str) -> dict:
    try:
        channel = manifest["channels"][channel_name]
    except KeyError as error:
        raise SystemExit(f"manifest must define [channels.{channel_name}]") from error
    if not isinstance(channel, dict):
        raise SystemExit(f"[channels.{channel_name}] must be a table")
    return channel


def _is_prerelease_tag(tag: str) -> bool:
    """Return whether a SemVer-style tag names a prerelease."""
    return "-" in tag.removeprefix("v").split("+", maxsplit=1)[0]


def _validate_homebrew_formulas(
    channel: dict, available_binaries: set[str] | None = None
) -> list[dict]:
    """Validate manifest-declared Homebrew formula entries."""
    formulas = channel.get("formulas")
    if not isinstance(formulas, list) or not formulas:
        raise SystemExit("[channels.homebrew] must define [[channels.homebrew.formulas]]")

    paths: set[str] = set()
    for index, formula in enumerate(formulas, start=1):
        label = f"[[channels.homebrew.formulas]] #{index}"
        if not isinstance(formula, dict):
            raise SystemExit(f"{label} must be a table")
        _require_keys(
            formula,
            ("path", "template", "class", "test_command", "test_output", "release_track"),
            label,
        )
        for key in ("path", "template", "class", "test_command", "test_output"):
            if not isinstance(formula[key], str) or not formula[key]:
                raise SystemExit(f"{label}.{key} must be a non-empty string")
        binaries = formula.get("binaries")
        if binaries is None:
            legacy_binary = formula.get("binary")
            if not isinstance(legacy_binary, str) or not legacy_binary:
                raise SystemExit(f"{label} must define non-empty binaries or legacy binary")
            binaries = [legacy_binary]
            formula["binaries"] = binaries
        if not isinstance(binaries, list) or not binaries or not all(
            isinstance(binary, str) and binary for binary in binaries
        ):
            raise SystemExit(f"{label}.binaries must be a non-empty list of strings")
        if len(set(binaries)) != len(binaries):
            raise SystemExit(f"{label}.binaries must not contain duplicates")
        if available_binaries is not None:
            missing_binaries = sorted(set(binaries) - available_binaries)
            if missing_binaries:
                raise SystemExit(
                    f"{label}.binaries references undeclared release binary(s): "
                    + ", ".join(missing_binaries)
                )
        test_binary = formula.setdefault("test_binary", binaries[0])
        if not isinstance(test_binary, str) or test_binary not in binaries:
            raise SystemExit(f"{label}.test_binary must name one of its binaries")
        for key in ("path", "template"):
            path = PurePosixPath(formula[key])
            if path.is_absolute() or ".." in path.parts or str(path) in ("", "."):
                raise SystemExit(f"{label}.{key} must be a safe relative path")
        if formula["release_track"] not in {"stable", "prerelease"}:
            raise SystemExit(f"{label}.release_track must be stable or prerelease")
        if formula["path"] in paths:
            raise SystemExit(f"duplicate Homebrew formula path: {formula['path']}")
        paths.add(formula["path"])

    return formulas


def _homebrew_formulas_for_tag(
    channel: dict, tag: str, available_binaries: set[str] | None = None
) -> list[dict]:
    """Select manifest-declared Homebrew formulas for one release tag."""
    formulas = _validate_homebrew_formulas(channel, available_binaries)
    selected_track = "prerelease" if _is_prerelease_tag(tag) else "stable"
    selected = [
        formula for formula in formulas if formula["release_track"] == selected_track
    ]

    if not selected:
        raise SystemExit(f"no Homebrew {selected_track} formulas declared for tag {tag}")
    return selected


def _normalize_pypi_name(name: str) -> str:
    """Return the PEP 503 canonical project name used for public lookups."""
    return re.sub(r"[-_.]+", "-", name).lower()


def _url_from_contract(template: str, name: str, version: str) -> str:
    return template.format(name=name, version=version)


def _public_registry_checks(
    contracts: dict[str, dict], channel_name: str, name: str, version: str | None
) -> list[dict[str, str | None]]:
    """Build contract-derived public registry checks for one candidate artifact."""
    try:
        contract = contracts[channel_name]
    except KeyError as error:
        raise SystemExit(f"channel contract missing for {channel_name}") from error
    if not contract.get("public_registry_checks", False):
        raise SystemExit(f"{channel_name} does not support a public registry inquiry")

    normalized_name = _normalize_pypi_name(name) if channel_name == "pypi" else name
    registry_contracts: list[dict[str, str]]
    if channel_name == "crates_io":
        registry_contracts = [
            {
                "name": "crates.io",
                "project_lookup_url": contract["project_lookup_url"],
                "version_lookup_url": contract["version_lookup_url"],
                "version_policy": "must_be_absent",
            }
        ]
    else:
        registry_contracts = contract.get("registries", [])

    checks: list[dict[str, str]] = []
    for registry in registry_contracts:
        check: dict[str, str | None] = {
            "channel": channel_name,
            "agent": contract["agent"],
            "registry": registry["name"],
            "name": name,
            "normalized_name": normalized_name,
            "expected_version": version,
            "project_lookup_url": _url_from_contract(
                registry["project_lookup_url"], normalized_name, version or ""
            ),
            "version_lookup_url": (
                _url_from_contract(registry["version_lookup_url"], normalized_name, version)
                if version
                else None
            ),
            "version_policy": registry["version_policy"],
        }
        checks.append(check)
    return checks


def registry_version_state(url: str, timeout: int = 20) -> str:
    """Resolve an exact version_lookup_url to published/absent; fail closed otherwise."""
    import urllib.error
    import urllib.request

    request = urllib.request.Request(url, headers={"User-Agent": "sc-publish-kit"})
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            status = response.status
    except urllib.error.HTTPError as error:
        status = error.code
    except (urllib.error.URLError, TimeoutError) as error:
        raise SystemExit(f"registry lookup failed for {url}: {error}") from error
    if status == 200:
        return "published"
    if status == 404:
        return "absent"
    raise SystemExit(f"registry state for {url} is indeterminate (status {status})")


def _channel_contract(manifest: dict, channel_name: str) -> dict:
    try:
        contract = manifest["channel_contracts"][channel_name]
    except KeyError as error:
        raise SystemExit(f"channel contract missing for {channel_name}") from error
    return contract


def _channel_names(manifest: dict) -> tuple[str, ...]:
    channels = manifest["channels"]
    if not isinstance(channels, dict):
        raise SystemExit("[channels] must be a table")
    contracts = manifest["channel_contracts"]
    post_release_channels = {
        name for name, contract in contracts.items() if contract["stage"] == "post_release"
    }
    unknown = sorted(set(channels) - post_release_channels)
    if unknown:
        raise SystemExit("unsupported release channel(s): " + ", ".join(unknown))
    # An empty table set is valid: post-release channels are opt-in per
    # consumer; the root channels are contract-required instead.
    return tuple(channels)


def _preflight_outcome_status(outcome: str | None) -> str:
    """Map a GitHub Actions step outcome to a non-disclosing check status."""
    if outcome == "success":
        return "passed"
    if outcome in ("failure", "cancelled"):
        return "failed"
    return "blocked"


def _channel_outcome(
    outcomes: dict[str, object], key: str, channel_name: str, fallback_key: str | None = None
) -> str | None:
    """Read a channel-specific outcome, retaining legacy scalar compatibility."""
    outcome = outcomes.get(key)
    if isinstance(outcome, dict):
        channel_outcome = outcome.get(channel_name)
        return channel_outcome if isinstance(channel_outcome, str) else None
    if isinstance(outcome, str):
        return outcome
    if fallback_key is not None:
        fallback = outcomes.get(fallback_key)
        return fallback if isinstance(fallback, str) else None
    return None


def _channel_preflight_result(
    channel: dict[str, object], outcomes: dict[str, object], tag: str | None
) -> dict[str, object]:
    """Materialize one worker result from its contract and check outcomes."""
    checks: list[dict[str, object]] = []
    channel_name = str(channel["name"])
    for requirement, outcome_key in (
        ("publisher ownership", "ownership"),
        ("normalized release tag", "release_metadata"),
    ):
        checks.append({
            "kind": "release_authorization",
            "requirements": [requirement],
            "status": _preflight_outcome_status(outcomes.get(outcome_key)),
        })
    for key, outcome_key, fallback_key in (
        ("repository_secrets", "repository_secret_channels", "repository_secrets"),
        ("environment_secrets", "environment_secrets", None),
        ("liveness_checks", "credential_liveness_channels", "credential_liveness"),
        ("github_actions_permissions", "github_release_permissions", None),
        ("public_registry_checks", "registry_state", None),
    ):
        requirements = channel.get(key, [])
        if requirements:
            outcome = _channel_outcome(
                outcomes, outcome_key, channel_name, fallback_key
            )
            checks.append({
                "kind": key,
                "requirements": requirements,
                "status": _preflight_outcome_status(outcome),
            })
    rehearsal = channel.get("credential_rehearsal")
    statuses = [check["status"] for check in checks]
    if "failed" in statuses:
        status, diagnostic = "failed", "PREFLIGHT.CHECK_FAILED"
    elif "blocked" in statuses:
        status, diagnostic = "blocked", "PREFLIGHT.CHECK_BLOCKED"
    else:
        status, diagnostic = "passed", ""
    return {
        "name": channel["name"],
        "agent": channel["agent"],
        "tag": tag,
        "status": status,
        "checks": checks,
        "credential_rehearsal": rehearsal,
        "sanitized_diagnostic": diagnostic,
    }
