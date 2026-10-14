"""Local-only proof that every sc-compose consumer tracks one current release."""

from __future__ import annotations

import importlib.metadata
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib
import unittest
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[2]
PIN_FILE = ROOT / "tools" / "bootstrap.toml"
CARGO_FILE = ROOT / "crates" / "atm-template-sc-compose" / "Cargo.toml"
LOCK_FILE = ROOT / "Cargo.lock"
VERSION_RE = re.compile(r"(?<!\d)(\d+\.\d+\.\d+)(?!\d)")
REGISTRY_HEADERS = {"User-Agent": "atm-core sc-compose version contract"}


def _release_json(url: str) -> dict[str, object]:
    """Fetch registry data; network errors intentionally fail this local proof."""

    with urlopen(Request(url, headers=REGISTRY_HEADERS), timeout=20) as response:
        payload = json.load(response)
    if not isinstance(payload, dict):
        raise AssertionError(f"registry response at {url} is not a JSON object")
    return payload


def _latest_pypi() -> str:
    info = _release_json("https://pypi.org/pypi/sc-compose/json").get("info")
    if not isinstance(info, dict) or not isinstance(info.get("version"), str):
        raise AssertionError("PyPI sc-compose response has no info.version")
    return info["version"]


def _latest_crate(name: str) -> str:
    crate = _release_json(f"https://crates.io/api/v1/crates/{name}").get("crate")
    if not isinstance(crate, dict) or not isinstance(crate.get("max_version"), str):
        raise AssertionError(f"crates.io {name} response has no crate.max_version")
    return crate["max_version"]


def _version_output(command: list[str]) -> str:
    output = subprocess.check_output(command, cwd=ROOT, text=True, stderr=subprocess.STDOUT)
    match = VERSION_RE.search(output)
    if match is None:
        raise AssertionError(f"{' '.join(command)} did not report a release version: {output!r}")
    return match.group(1)


def _release_mismatches(pin: str, *, pypi: str, crates: dict[str, str]) -> list[str]:
    """Return update instructions for a pin that trails a public registry."""

    messages: list[str] = []
    if pypi != pin:
        messages.append(f"{PIN_FILE} is stale; bump [sc-compose].version and run just bootstrap")
    for crate, version in crates.items():
        if version != pin:
            messages.append(
                f"{PIN_FILE} and {CARGO_FILE} are stale; bump the bootstrap pin then cargo update -p {crate}"
            )
    return messages


@unittest.skipIf("CI" in os.environ, "registry and installed-tool parity is a local-only test")
class ScComposeVersionTests(unittest.TestCase):
    def test_all_sources_and_installed_tools_match_current_release(self) -> None:
        pin = tomllib.loads(PIN_FILE.read_text(encoding="utf-8"))["sc-compose"]["version"]
        self.assertIsInstance(pin, str)
        assert isinstance(pin, str)

        cargo_dependencies = tomllib.loads(CARGO_FILE.read_text(encoding="utf-8"))["dependencies"]
        locked_packages = {
            package["name"]: package["version"]
            for package in tomllib.loads(LOCK_FILE.read_text(encoding="utf-8"))["package"]
            if package["name"] in {"sc-composer", "sc-sha"}
        }
        for crate in ("sc-composer", "sc-sha"):
            self.assertEqual(
                cargo_dependencies[crate],
                pin,
                f"{CARGO_FILE} {crate} must use Cargo's caret requirement at {pin}; bump {PIN_FILE} first",
            )
            self.assertEqual(
                locked_packages[crate],
                pin,
                f"{LOCK_FILE} {crate} is stale; bump {PIN_FILE} then cargo update -p {crate}",
            )
        newest = {crate: _latest_crate(crate) for crate in ("sc-composer", "sc-sha")}
        mismatches = _release_mismatches(pin, pypi=_latest_pypi(), crates=newest)
        self.assertEqual(
            mismatches,
            [],
            " ; ".join(mismatches),
        )
        self.assertEqual(
            _version_output(["sc-compose", "--version"]),
            pin,
            f"installed sc-compose CLI differs from {PIN_FILE}; run just bootstrap",
        )
        self.assertEqual(
            importlib.metadata.version("sc-compose"),
            pin,
            f"installed python3 sc-compose wheel differs from {PIN_FILE}; run just bootstrap",
        )

    def test_stale_pin_has_actionable_failures(self) -> None:
        messages = _release_mismatches(
            "0.0.0", pypi="1.7.0", crates={"sc-composer": "1.7.0", "sc-sha": "1.7.0"}
        )
        self.assertEqual(len(messages), 3)
        self.assertTrue(all("bump" in message and "bootstrap" in message for message in messages))

    def test_sc_compose_logs_are_ignored_and_absent_from_untracked_manifest_input(self) -> None:
        generated = ".sc-compose/logs/local-version-check.jsonl"
        ignored = subprocess.run(
            ["git", "check-ignore", "--quiet", generated], cwd=ROOT, check=False
        )
        self.assertEqual(ignored.returncode, 0, f"{generated} must be ignored by {ROOT / '.gitignore'}")
        untracked = subprocess.check_output(
            ["git", "ls-files", "--others", "--exclude-standard"], cwd=ROOT, text=True
        ).splitlines()
        self.assertNotIn(generated, untracked, "generated sc-compose logs must not enter manifest inputs")


if __name__ == "__main__":
    unittest.main()
