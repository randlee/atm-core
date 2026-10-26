"""Controlled file-list fixtures for the CI runtime classifier."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest


SCRIPT = Path(__file__).resolve().parents[2] / ".github/scripts/ci_scope.py"
SPEC = importlib.util.spec_from_file_location("ci_scope", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
ci_scope = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci_scope)


class CiScopeTests(unittest.TestCase):
    def test_runtime_inputs(self) -> None:
        for path in (
            "crates/atm-http-runtime/src/lib.rs",
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "rust-toolchain-nightly",
            "build.rs",
            ".github/workflows/ci.yml",
            ".github/scripts/ci_scope.py",
            "Justfile",
        ):
            with self.subTest(path=path):
                self.assertTrue(ci_scope.requires_runtime(["docs/readme.md", path]))

    def test_fast_path_inputs(self) -> None:
        self.assertFalse(
            ci_scope.requires_runtime(
                [
                    "scripts/release.py",
                    ".just/run_lint.py",
                    "release/publish-artifacts.toml",
                    "docs/plan.md",
                    ".claude/agents/reviewer.md",
                    "site/index.html",
                    ".github/actions/setup-renderer/action.yml",
                    "crates-not/README.md",
                    "nested/Cargo.toml",
                ]
            )
        )
        self.assertFalse(ci_scope.requires_runtime([]))


if __name__ == "__main__":
    unittest.main()
