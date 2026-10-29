#!/usr/bin/env python3
# pyright: strict
"""The repository's atm-bd-orchestration configuration, read at run time.

The installer renders `.claude/project/atm-bd-orchestration.yaml` in the
consuming repository; every script reads it through `load()`. Only
max_sanity_rounds (5) and gate_timeout_s (180) have defaults: a missing file raises `ConfigNotFound`, a missing key raises
`ConfigKeyMissing`, and both name the file.

    repo_config.py get <key>     print one value (a list or mapping as JSON)
    repo_config.py json          print the whole file as JSON (a dispatch --var-file base)

Exit 0 on success, 2 when the file or the key is missing.
"""
from __future__ import annotations

import json
import subprocess
import sys
from collections.abc import Sequence
from pathlib import Path
from typing import Literal, TypeAlias, TypeGuard, overload

RELATIVE_PATH = Path(".claude/project/atm-bd-orchestration.yaml")
ConfigValue: TypeAlias = str | int | float | bool | None | Sequence["ConfigValue"] | dict[str, "ConfigValue"]


def is_object_list(value: object) -> TypeGuard[list[object]]:
    return isinstance(value, list)


def is_object_mapping(value: object) -> TypeGuard[dict[object, object]]:
    return isinstance(value, dict)


def is_config_value(value: object) -> TypeGuard[ConfigValue]:
    if value is None or isinstance(value, (str, bool, int, float)):
        return True
    if is_object_list(value):
        for item in value:
            if not is_config_value(item):
                return False
        return True
    if is_object_mapping(value):
        for key, item in value.items():
            if not isinstance(key, str) or not is_config_value(item):
                return False
        return True
    return False


def is_string_list(value: object) -> TypeGuard[Sequence[str]]:
    return is_object_list(value) and all(isinstance(item, str) for item in value)


def is_config_mapping(value: object) -> TypeGuard[dict[str, ConfigValue]]:
    if not is_object_mapping(value):
        return False
    for key, item in value.items():
        if not isinstance(key, str) or not is_config_value(item):
            return False
    return True


class ConfigError(RuntimeError):
    """The repository configuration cannot be used."""


class ConfigNotFound(ConfigError):
    """The configuration file does not exist."""


class ConfigKeyMissing(ConfigError, KeyError):
    """A required key is absent from the configuration file."""

    def __str__(self) -> str:  # KeyError would quote the message
        return self.args[0]


class RepoConfig(dict[str, ConfigValue]):
    """The parsed file; indexing a key it lacks raises ConfigKeyMissing naming key and file."""

    def __init__(self, path: Path, data: dict[str, ConfigValue]) -> None:
        super().__init__(data)
        self.path = path

    @overload
    def __getitem__(self, key: Literal[
        "bead_prefix", "dev_sanity_member", "qa_member", "team_lead_member", "worktree_base",
        "test_command", "lint_command", "integration_branch_pattern", "plans_dir", "policy_path",
    ]) -> str: ...

    @overload
    def __getitem__(self, key: Literal["requirements_globs", "adr_globs", "reviewers_round1"]) -> Sequence[str]: ...

    @overload
    def __getitem__(self, key: Literal["max_sanity_rounds", "gate_timeout_s"]) -> int: ...

    @overload
    def __getitem__(self, key: str) -> ConfigValue: ...

    def __getitem__(self, key: str) -> ConfigValue:
        value = super().__getitem__(key)
        if key in {
            "bead_prefix", "dev_sanity_member", "qa_member", "team_lead_member", "worktree_base",
            "test_command", "lint_command", "integration_branch_pattern", "plans_dir", "policy_path",
        } and not isinstance(value, str):
            raise ConfigError(f"{self.path}: {key} must be a string")
        if key in {"requirements_globs", "adr_globs", "reviewers_round1"} and not is_string_list(value):
            raise ConfigError(f"{self.path}: {key} must be a list of strings")
        if key in {"max_sanity_rounds", "gate_timeout_s"} and (
            isinstance(value, bool) or not isinstance(value, int)
        ):
            raise ConfigError(f"{self.path}: {key} must be an integer")
        return value

    def __missing__(self, key: str) -> ConfigValue:
        raise ConfigKeyMissing(f"{self.path}: required key '{key}' is missing; re-run the atm-bd-orchestration installer")


def repo_root(start: Path | None = None) -> Path:
    try:
        out = subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=start, check=True,
                             capture_output=True, text=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError) as exc:
        detail = getattr(exc, "stderr", "") or str(exc)
        raise ConfigError(f"not inside a git repository: {detail.strip()}") from exc
    return Path(out)


def load(repo: Path | None = None) -> RepoConfig:
    """Load the configuration of `repo` (default: the git toplevel of the working directory)."""
    root = Path(repo) if repo is not None else repo_root()
    path = root / RELATIVE_PATH
    try:
        text = path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise ConfigNotFound(f"{path}: not found; install atm-bd-orchestration into this repository") from exc
    try:
        import yaml
    except ImportError as exc:  # pragma: no cover - environment dependent
        raise ConfigError("PyYAML is required to read the repository configuration (pip install pyyaml)") from exc
    try:
        data = yaml.safe_load(text)
    except yaml.YAMLError as exc:
        raise ConfigError(f"{path}: invalid YAML: {exc}") from exc
    if not is_config_mapping(data):
        raise ConfigError(f"{path}: expected a mapping at the top level")
    if "lead" in data:
        raise ConfigError(f"{path}: lead belongs in .atm-bd/phase-<x>.toml; set each phase's lead and re-run the installer to remove the repository lead")
    rounds = data.setdefault("max_sanity_rounds", 5)
    if isinstance(rounds, bool) or not isinstance(rounds, int) or rounds < 1:
        raise ConfigError(f"{path}: max_sanity_rounds must be a positive integer")
    timeout = data.setdefault("gate_timeout_s", 180)
    if isinstance(timeout, bool) or not isinstance(timeout, int) or timeout < 1:
        raise ConfigError(f"{path}: gate_timeout_s must be a positive integer number of seconds")
    return RepoConfig(path, data)


def main(argv: list[str]) -> int:
    if argv[:1] == ["json"] and len(argv) == 1:
        print(json.dumps(dict(load()), indent=2))
        return 0
    if argv[:1] == ["get"] and len(argv) == 2:
        value = load()[argv[1]]
        print(value if isinstance(value, str) else json.dumps(value))
        return 0
    print("usage: repo_config.py get <key> | json", file=sys.stderr)
    return 2


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except ConfigError as exc:
        print(f"repo_config: {exc}", file=sys.stderr)
        raise SystemExit(2)
