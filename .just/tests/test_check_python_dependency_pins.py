from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from check_python_dependency_pins import specifier_admits, validate_python_dependency_pins


class PythonDependencyPinTests(unittest.TestCase):
    def test_specifier_comparison_and_unsupported_syntax(self) -> None:
        self.assertTrue(specifier_admits("1.6.5", ">=1.6,<1.7"))
        self.assertFalse(specifier_admits("1.6.5", "==1.6"))
        self.assertFalse(specifier_admits("1.6.5", ">=1.4,<1.5"))
        with self.assertRaises(SystemExit):
            specifier_admits("1.6.5", ">=1.6,garbage")

    def test_stale_and_missing_pin_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "release").mkdir()
            (root / "release/publish-artifacts.toml").write_text('[[python_distributions]]\nname = "atm-graft"\n')
            graft = root / "crates/graft/pyproject.toml"
            graft.parent.mkdir(parents=True)
            graft.write_text('[project]\nname = "atm-graft"\nversion = "1.6.5"\n')
            hermes = root / "crates/hermes/pyproject.toml"
            hermes.parent.mkdir(parents=True)
            for dependency, allowed in (("atm-graft==1.6.5", True), ("atm-graft>=1.4,<1.5", False), ("atm-graft", False)):
                with self.subTest(dependency=dependency):
                    hermes.write_text(f'[project]\nname = "hermes"\ndependencies = ["{dependency}"]\n')
                    if allowed:
                        self.assertTrue(validate_python_dependency_pins(root, "1.6.5"))
                    else:
                        with self.assertRaises(SystemExit):
                            validate_python_dependency_pins(root, "1.6.5")


if __name__ == "__main__":
    unittest.main()
