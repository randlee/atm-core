from __future__ import annotations

from pathlib import Path
import sys
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

from bootstrap_python_tools import REQUIREMENTS
from bootstrap_python_tools import commands
from bootstrap_python_tools import venv_python


class BootstrapPythonToolsTests(unittest.TestCase):
    def test_bootstrap_uses_the_shared_requirements_manifest(self) -> None:
        venv_dir = Path("/tmp/atm-tools")
        planned = commands("python3.14", venv_dir)

        self.assertEqual(planned[0], ["python3.14", "-m", "venv", str(venv_dir)])
        self.assertEqual(planned[2][-2:], ["--requirement", str(REQUIREMENTS)])
        self.assertEqual(planned[3][-2:], ["pip", "check"])

    def test_venv_python_uses_platform_specific_path(self) -> None:
        python = venv_python(Path(".venv") / "atm-tools")
        self.assertIn("atm-tools", str(python))
        self.assertEqual(python.name, "python.exe" if sys.platform == "win32" else "python")

    def test_justfile_accepts_the_bootstrap_python_environment_variable(self) -> None:
        justfile = (ROOT / "Justfile").read_text(encoding="utf-8")

        self.assertIn('env_var_or_default("ATM_PYTHON_CMD", default_python_cmd)', justfile)


if __name__ == "__main__":
    unittest.main()
