#!/usr/bin/env python3
"""Install sc-publish assets and render repository-specific manifests."""

from __future__ import annotations

import argparse
import difflib
import json
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from sc_compose import ComposeRequest


PACKAGE_ROOT = Path(__file__).resolve().parent
TEMPLATES = {
    Path("release/publish-channel-contracts.toml.j2"): Path(
        "release/publish-channel-contracts.toml"
    ),
    Path("release/publish-artifacts.toml.j2"): Path("release/publish-artifacts.toml"),
}

CHANNEL_NAMES = (
    "github_release",
    "crates_io",
    "pypi",
    "homebrew",
    "scoop",
    "winget",
)


def discover_crates(consumer: Path) -> list[str]:
    """Return publishable Cargo package names without reading release metadata."""
    if not (consumer / "Cargo.toml").is_file():
        return []
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=consumer,
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(result.stdout)
    return [
        package["name"]
        for package in metadata["packages"]
        if package.get("publish") != []
    ]


def discover_binaries(consumer: Path) -> list[str]:
    """Return Cargo binary target names for an example input only."""
    if not (consumer / "Cargo.toml").is_file():
        return []
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=consumer,
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(result.stdout)
    return sorted(
        {
            target["name"]
            for package in metadata["packages"]
            if package.get("publish") != []
            for target in package.get("targets", [])
            if "bin" in target.get("kind", [])
        }
    )


def discover_wheels(consumer: Path) -> list[str]:
    """Return Python project names without reading release metadata."""
    excluded = {".git", "target", ".venv", "venv"}
    wheels = []
    for manifest in sorted(consumer.rglob("pyproject.toml")):
        if any(part in excluded for part in manifest.parts):
            continue
        with manifest.open("rb") as file:
            project = tomllib.load(file).get("project", {})
        name = project.get("name")
        if isinstance(name, str):
            wheels.append(name)
    return wheels


def example_values(consumer: Path) -> dict[str, object]:
    """Return a source-discovered starting point; never use it for installation."""
    crates = discover_crates(consumer)
    wheels = discover_wheels(consumer)
    binaries = discover_binaries(consumer)
    has_binaries = bool(binaries)
    return {
        "release": {"version_source": "Cargo.toml", "tag_prefix": "v"},
        "artifacts": {
            "crates": [
                {"name": name, "publish_order": position}
                for position, name in enumerate(crates, start=1)
            ],
            "wheels": [
                {"package": name, "python_package": name.replace("-", "_")}
                for name in wheels
            ],
            "binaries": binaries,
        },
        "channels": {
            "github_release": {"enabled": has_binaries},
            "crates_io": {"enabled": bool(crates)},
            "pypi": {"enabled": bool(wheels), "workflow": "pypi-publish.yml"},
            "homebrew": {"enabled": has_binaries},
            "scoop": {"enabled": has_binaries},
            "winget": {"enabled": has_binaries},
        },
    }


def _require_mapping(value: object, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise argparse.ArgumentTypeError(f"{label} must be an object")
    return value


def _require_string(value: object, label: str) -> str:
    if not isinstance(value, str) or not value:
        raise argparse.ArgumentTypeError(f"{label} must be a non-empty string")
    return value


def load_install_values(path: Path) -> dict[str, object]:
    """Load the complete, caller-declared install contract without inference."""
    try:
        values = _require_mapping(json.loads(path.read_text(encoding="utf-8")), "install input")
    except OSError as error:
        raise argparse.ArgumentTypeError(f"cannot read --input {path}: {error}") from error
    except json.JSONDecodeError as error:
        raise argparse.ArgumentTypeError(f"--input must contain JSON: {error}") from error

    release = _require_mapping(values.get("release"), "release")
    _require_string(release.get("version_source"), "release.version_source")
    _require_string(release.get("tag_prefix"), "release.tag_prefix")
    artifacts = _require_mapping(values.get("artifacts"), "artifacts")
    for key in ("crates", "wheels", "binaries"):
        if not isinstance(artifacts.get(key), list):
            raise argparse.ArgumentTypeError(f"artifacts.{key} must be an array")
    publish_orders: set[int] = set()
    for position, crate in enumerate(artifacts["crates"], start=1):
        crate_value = _require_mapping(crate, f"artifacts.crates[{position}]")
        _require_string(crate_value.get("name"), f"artifacts.crates[{position}].name")
        publish_order = crate_value.get("publish_order")
        if type(publish_order) is not int or publish_order <= 0:
            raise argparse.ArgumentTypeError(
                f"artifacts.crates[{position}].publish_order must be a positive integer"
            )
        if publish_order in publish_orders:
            raise argparse.ArgumentTypeError(
                f"artifacts.crates[{position}].publish_order must be unique"
            )
        publish_orders.add(publish_order)
    for position, wheel in enumerate(artifacts["wheels"], start=1):
        wheel_value = _require_mapping(wheel, f"artifacts.wheels[{position}]")
        _require_string(wheel_value.get("package"), f"artifacts.wheels[{position}].package")
        _require_string(
            wheel_value.get("python_package"), f"artifacts.wheels[{position}].python_package"
        )
    if not all(isinstance(binary, str) and binary for binary in artifacts["binaries"]):
        raise argparse.ArgumentTypeError("artifacts.binaries must be an array of non-empty strings")

    channels = _require_mapping(values.get("channels"), "channels")
    missing_channels = [name for name in CHANNEL_NAMES if name not in channels]
    if missing_channels:
        raise argparse.ArgumentTypeError(
            f"channels must explicitly declare: {', '.join(missing_channels)}"
        )
    for name in CHANNEL_NAMES:
        channel = _require_mapping(channels[name], f"channels.{name}")
        if not isinstance(channel.get("enabled"), bool):
            raise argparse.ArgumentTypeError(f"channels.{name}.enabled must be true or false")
    _require_string(_require_mapping(channels["pypi"], "channels.pypi").get("workflow"), "channels.pypi.workflow")
    return values


def package_files() -> list[Path]:
    """Return package files that are copied unchanged into a consumer."""
    return sorted(
        path
        for path in PACKAGE_ROOT.rglob("*")
        if path.is_file()
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
        vars_input=values,
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
  1. Generate a reviewable input: --example-json install.json REPOSITORY
  2. Confirm artifact names, publish order, and enabled channels in install.json.
  3. Install: --input install.json REPOSITORY
  4. Verify a repeat install without changing files: --dry-run --input install.json REPOSITORY

All installed package assets are shared verbatim. Only the two release manifests
are rendered from the caller-owned JSON input. Exit 0 means clean/success; a
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
    input_mode = parser.add_mutually_exclusive_group()
    input_mode.add_argument(
        "--input",
        type=Path,
        metavar="INSTALL.json",
        help="required JSON file declaring release artifacts, explicit order, and channel activation",
    )
    input_mode.add_argument(
        "--example-json",
        nargs="?",
        type=Path,
        metavar="INSTALL.json",
        const=Path("-"),
        help="print or write a source-discovered starting JSON document and exit; never installs",
    )
    if len(sys.argv) == 1:
        parser.print_help()
        return 0
    args = parser.parse_args()

    consumer = args.consumer_repository.resolve()
    if not consumer.is_dir():
        parser.error(f"consumer repository does not exist: {consumer}")
    if args.example_json is not None:
        try:
            example = f"{json.dumps(example_values(consumer), indent=2)}\n"
            if args.example_json == Path("-"):
                print(example, end="")
            else:
                if args.example_json.exists():
                    parser.error(f"refusing to overwrite existing example input: {args.example_json}")
                args.example_json.write_text(example, encoding="utf-8")
                print(f"wrote reviewable install input: {args.example_json}")
        except (OSError, subprocess.CalledProcessError, tomllib.TOMLDecodeError) as error:
            parser.error(str(error))
        return 0
    if args.input is None:
        parser.error("--input is required for installation; use --example-json only to draft it")
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
