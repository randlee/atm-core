"""Unit tests for the prompt-driven colima integration driver."""

from __future__ import annotations

import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from scripts.integration import run_colima as DRIVER


def report(test_id: str, verdict: str = "pass") -> dict[str, object]:
    return {
        "schema": "prompt-report-1",
        "test_id": test_id,
        "agent": "claude-code",
        "steps": [{"name": "observable", "status": verdict, "detail": "atm task events --json: {}"}],
        "verdict": verdict,
        "atm_versions": {"atm": "1.5.17", "hermes_atm": "n/a", "atm_graft": "n/a"},
        "started_at": "2026-09-14T00:00:00Z",
        "finished_at": "2026-09-14T00:01:00Z",
    }


class PromptReportTests(unittest.TestCase):
    def test_valid_report_becomes_one_step_payload(self) -> None:
        payload = DRIVER.step_payload(
            DRIVER.validate_prompt_report(report("AT9-task-start"), "AT9-task-start"),
            step_name="task-start",
            revision="a" * 40,
            container="fixture",
        )
        self.assertEqual((payload["status"], payload["cases"][0]["status"]), ("PASS", "PASS"))
        self.assertEqual(payload["source_revision"], "a" * 40)
        self.assertEqual(payload["prompt_report"]["schema"], "prompt-report-1")

    def test_skip_is_visible_without_changing_report_verdict(self) -> None:
        value = report("AT9-task-start")
        value["steps"][0]["status"] = "skip"
        payload = DRIVER.step_payload(value, step_name="task-start", revision=None, container="fixture")
        self.assertEqual(payload["status"], "PASS")
        self.assertEqual(payload["cases"][0]["status"], "SKIP")

    def test_invalid_report_is_rejected_with_context(self) -> None:
        with self.assertRaisesRegex(DRIVER.DriverError, "missing fields"):
            DRIVER.validate_prompt_report({}, "AT9-task-start")
        with self.assertRaisesRegex(DRIVER.DriverError, "identity mismatch"):
            DRIVER.validate_prompt_report(report("wrong"), "AT9-task-start")
        value = report("AT9-task-start")
        value["steps"][0]["extra"] = True
        with self.assertRaisesRegex(DRIVER.DriverError, "name, status, detail"):
            DRIVER.validate_prompt_report(value, "AT9-task-start")


class DriverTests(unittest.TestCase):
    def test_canonical_testbed_result_is_converted_and_retained(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            testbed = root / "testbed"
            results = root / "results"
            step_dir = root / "step"
            testbed.mkdir()
            results.mkdir()
            calls: list[tuple[list[str], Path | None, float]] = []

            def command(argv: list[str], cwd: Path | None, timeout: float) -> dict[str, object]:
                calls.append((argv, cwd, timeout))
                return {
                    "exit_code": 1,
                    "stdout": f"fixture failed honestly\nreports and logs: {results}\n",
                    "stderr": "",
                    "command": "test.sh",
                }

            converted = {"feature": "colima-hermes-skills", "status": "FAIL", "cases": []}
            with mock.patch.object(DRIVER.colima_skill_report, "run_step", return_value=converted) as run_step:
                payload = DRIVER.run_hermes_skills(testbed, step_dir, command)

            self.assertEqual(calls, [([str(testbed / "test.sh")], testbed, 3600.0)])
            run_step.assert_called_once_with(results, step_dir)
            self.assertEqual(payload["status"], "FAIL")
            self.assertEqual(payload["testbed_execution"]["exit_code"], 1)

    def test_canonical_testbed_requires_a_retained_result_directory(self) -> None:
        def command(_argv: list[str], _cwd: Path | None, _timeout: float) -> dict[str, object]:
            return {"exit_code": 1, "stdout": "fixture failed", "stderr": "root cause", "command": "test.sh"}

        with self.assertRaisesRegex(DRIVER.DriverError, "root cause"):
            DRIVER.run_hermes_skills(Path("testbed"), Path("step"), command)

    def test_one_canonical_testbed_run_then_three_prompts_in_order(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            testbed = root / "testbed"
            testbed.mkdir()
            (testbed / "test.sh").write_text("#!/bin/sh\n", encoding="utf-8")
            run_dir = root / "site/reports/integration/colima/run"
            commands: list[list[str]] = []

            def command(argv: list[str], _cwd: Path | None, _timeout: float) -> dict[str, object]:
                commands.append(argv)
                if argv[:2] == ["git", "rev-parse"]:
                    return {"exit_code": 0, "stdout": "b" * 40 + "\n", "stderr": "", "command": "git"}
                if argv[:2] == ["docker", "cp"]:
                    prompt_id = Path(argv[2]).name.removeprefix("prompt-").removesuffix(".json")
                    test_id = next(item[2] for item in DRIVER.PROMPTS if item[0] == prompt_id)
                    Path(argv[3]).write_text(json.dumps(report(test_id)), encoding="utf-8")
                return {"exit_code": 0, "stdout": "ok", "stderr": "", "command": " ".join(argv)}

            def renderer(path: Path, *, root: Path) -> dict[str, object]:
                statuses = [json.loads(item.read_text())["status"] for item in sorted(path.glob("steps/*/step.json"))]
                return {"status": "PASS" if all(item == "PASS" for item in statuses) else "FAIL", "steps": statuses}

            def skills(_testbed: Path, step_dir: Path, _command: DRIVER.Command) -> dict[str, object]:
                return {
                    "feature": "colima-hermes-skills",
                    "generated_at": "2026-09-14T00:00:00Z",
                    "source_revision": "b" * 40,
                    "container": "fixture",
                    "status": "PASS",
                    "cases": [{"name": "skills", "status": "PASS", "detail": "test.sh"}],
                }

            summary = DRIVER.run_sequence(
                run_dir,
                testbed=testbed,
                root=root,
                run_command=command,
                skill_runner=skills,
                renderer=renderer,
            )
            self.assertEqual(summary, {"status": "PASS", "steps": ["PASS", "PASS", "PASS", "PASS"]})
            prompt_calls = [argv[-1] for argv in commands if argv[:2] == ["docker", "exec"]]
            self.assertEqual(prompt_calls, ["AT9", "AT10", "AT11"])
            self.assertEqual(
                [path.parent.name for path in sorted(run_dir.glob("steps/*/step.json"))],
                ["01-hermes-skills", "02-task-start", "03-assignment", "04-prompt-handoffs"],
            )

    def test_failed_prompt_is_recorded_and_later_prompt_runs(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            testbed = root / "testbed"
            testbed.mkdir()
            run_dir = root / "site/reports/integration/colima/run"

            def command(argv: list[str], _cwd: Path | None, _timeout: float) -> dict[str, object]:
                if argv[:2] == ["git", "rev-parse"]:
                    return {"exit_code": 0, "stdout": "c" * 40, "stderr": "", "command": "git"}
                if argv[:2] == ["docker", "exec"] and argv[-1] == "AT10":
                    return {"exit_code": 1, "stdout": "AT10 failed", "stderr": "", "command": "docker"}
                if argv[:2] == ["docker", "cp"]:
                    prompt_id = Path(argv[2]).name.removeprefix("prompt-").removesuffix(".json")
                    test_id = next(item[2] for item in DRIVER.PROMPTS if item[0] == prompt_id)
                    Path(argv[3]).write_text(json.dumps(report(test_id)), encoding="utf-8")
                return {"exit_code": 0, "stdout": "ok", "stderr": "", "command": " ".join(argv)}

            def renderer(path: Path, *, root: Path) -> dict[str, object]:
                statuses = [json.loads(item.read_text())["status"] for item in sorted(path.glob("steps/*/step.json"))]
                return {"status": "FAIL", "steps": statuses}

            def skills(_testbed: Path, _step_dir: Path, _command: DRIVER.Command) -> dict[str, object]:
                return {
                    "feature": "colima-hermes-skills",
                    "generated_at": "2026-09-14T00:00:00Z",
                    "source_revision": "c" * 40,
                    "container": "fixture",
                    "status": "PASS",
                    "cases": [],
                }

            summary = DRIVER.run_sequence(
                run_dir,
                testbed=testbed,
                root=root,
                run_command=command,
                skill_runner=skills,
                renderer=renderer,
            )
            self.assertEqual(summary["steps"], ["PASS", "PASS", "FAIL", "PASS"])

    def test_fixture_bringup_failure_records_unrun_prompt_steps(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            run_dir = root / "site/reports/integration/colima/run"
            commands: list[list[str]] = []

            def command(argv: list[str], _cwd: Path | None, _timeout: float) -> dict[str, object]:
                commands.append(argv)
                return {"exit_code": 0, "stdout": "d" * 40, "stderr": "", "command": "git"}

            def skills(_testbed: Path, _step_dir: Path, _command: DRIVER.Command) -> dict[str, object]:
                raise DRIVER.FixtureBringupError("CANNOT RUN: run.sh failed")

            def renderer(path: Path, *, root: Path) -> dict[str, object]:
                payloads = [json.loads(item.read_text()) for item in sorted(path.glob("steps/*/step.json"))]
                return {"status": "FAIL", "steps": payloads}

            summary = DRIVER.run_sequence(
                run_dir,
                testbed=root / "testbed",
                root=root,
                run_command=command,
                skill_runner=skills,
                renderer=renderer,
            )

            self.assertEqual([item["status"] for item in summary["steps"]], ["FAIL"] * 4)
            for payload in summary["steps"][1:]:
                self.assertEqual(payload["cases"][0]["detail"], "not run: fixture bringup failed (step 1)")
            self.assertFalse(any(argv[:2] == ["docker", "exec"] for argv in commands))

    def test_driver_called_sources_use_no_database_or_process_shortcuts(self) -> None:
        source = Path(DRIVER.__file__).read_text(encoding="utf-8").lower()
        forbidden = ("sqlite3", "mail.db", "pkill", "herdr pane", "docker exec atm doctor")
        self.assertEqual([token for token in forbidden if token in source], [])
        self.assertIn("/opt/testbed/harness/run-prompts.sh", source)


if __name__ == "__main__":
    unittest.main()
