from __future__ import annotations

import json
from pathlib import Path
import subprocess
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]


class PyRecipeTests(unittest.TestCase):
    def test_arguments_reach_python_unsplit(self) -> None:
        # Spaces, quotes and shell syntax would be re-split or rejected by a
        # shell-interpolated `{{args}}`; positional arguments pass them intact.
        arguments = ["a b", "it's", 'say "hi"', "x;y", ""]
        result = subprocess.run(
            [
                "just",
                "_py",
                "-c",
                "import json, sys; print(json.dumps(sys.argv[1:]))",
                *arguments,
            ],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout.strip().splitlines()[-1]), arguments)

    def test_sc_lint_py_runs_the_kit_environment_interpreter_unsplit(self) -> None:
        # Wheel-only sc-lint helpers run on the kit venv (randlee/sc-lint#198);
        # the recipe must select that interpreter and pass arguments intact.
        arguments = ["a b", "it's", 'say "hi"', "x;y", ""]
        result = subprocess.run(
            [
                "just",
                "_sc-lint-py",
                "-c",
                "import json, sys; print(json.dumps([sys.prefix, sys.argv[1:]]))",
                *arguments,
            ],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        prefix, received = json.loads(result.stdout.strip().splitlines()[-1])
        self.assertEqual(Path(prefix).resolve(), (REPO_ROOT / ".sc-lint" / "venv").resolve())
        self.assertEqual(received, arguments)


if __name__ == "__main__":
    unittest.main()
