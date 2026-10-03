"""qa-report (consumer) reads exactly the logs qa-template step j (producer) writes.

Step j is rendered, its two jq programs are executed to produce real rows, and
every path, sort key and column qa-report/SKILL.md names must exist in them.
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

from test_templates import ROOT, _example, _render

QA_REPORT = ROOT.parent / "qa-report" / "SKILL.md"


def _step_j(values: dict) -> str:
    result = _render("qa-template.xml.j2", values)
    if result.returncode:
        raise AssertionError(result.stderr)
    text = result.stdout[result.stdout.index('<step id="j">'):]
    return text[text.index("```"):text.rindex("rmdir")]


def _produce(step: str, workdir: Path) -> tuple[dict, dict]:
    """Run step j's two row writers (bd replaced by an empty finding list) and return one row of each log."""
    round_cmd = step[step.index("jq -nc"):step.index("\n\nbd list")]
    stats_cmd = step[step.index("bd list"):].rstrip()
    stats_cmd = stats_cmd.replace("bd list --all --json 2>/dev/null", "echo '[]'", 1)
    env = {**os.environ, "NOW": "2026-10-02T12:00:00Z", "LOCAL": "05:00", "DURATION": "3m",
           "VERDICT": "PASS", "FND": "0", "BLK": "0", "IMP": "0", "MIN": "0", "TESTED": "x"}
    script = "set -eu\nmkdir -p .sc/qa-log\n" + round_cmd + "\n" + stats_cmd + "\n"
    subprocess.run(["bash", "-c", script], cwd=workdir, env=env, check=True, capture_output=True, text=True)
    logs = workdir / ".sc/qa-log"
    round_rows = (logs / "phase-d.jsonl").read_text().splitlines()
    stats_rows = (logs / "phase-d-stats.jsonl").read_text().splitlines()
    assert len(round_rows) == 1 and len(stats_rows) == 1, (round_rows, stats_rows)
    return json.loads(round_rows[0]), json.loads(stats_rows[0])


def _columns(skill: str, log_label: str) -> list[str]:
    block = re.search(rf"\*\*{log_label}[^*]*\*\*.*?columns(.*?)\.\n", skill, re.S)
    if block is None:
        raise AssertionError(f"qa-report/SKILL.md has no column list for {log_label}")
    return re.findall(r"`([a-z_]+)`", block.group(1))


class QaReportReadsWhatStepJWrites(unittest.TestCase):
    def setUp(self):
        self.skill = QA_REPORT.read_text()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def test_paths_sort_keys_and_columns_exist_in_the_produced_rows(self):
        for example in ("qa-template-vars.json", "qa-template-fix-round-vars.json"):
            with self.subTest(example=example):
                values = _example(example)
                self.assertEqual(values["phase"], "d")
                workdir = Path(self.tmp.name) / example
                workdir.mkdir()
                round_row, stats_row = _produce(_step_j(values), workdir)

                # Paths: qa-report's directory and file patterns name the files step j appends to.
                self.assertIn("`.sc/qa-log/`", self.skill)
                self.assertIn("(`phase-<p>.jsonl`, \"Log B\")", self.skill)
                self.assertIn("(`phase-<p>-stats.jsonl`, \"Log A\")", self.skill)

                # Sort keys.
                self.assertIn("Log A by `snapshot_at`", self.skill)
                self.assertIn("Log B by `completed_at`", self.skill)
                self.assertIn("snapshot_at", stats_row)
                self.assertIn("completed_at", round_row)

                # Columns.
                stats_columns = _columns(self.skill, "Log A")
                round_columns = _columns(self.skill, "Log B")
                self.assertEqual(stats_columns, ["snapshot_local", "tot", "open", "blk", "imp", "min", "trigger_task"])
                self.assertEqual(round_columns, ["completed_local", "task", "sprint", "tested", "iteration", "verdict",
                                                 "blk", "imp", "min", "fnd", "duration", "pr_number"])
                self.assertEqual([c for c in stats_columns if c not in stats_row], [])
                self.assertEqual([c for c in round_columns if c not in round_row], [])

                # The display shortening keys off qa-pr<number> in the task id step j records.
                self.assertEqual(round_row["task"], values["task_id"])
                self.assertEqual(stats_row["trigger_task"], values["task_id"])


if __name__ == "__main__":
    unittest.main()
