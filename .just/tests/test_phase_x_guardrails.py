from __future__ import annotations

from pathlib import Path
import importlib.util
import sys
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]


def load_script(module_name: str, relative_path: str):
    script_path = REPO_ROOT / relative_path
    spec = importlib.util.spec_from_file_location(module_name, script_path)
    if spec is None or spec.loader is None:
        raise ImportError(f"unable to import {script_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


LEGACY_MODULE = load_script("check_legacy_mailbox_paths", "scripts/check-legacy-mailbox-paths.py")
CAPABILITY_MODULE = load_script("check_capability_degradation", "scripts/check-capability-degradation.py")


class PhaseXGuardrailTests(unittest.TestCase):
    def test_legacy_guardrail_allows_expected_workflow_compat_line(self) -> None:
        self.assertTrue(
            LEGACY_MODULE.is_allowed_legacy_literal(
                "crates/atm-core/src/workflow.rs",
                '            .or_else(|| value.strip_prefix("legacy:"))',
            )
        )

    def test_legacy_guardrail_rejects_new_legacy_runtime_symbol(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            path = repo_root / "crates" / "atm-core" / "src"
            path.mkdir(parents=True)
            (path / "sample.rs").write_text(
                "fn demo() { let runtime = LegacyMailboxRuntime::default(); }\n",
                encoding="utf-8",
            )

            violations = LEGACY_MODULE.find_violations(repo_root)

        self.assertEqual(len(violations), 1)
        self.assertEqual(violations[0].label, "legacy mailbox runtime type")

    def test_capability_guardrail_rejects_new_replay_store_none_assignment(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            path = repo_root / "crates" / "atm-daemon" / "src"
            path.mkdir(parents=True)
            (path / "sample.rs").write_text(
                "fn demo() {\n    let replay_store = None;\n}\n",
                encoding="utf-8",
            )

            violations = CAPABILITY_MODULE.find_violations(repo_root)

        self.assertEqual(len(violations), 1)
        self.assertEqual(violations[0].label, "replay capability degradation assignment")


if __name__ == "__main__":
    unittest.main()
