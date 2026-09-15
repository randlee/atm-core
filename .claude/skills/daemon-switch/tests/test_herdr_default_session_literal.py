"""Keep the Rust and daemon-switch default-session literals synchronized."""

from __future__ import annotations

import importlib.util
import re
from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).parents[4]
HERDR_ENTRY = REPO_ROOT / ".claude/skills/daemon-switch/scripts/herdr_entry.py"
RUST_SESSION = REPO_ROOT / "crates/atm-core/src/delivery_channel.rs"
SPEC = importlib.util.spec_from_file_location("herdr_entry", HERDR_ENTRY)
assert SPEC is not None and SPEC.loader is not None
HERDR_ENTRY_MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(HERDR_ENTRY_MODULE)


class HerdrDefaultSessionLiteralTests(unittest.TestCase):
    def test_rust_default_name_matches_daemon_switch_constant(self) -> None:
        rust_source = RUST_SESSION.read_text(encoding="utf-8")
        match = re.search(r'pub const DEFAULT_NAME: &str = "([^"]+)";', rust_source)
        self.assertIsNotNone(match, "HerdrSession::DEFAULT_NAME must remain a string literal")
        assert match is not None
        self.assertEqual(match.group(1), HERDR_ENTRY_MODULE.HERDR_DEFAULT_SESSION)
