from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).parents[1] / "check-runtime-stderr.py"
SPEC = importlib.util.spec_from_file_location("check_runtime_stderr", SCRIPT)
assert SPEC and SPEC.loader
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


class RuntimeStderrGateTests(unittest.TestCase):
    def test_early_test_import_does_not_hide_production_violation(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            source = root / "crates/atm-daemon-bootstrap/src/example.rs"
            source.parent.mkdir(parents=True)
            source.write_text("#[cfg(test)]\nuse test_support::Thing;\nfn live() { eprintln!(\"bad\"); }\n#[cfg(test)]\nmod tests { #[test] fn proof() { eprintln!(\"ok\"); } }\n")
            self.assertEqual(GATE.violations(root), ["crates/atm-daemon-bootstrap/src/example.rs:3: runtime stdout/stderr bypass"])
