#!/usr/bin/env python3
"""Install sc-publish assets and render repository-specific manifests."""

from __future__ import annotations

import argparse
import difflib
import json
import shutil
import sys
import tempfile
import tomllib
from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from sc_compose import ComposeRequest


PACKAGE_ROOT = Path(__file__).resolve().parent
SOURCE_ROOT_MARKER = ".sc-publish-source-root"
TEMPLATES = {
    Path("release/publish-channel-contracts.toml.j2"): Path(
        "release/publish-channel-contracts.toml"
    ),
    Path("release/publish-artifacts.toml.j2"): Path("release/publish-artifacts.toml"),
}

CHANNEL_NAMES = ("pypi", "homebrew", "scoop", "winget")


def _require_mapping(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise argparse.ArgumentTypeError(f"{label} must be an object")
    return value


def _require_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise argparse.ArgumentTypeError(f"{label} must be a non-empty string")
    return value


def _require_array(value: object, label: str) -> list[Any]:
    if not isinstance(value, list):
        raise argparse.ArgumentTypeError(f"{label} must be an array")
    return value


def _require_boolean(value: object, label: str) -> bool:
    if type(value) is not bool:
        raise argparse.ArgumentTypeError(f"{label} must be a boolean")
    return value


def _require_non_negative_integer(value: object, label: str) -> int:
    if type(value) is not int or value < 0:
        raise argparse.ArgumentTypeError(f"{label} must be a non-negative integer")
    return value


def _require_string_array(value: object, label: str) -> list[str]:
    entries = _require_array(value, label)
    for position, entry in enumerate(entries, start=1):
        _require_string(entry, f"{label}[{position}]")
    return entries


def _require_string_mapping(value: object, label: str) -> dict[str, str]:
    mapping = _require_mapping(value, label)
    for key, entry in mapping.items():
        _require_string(key, f"{label} key")
        _require_string(entry, f"{label}.{key}")
    return mapping


def _require_entries(
    values: object, label: str, required_fields: tuple[str, ...]
) -> list[dict[str, Any]]:
    entries = _require_array(values, label)
    for position, entry in enumerate(entries, start=1):
        mapping = _require_mapping(entry, f"{label}[{position}]")
        for field in required_fields:
            _require_string(mapping.get(field), f"{label}[{position}].{field}")
    return entries


def load_install_values(path: Path) -> dict[str, object]:
    """Load the complete, caller-declared publish manifest contract."""
    try:
        values = _require_mapping(json.loads(path.read_text(encoding="utf-8")), "install input")
    except OSError as error:
        raise argparse.ArgumentTypeError(f"cannot read --input {path}: {error}") from error
    except json.JSONDecodeError as error:
        raise argparse.ArgumentTypeError(f"--input must contain JSON: {error}") from error

    if values.get("schema_version") != 1:
        raise argparse.ArgumentTypeError("schema_version must be 1")

    project = _require_mapping(values.get("project"), "project")
    for field in ("name", "archive_prefix", "description", "homepage", "license"):
        _require_string(project.get(field), f"project.{field}")
    if "readme_dependency_crate" in project:
        _require_string(project["readme_dependency_crate"], "project.readme_dependency_crate")

    release_targets = _require_entries(
        values.get("release_targets"), "release_targets", ("target", "os", "archive")
    )
    if not release_targets:
        raise argparse.ArgumentTypeError("release_targets must not be empty")

    crates = _require_array(values.get("crates"), "crates")
    publish_orders: set[int] = set()
    for position, crate in enumerate(crates, start=1):
        crate_value = _require_mapping(crate, f"crates[{position}]")
        _require_string(crate_value.get("artifact"), f"crates[{position}].artifact")
        _require_string(crate_value.get("package"), f"crates[{position}].package")
        _require_string(crate_value.get("cargo_toml"), f"crates[{position}].cargo_toml")
        _require_string(
            crate_value.get("preflight_check"), f"crates[{position}].preflight_check"
        )
        _require_boolean(crate_value.get("required"), f"crates[{position}].required")
        publish = _require_boolean(crate_value.get("publish"), f"crates[{position}].publish")
        _require_boolean(crate_value.get("verify_install"), f"crates[{position}].verify_install")
        _require_non_negative_integer(
            crate_value.get("wait_after_publish_seconds"),
            f"crates[{position}].wait_after_publish_seconds",
        )
        publish_order = _require_non_negative_integer(
            crate_value.get("publish_order"), f"crates[{position}].publish_order"
        )
        if publish and publish_order == 0:
            raise argparse.ArgumentTypeError(
                f"crates[{position}].publish_order must be positive when publish is true"
            )
        if not publish and publish_order != 0:
            raise argparse.ArgumentTypeError(
                f"crates[{position}].publish_order must be zero when publish is false"
            )
        if not publish:
            continue
        if publish_order in publish_orders:
            raise argparse.ArgumentTypeError(
                f"crates[{position}].publish_order must be unique"
            )
        publish_orders.add(publish_order)

    release_binaries = _require_entries(values.get("release_binaries"), "release_binaries", ("name",))
    for position, binary in enumerate(release_binaries, start=1):
        if "bundled_paths" not in binary:
            continue
        for path_position, bundled_path in enumerate(
            _require_array(binary["bundled_paths"], f"release_binaries[{position}].bundled_paths"),
            start=1,
        ):
            bundled = _require_mapping(
                bundled_path, f"release_binaries[{position}].bundled_paths[{path_position}]"
            )
            _require_string(bundled.get("source"), "bundled_paths.source")
            _require_string(bundled.get("destination"), "bundled_paths.destination")
            _require_string_array(
                bundled.get("homebrew_destination_components"),
                "bundled_paths.homebrew_destination_components",
            )

    if "installed_docs" in values:
        installed_docs = _require_mapping(values["installed_docs"], "installed_docs")
        for field in ("source_root", "install_root", "entrypoint"):
            _require_string(installed_docs.get(field), f"installed_docs.{field}")

    _require_entries(
        values.get("python_packages"),
        "python_packages",
        ("artifact", "package", "manifest", "module", "publish"),
    )
    distributions = _require_entries(
        values.get("python_distributions"),
        "python_distributions",
        ("name", "source", "module_path"),
    )
    for position, distribution in enumerate(distributions, start=1):
        _require_boolean(distribution.get("sdist"), f"python_distributions[{position}].sdist")
        _require_string_array(distribution.get("wheels"), f"python_distributions[{position}].wheels")
        cargo_manifest = distribution.get("cargo_manifest")
        build_system = distribution.get("build_system")
        if cargo_manifest is None and build_system is None:
            raise argparse.ArgumentTypeError(
                f"python_distributions[{position}] requires cargo_manifest or build_system"
            )
        if cargo_manifest is not None and build_system is not None:
            raise argparse.ArgumentTypeError(
                f"python_distributions[{position}] must not set both cargo_manifest and build_system"
            )
        if cargo_manifest is not None:
            _require_string(cargo_manifest, f"python_distributions[{position}].cargo_manifest")
        if build_system is not None:
            if _require_string(build_system, f"python_distributions[{position}].build_system") != "setuptools":
                raise argparse.ArgumentTypeError(
                    f"python_distributions[{position}].build_system must be setuptools"
                )

    channels = _require_mapping(values.get("channels"), "channels")
    missing_channels = [name for name in CHANNEL_NAMES if name not in channels]
    if missing_channels:
        raise argparse.ArgumentTypeError(
            f"channels must explicitly declare: {', '.join(missing_channels)}"
        )
    for name in CHANNEL_NAMES:
        channel = _require_mapping(channels[name], f"channels.{name}")
        _require_string(channel.get("workflow"), f"channels.{name}.workflow")
        _require_string_mapping(channel.get("dispatch_inputs"), f"channels.{name}.dispatch_inputs")
    pypi = _require_mapping(channels["pypi"], "channels.pypi")
    _require_string_mapping(pypi.get("credential_rehearsal_inputs", {}), "channels.pypi.credential_rehearsal_inputs")
    for field in ("test_repository", "production_repository"):
        _require_string(pypi.get(field), f"channels.pypi.{field}")
    homebrew = _require_mapping(channels["homebrew"], "channels.homebrew")
    for field in ("tap_repository", "renderer_target"):
        _require_string(homebrew.get(field), f"channels.homebrew.{field}")
    for formula_position, formula in enumerate(
        _require_array(homebrew.get("formulas"), "channels.homebrew.formulas"), start=1
    ):
        formula_value = _require_mapping(formula, f"channels.homebrew.formulas[{formula_position}]")
        for field in ("path", "template", "class", "test_command", "test_output", "release_track"):
            _require_string(formula_value.get(field), f"channels.homebrew.formulas[{formula_position}].{field}")
        _require_string_array(formula_value.get("binaries"), f"channels.homebrew.formulas[{formula_position}].binaries")
    _require_entries(homebrew.get("assets"), "channels.homebrew.assets", ("key", "target"))
    for name, fields in {
        "winget": ("identifier", "installer_target"),
        "scoop": (
            "bucket_repository",
            "manifest_path",
            "manifest_template",
            "installer_target",
            "binary",
            "renderer_target",
        ),
    }.items():
        channel = _require_mapping(channels[name], f"channels.{name}")
        for field in fields:
            _require_string(channel.get(field), f"channels.{name}.{field}")

    if "homebrew" in channels or "scoop" in channels:
        _require_string(project.get("renderer_archive_path"), "project.renderer_archive_path")
    return values


def _toml_literal(value: object) -> str:
    """Return a TOML literal for JSON-compatible manifest values."""
    if isinstance(value, str):
        return json.dumps(value)
    if type(value) is bool:
        return "true" if value else "false"
    if type(value) is int:
        return str(value)
    if isinstance(value, list):
        return "[" + ", ".join(_toml_literal(entry) for entry in value) + "]"
    if isinstance(value, dict):
        entries = []
        for key, entry in value.items():
            key_text = key if key.replace("_", "").replace("-", "").isalnum() else json.dumps(key)
            entries.append(f"{key_text} = {_toml_literal(entry)}")
        return "{ " + ", ".join(entries) + " }"
    raise TypeError(f"unsupported TOML literal value: {type(value).__name__}")


def _toml_scalars(mapping: dict[str, Any], literal_lists: tuple[str, ...] = ()) -> dict[str, Any]:
    """Serialize scalar template fields while retaining table-array structure."""
    converted: dict[str, Any] = {}
    for key, value in mapping.items():
        if key in literal_lists:
            converted[key] = _toml_literal(value)
        elif isinstance(value, (str, bool, int)):
            converted[key] = _toml_literal(value)
        else:
            converted[key] = value
    return converted


def template_values(values: dict[str, object]) -> dict[str, object]:
    """Convert validated JSON values to TOML-safe values for the Jinja template."""
    project = _require_mapping(values["project"], "project")
    channels = _require_mapping(values["channels"], "channels")
    converted_channels: dict[str, Any] = {}
    for name, channel_value in channels.items():
        channel = _require_mapping(channel_value, f"channels.{name}")
        converted = _toml_scalars(
            channel,
            ("dispatch_inputs", "credential_rehearsal_inputs"),
        )
        if name == "homebrew":
            converted["formulas"] = [
                _toml_scalars(
                    _require_mapping(formula, "channels.homebrew.formulas entry"), ("binaries",)
                )
                for formula in _require_array(channel["formulas"], "channels.homebrew.formulas")
            ]
            converted["assets"] = [
                _toml_scalars(_require_mapping(asset, "channels.homebrew.assets entry"))
                for asset in _require_array(channel["assets"], "channels.homebrew.assets")
            ]
        converted_channels[name] = converted

    release_binaries = []
    for binary_value in _require_array(values["release_binaries"], "release_binaries"):
        binary = _require_mapping(binary_value, "release_binaries entry")
        converted = _toml_scalars(binary, ("bundled_paths",))
        converted["has_bundled_paths"] = "bundled_paths" in binary
        release_binaries.append(converted)

    distributions = []
    for distribution_value in _require_array(values["python_distributions"], "python_distributions"):
        distribution = _toml_scalars(
            _require_mapping(distribution_value, "python_distributions entry"), ("wheels",)
        )
        # Both build systems use one template. Empty sentinels keep the other
        # branch defined under strict-undeclared-variable rendering.
        distribution.setdefault("cargo_manifest", "")
        distribution.setdefault("build_system", "")
        distributions.append(distribution)

    template_project = _toml_scalars(project)
    template_project.setdefault("readme_dependency_crate", "")
    template_project.setdefault("renderer_archive_path", "")
    installed_docs = _toml_scalars(
        _require_mapping(values.get("installed_docs", {}), "installed_docs")
    )
    for field in ("source_root", "install_root", "entrypoint"):
        installed_docs.setdefault(field, "")

    return {
        "schema_version": _toml_literal(values["schema_version"]),
        "project": template_project,
        "release_targets": [
            _toml_scalars(_require_mapping(target, "release_targets entry"))
            for target in _require_array(values["release_targets"], "release_targets")
        ],
        "crates": [
            _toml_scalars(_require_mapping(crate, "crates entry"))
            for crate in _require_array(values["crates"], "crates")
        ],
        "release_binaries": release_binaries,
        "installed_docs": installed_docs,
        "has_installed_docs": "installed_docs" in values,
        "python_packages": [
            _toml_scalars(_require_mapping(package, "python_packages entry"))
            for package in _require_array(values["python_packages"], "python_packages")
        ],
        "python_distributions": distributions,
        "channels": converted_channels,
        "has_readme_dependency_crate": "readme_dependency_crate" in project,
        "has_renderer_archive_path": "renderer_archive_path" in project,
    }


def package_files() -> list[Path]:
    """Return package files that are copied unchanged into a consumer."""
    return sorted(
        path
        for path in PACKAGE_ROOT.rglob("*")
        if path.is_file()
        and path.name != SOURCE_ROOT_MARKER
        and "__pycache__" not in path.parts
        and path.suffix != ".pyc"
    )


def print_diff(destination: Path, source: Path, relative: Path) -> None:
    before = destination.read_text(encoding="utf-8").splitlines(keepends=True) if destination.exists() else []
    after = source.read_text(encoding="utf-8").splitlines(keepends=True)
    sys.stdout.writelines(
        difflib.unified_diff(
            before,
            after,
            fromfile=f"consumer/{relative}",
            tofile=f"sc-publish/{relative}",
        )
    )


def render_template(template: Path, values: dict[str, object], output: Path) -> None:
    """Render a package template through the pinned Python binding contract."""
    try:
        import sc_compose
    except ModuleNotFoundError as error:
        raise RuntimeError(
            "sc-compose Python bindings are required to install; run "
            ".github/scripts/bootstrap_sc_compose.py first"
        ) from error
    template_path = template if template.is_absolute() else PACKAGE_ROOT / template
    request: ComposeRequest = sc_compose.ComposeRequest(
        root=PACKAGE_ROOT,
        mode=sc_compose.ComposeMode.file(str(template_path.relative_to(PACKAGE_ROOT))),
        vars_input=template_values(values),
        policy=sc_compose.ComposePolicy(strict_undeclared_variables=True),
    )
    rendered = sc_compose.compose_file(request).rendered_text
    tomllib.loads(rendered)
    output.write_text(rendered, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""workflow:
  1. Create a complete, reviewable JSON manifest contract for this repository.
  2. Confirm artifact names, publish order, targets, package distributions, and channels.
  3. Install: --input install.json REPOSITORY
  4. Verify a repeat install without changing files: --dry-run --input install.json REPOSITORY

All installed package assets are shared verbatim. Only the two release manifests
are rendered from the caller-owned complete JSON input. Exit 0 means clean/success; a
dry-run returns 1 when consumer files would change.""",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="show drift without changes; return 1 when installation is needed",
    )
    parser.add_argument(
        "consumer_repository",
        nargs="?",
        type=Path,
        default=Path.cwd(),
        metavar="REPOSITORY",
        help="consumer repository (default: current directory)",
    )
    parser.add_argument(
        "--input",
        type=Path,
        metavar="INSTALL.json",
        help="required complete JSON file declaring the release manifest contract",
    )
    if len(sys.argv) == 1:
        parser.print_help()
        return 0
    args = parser.parse_args()

    consumer = args.consumer_repository.resolve()
    if not consumer.is_dir():
        parser.error(f"consumer repository does not exist: {consumer}")
    if args.input is None:
        parser.error("--input is required for installation")
    try:
        values = load_install_values(args.input)
    except argparse.ArgumentTypeError as error:
        parser.error(str(error))

    with tempfile.TemporaryDirectory() as temporary_directory:
        rendered_templates = {
            template: Path(temporary_directory) / output.name
            for template, output in TEMPLATES.items()
        }
        try:
            for template, rendered in rendered_templates.items():
                render_template(template, values, rendered)
        except RuntimeError as error:
            parser.error(str(error))

        changed = False
        for source in package_files():
            relative = source.relative_to(PACKAGE_ROOT)
            destination = consumer / relative
            if destination.exists() and destination.read_bytes() == source.read_bytes():
                continue
            changed = True
            if args.dry_run:
                print_diff(destination, source, relative)
                continue
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
            print(f"copied {relative}")

        for template, output in TEMPLATES.items():
            destination = consumer / output
            rendered = rendered_templates[template]
            if destination.exists() and destination.read_bytes() == rendered.read_bytes():
                continue
            changed = True
            if args.dry_run:
                print_diff(destination, rendered, output)
            else:
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(rendered, destination)
                print(f"rendered {output}")

    if args.dry_run:
        if changed:
            return 1
        print("Publish-kit assets are in sync.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
