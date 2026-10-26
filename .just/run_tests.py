#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path
import json
import subprocess
import sys


DEFAULT_TEST_ORDER = ("python", "build", "workspace", "cli-surface", "daemon-signing")


def repo_root() -> Path:
    return Path(__file__).resolve().parents[1]


def load_test_manifest(root: Path | None = None) -> dict[str, list[str]]:
    manifest_path = (root or repo_root()) / ".sc-lint" / "install.json"
    try:
        payload = json.loads(manifest_path.read_text(encoding="utf-8"))
        commands = payload["test"]
    except (OSError, json.JSONDecodeError, KeyError, TypeError) as error:
        raise ValueError(f"cannot read test commands from {manifest_path}: {error}") from error
    if not isinstance(commands, dict) or "default" not in commands:
        raise ValueError(f"{manifest_path}: test must define a default command and named selectors")
    validated: dict[str, list[str]] = {}
    for name, command in commands.items():
        if not isinstance(name, str) or not name or not isinstance(command, list) or not command:
            raise ValueError(f"{manifest_path}: test command {name!r} must be a non-empty argument array")
        if not all(isinstance(argument, str) and argument for argument in command):
            raise ValueError(f"{manifest_path}: test command {name!r} has an invalid argument")
        validated[name] = command
    return validated


def selected_test_commands(
    manifest: dict[str, list[str]], selectors: list[str]
) -> list[tuple[str, list[str]]]:
    named_selectors = [name for name in manifest if name not in {"default", "coverage"}]
    if not selectors:
        selectors = [name for name in DEFAULT_TEST_ORDER if name in manifest]
        selectors.extend(sorted(set(named_selectors) - set(selectors)))
    unknown = [selector for selector in selectors if selector not in named_selectors and selector != "coverage"]
    if unknown:
        valid_names = [*named_selectors]
        if "coverage" in manifest:
            valid_names.append("coverage")
        valid = ", ".join(valid_names)
        raise ValueError(f"unknown test selector(s): {', '.join(unknown)}; expected one or more of: {valid}")
    if "coverage" in selectors and "coverage" not in manifest:
        raise ValueError("coverage selector is not defined in the test manifest")
    return [(selector, manifest[selector]) for selector in selectors]


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    root = repo_root()
    try:
        manifest = load_test_manifest(root)
        selected = selected_test_commands(manifest, args)
    except ValueError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2

    for selector, command in selected:
        print(f"test {selector}: {' '.join(command)}", flush=True)
        try:
            completed = subprocess.run(command, cwd=root, check=False)
        except OSError as error:
            print(f"test {selector} failed to start: {error}", file=sys.stderr)
            return 127
        if completed.returncode:
            return completed.returncode
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
