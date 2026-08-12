from __future__ import annotations

from pathlib import Path
import shutil
import sys
import tempfile
import unittest

JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_hermes_atm_boundary import collect_violations


class HermesAtmBoundaryLintTests(unittest.TestCase):
    def copy_fixture(self, destination: Path) -> None:
        root = JUST_DIR.parent
        for relative in (
            "boundaries/hermes-atm/runtime-composition.toml",
            "crates/hermes-atm/pyproject.toml",
            "crates/hermes-atm/src/hermes_atm",
        ):
            source = root / relative
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            if source.is_dir():
                shutil.copytree(source, target)
            else:
                shutil.copy2(source, target)

    def test_current_policy_passes(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.copy_fixture(root)
            self.assertEqual(collect_violations(root), [])

    def test_forbidden_socket_import_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.copy_fixture(root)
            source = root / "crates/hermes-atm/src/hermes_atm/runtime.py"
            source.write_text(source.read_text(encoding="utf-8") + "\nimport socket\n", encoding="utf-8")
            findings = collect_violations(root)
            self.assertTrue(any("direct_socket_io" in item.message for item in findings))

    def test_forbidden_package_edge_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.copy_fixture(root)
            package = root / "crates/hermes-atm/pyproject.toml"
            package.write_text(package.read_text(encoding="utf-8").replace('dependencies = ["atm-graft>=1.4,<1.5"]', 'dependencies = ["atm-graft>=1.4,<1.5", "atm-daemon"]'), encoding="utf-8")
            findings = collect_violations(root)
            self.assertTrue(any("forbidden package edge" in item.message for item in findings))


if __name__ == "__main__":
    unittest.main()
