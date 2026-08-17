from __future__ import annotations

import os
from pathlib import Path
import sys
import tempfile
import textwrap
import tomllib
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
ROOT = JUST_DIR.parent
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from run_hermes_graft_bridge_tests import bridge_test_environment
from run_hermes_graft_bridge_tests import build_or_locate_graft_wheel
from run_hermes_graft_bridge_tests import GRAFT_WHEEL_ENV
from run_hermes_graft_bridge_tests import project_dependency_requirement
from run_hermes_graft_bridge_tests import require_universal_python_wheel
from run_hermes_graft_bridge_tests import WHEEL_OUTPUT_DIR_ENV
from run_hermes_graft_bridge_tests import wheel_output_dir


def release_wheel_selector_script(workflow: str) -> str:
    step_start = workflow.index("      - name: Select host release atm-graft wheel")
    step_end = workflow.index("\n      - name:", step_start + 1)
    step = workflow[step_start:step_end]
    run_marker = "        run: |\n"
    script_start = step.index(run_marker) + len(run_marker)
    return textwrap.dedent(step[script_start:])


class HermesGraftBridgeRunnerTests(unittest.TestCase):
    def test_environment_does_not_inject_checked_in_adapter_source(self) -> None:
        with mock.patch.dict("os.environ", {"PATH": "/system/bin"}, clear=True):
            environment = bridge_test_environment(Path("/tmp/venv"), Path("/tmp/venv/bin/python"))

        self.assertEqual(Path(environment["VIRTUAL_ENV"]), Path("/tmp/venv"))
        self.assertNotIn("PYTHONPATH", environment)
        self.assertNotIn("HERMES_SRC", environment)

    def test_wheel_output_dir_honors_configured_directory(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            configured_directory = Path(temporary_directory) / "release-output"
            with mock.patch.dict(
                "os.environ", {WHEEL_OUTPUT_DIR_ENV: str(configured_directory)}, clear=True
            ):
                output_directory = wheel_output_dir(Path(temporary_directory) / "temporary")

            self.assertEqual(output_directory, configured_directory.resolve())
            self.assertTrue(output_directory.is_dir())

    def test_graft_wheel_override_uses_existing_release_wheel(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            wheel_path = Path(temporary_directory) / "atm_graft-1.4.2-cp311-abi3-win_amd64.whl"
            wheel_path.touch()
            with mock.patch.dict("os.environ", {GRAFT_WHEEL_ENV: str(wheel_path)}, clear=True):
                selected_wheel = build_or_locate_graft_wheel(
                    python=Path("/unused/python"),
                    wheel_dir=Path(temporary_directory),
                    environment={},
                )

            self.assertEqual(selected_wheel, wheel_path.resolve())

    def test_graft_wheel_override_rejects_missing_or_wrong_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            invalid_wheel = Path(temporary_directory) / "not-atm-graft.whl"
            invalid_wheel.touch()
            with mock.patch.dict("os.environ", {GRAFT_WHEEL_ENV: str(invalid_wheel)}, clear=True):
                with self.assertRaisesRegex(RuntimeError, GRAFT_WHEEL_ENV):
                    build_or_locate_graft_wheel(
                        python=Path("/unused/python"),
                        wheel_dir=Path(temporary_directory),
                        environment={},
                    )

    def test_ci_runs_the_bridge_boundary_suite_for_each_supported_python(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")

        self.assertIn("Run Hermes graft steer boundary tests", workflow)
        self.assertIn("python .just/run_hermes_graft_bridge_tests.py", workflow)
        self.assertIn("Hermes ATM release wheel (${{ matrix.name }})", workflow)
        self.assertIn("Hermes ATM abi3 compatibility (Python", workflow)
        self.assertIn("Download host release-wheel candidate", workflow)
        self.assertIn("ATM_GRAFT_WHEEL=", workflow)
        self.assertIn("ATM_WHEEL_OUTPUT_DIR=", workflow)
        for version in ("3.11", "3.12", "3.13", "3.14"):
            self.assertIn(f'"{version}"', workflow)

    def test_ci_release_wheel_selector_writes_distinct_environment_lines(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        selector = release_wheel_selector_script(workflow)

        with tempfile.TemporaryDirectory() as temporary_directory:
            temporary_path = Path(temporary_directory)
            release_wheels = temporary_path / "release-wheels"
            release_wheels.mkdir()
            wheel = release_wheels / "atm_graft-1.4.2-cp311-abi3-win_amd64.whl"
            wheel.touch()
            github_environment = temporary_path / "github-env"
            original_directory = Path.cwd()
            try:
                os.chdir(temporary_path)
                with mock.patch.dict(
                    "os.environ", {"GITHUB_ENV": str(github_environment)}, clear=True
                ):
                    exec(selector, {"__name__": "__main__"})
            finally:
                os.chdir(original_directory)

            self.assertEqual(
                github_environment.read_text(encoding="utf-8").splitlines(),
                [
                    f"ATM_GRAFT_WHEEL={wheel.resolve()}",
                    f"ATM_WHEEL_OUTPUT_DIR={(temporary_path / 'bridge-wheels').resolve()}",
                ],
            )

    def test_bridge_runner_rejects_non_universal_hermes_wheel(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "universal Python wheel"):
            require_universal_python_wheel(Path("hermes_atm-1.4.2-cp311-cp311-any.whl"))

    def test_bridge_runner_uses_graft_manifest_pydantic_constraint(self) -> None:
        requirement = project_dependency_requirement(
            ROOT / "crates" / "atm-graft-python" / "pyproject.toml", "pydantic"
        )

        self.assertEqual(requirement, "pydantic>=2,<3")

    def test_python_artifacts_declare_the_same_supported_python_release_range(self) -> None:
        for manifest in (
            ROOT / "crates" / "atm-graft-python" / "pyproject.toml",
            ROOT / "crates" / "hermes-atm" / "pyproject.toml",
        ):
            metadata = tomllib.loads(manifest.read_text(encoding="utf-8"))
            self.assertEqual(metadata["project"]["requires-python"], ">=3.11,<3.15")


if __name__ == "__main__":
    unittest.main()
