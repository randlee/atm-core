#!/usr/bin/env python3
"""The repository's atm-bd-orchestration configuration, read at run time.

The installer renders `.claude/project/atm-bd-orchestration.yaml` in the
consuming repository; every script reads it through `load()`. There are no
defaults: a missing file raises `ConfigNotFound`, a missing key raises
`ConfigKeyMissing`, and both name the file.

    repo_config.py get <key>     print one value (a list or mapping as JSON)
    repo_config.py json          print the whole file as JSON (a dispatch --var-file base)

Exit 0 on success, 2 when the file or the key is missing.
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from typing import Any

RELATIVE_PATH = Path(".claude/project/atm-bd-orchestration.yaml")


class ConfigError(RuntimeError):
    """The repository configuration cannot be used."""


class ConfigNotFound(ConfigError):
    """The configuration file does not exist."""


class ConfigKeyMissing(ConfigError, KeyError):
    """A required key is absent from the configuration file."""

    def __str__(self) -> str:  # KeyError would quote the message
        return self.args[0]


class RepoConfig(dict):
    """The parsed file; indexing a key it lacks raises ConfigKeyMissing naming key and file."""

    def __init__(self, path: Path, data: dict[str, Any]):
        super().__init__(data)
        self.path = path

    def __missing__(self, key: str) -> Any:
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
    if not isinstance(data, dict):
        raise ConfigError(f"{path}: expected a mapping at the top level")
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
