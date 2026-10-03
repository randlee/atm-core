"""Exercise the installed sc-compose -> typed JSON -> locked append pipeline."""
import concurrent.futures
import importlib.machinery
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).parents[2]
SCRIPTS = ROOT / "scripts"
LOADER = importlib.machinery.SourceFileLoader("sanity_history", str(SCRIPTS / "sanity-run-history"))
HISTORY = importlib.util.module_from_spec(importlib.util.spec_from_loader(LOADER.name, LOADER))
LOADER.exec_module(HISTORY)


def record(**changes):
    value = dict(run_id="run-one", reviewer="sanity-llm", commit="a" * 40,
                 task='task-"quote"\nnext', sprint="d-4", phase="d", started_at="2026-09-30T16:00:00Z",
                 completed_at="2026-09-30T16:01:05Z", duration="1m05s", duration_seconds=65,
                 pr_number=42, iteration=1, verdict="PASS", final_verdict="PASS", findings=0,
                 error=None, selection=None)
    return dict(value, **changes)


class SanityHistory(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.log = self.root / "phase-d.jsonl"
        self.output = self.root / "display.json"

    def test_real_render_compact_append_escaping_types_and_retry(self):
        original = record()
        rendered = HISTORY.render_record(original)
        self.assertEqual(rendered, original)
        HISTORY.append_record(self.log, rendered, self.output)
        HISTORY.append_record(self.log, rendered, self.output)
        lines = self.log.read_text().splitlines()
        self.assertEqual(len(lines), 1)
        self.assertEqual(json.loads(lines[0]), original)
        self.assertNotIn(': ', lines[0])
        self.assertNotIn("completed_local", json.loads(lines[0]))
        self.assertIs(type(rendered["findings"]), int)
        unavailable = record(reviewer="sanity-jev", verdict="CANNOT_RUN", findings=None,
                             error={"code": "JEV.UNAVAILABLE", "message": 'failed "transport"\nretry'})
        HISTORY.append_record(self.log, HISTORY.render_record(unavailable), self.output)
        self.assertEqual(json.loads(self.log.read_text().splitlines()[1]), unavailable)

    def test_real_render_or_validation_failure_leaves_log_unchanged(self):
        HISTORY.append_record(self.log, HISTORY.render_record(record()), self.output)
        before = self.log.read_bytes()
        missing = record()
        del missing["reviewer"]
        missing_final = record()
        del missing_final["final_verdict"]
        for invalid in (missing, missing_final, record(pr_number="42"), record(completed_at="2026-09-30T09:01:05-07:00")):
            with self.subTest(invalid=invalid), self.assertRaises(SystemExit):
                HISTORY.append_record(self.log, HISTORY.render_record(invalid), self.output)
            self.assertEqual(self.log.read_bytes(), before)
        template = self.root / "invalid.json.j2"
        template.write_text('---\nname: invalid\nformat: json\n---\n{broken json')
        with patch.object(HISTORY, "RECORD_TEMPLATE", template), self.assertRaises(SystemExit):
            HISTORY.append_record(self.log, HISTORY.render_record(record()), self.output)
        self.assertEqual(self.log.read_bytes(), before)

    def test_group_limit_retains_pair_and_legacy_is_llm_without_mutation(self):
        legacy = {k: v for k, v in record().items() if k not in {"run_id", "reviewer", "final_verdict"}}
        legacy["completed_local"] = "wrong old local time"
        saved = dict(legacy)
        pair = [record(run_id="new", completed_at="2026-09-30T18:01:05Z"),
                record(run_id="new", reviewer="sanity-jev", completed_at="2026-09-30T18:02:05Z")]
        rows = HISTORY.display_runs([legacy, *pair], 1)
        self.assertEqual([r["reviewer_short"] for r in rows], ["LLM", "JEV"])
        self.assertEqual(len(HISTORY.display_runs([legacy, *pair], 2)), 3)
        self.assertEqual(HISTORY.display_runs([legacy])[0]["reviewer"], "sanity-llm")
        self.assertEqual(legacy, saved)
        self.assertEqual(len(HISTORY.display_runs([*pair, *[record(run_id=str(i)) for i in range(10)]])), 11)
        self.assertEqual(HISTORY.display_runs([legacy])[0]["match"], "—")

    def test_selected_final_verdict_must_match_and_reviewer_match_is_rendered(self):
        selected = record(reviewer="sanity-selected", selection=[{"deliverable": 1}])
        with self.assertRaises(SystemExit):
            HISTORY.validate_record(dict(selected, final_verdict="FAIL"))
        rows = HISTORY.display_runs([
            record(reviewer="sanity-llm"),
            record(reviewer="sanity-jev", verdict="FAIL", findings=1),
            selected,
        ])
        self.assertEqual([row["match"] for row in rows], ["—", "✓", "✗"])
        self.output.write_text(json.dumps({"runs": rows}))
        result = subprocess.run(["sc-compose", "render", "--strict", "--file",
                                 str(SCRIPTS.parent / "templates/sanity-run-table.md.j2"),
                                 "--var-file", str(self.output)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("| Match |", result.stdout)
        self.assertIn("| ✓ |", result.stdout)
        self.assertIn("| ✗ |", result.stdout)

    def test_local_display_is_derived_from_utc_in_requested_timezone(self):
        old = os.environ.get("TZ")
        try:
            os.environ["TZ"] = "America/Los_Angeles"
            time.tzset()
            row = HISTORY.display_runs([record(completed_local="incorrect cached value")])[0]
            self.assertEqual(row["completed_local"], "09-30 09:01")
        finally:
            if old is None:
                os.environ.pop("TZ", None)
            else:
                os.environ["TZ"] = old
            time.tzset()

    def test_single_both_and_unavailable_compact_table_real_render(self):
        for selected in ([record()], [record(reviewer="sanity-jev")],
                         [record(), record(reviewer="sanity-jev", verdict="CANNOT_RUN", findings=None)]):
            with self.subTest(reviewers=[r["reviewer"] for r in selected]):
                self.output.write_text(json.dumps({"runs": HISTORY.display_runs(selected)}))
                result = subprocess.run(["sc-compose", "render", "--strict", "--file",
                                         str(SCRIPTS.parent / "templates/sanity-run-table.md.j2"),
                                         "--var-file", str(self.output)], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("| S | PR | R | Pick | Find | Result | Match | Done | Iter |", result.stdout)
                self.assertEqual(sum(line.startswith("| d-4") for line in result.stdout.splitlines()), len(selected))
                self.assertNotIn("task-", result.stdout)
                if len(selected) == 2:
                    self.assertIn("⚠ unavailable", result.stdout)
                    self.assertIn("| JEV | — |", result.stdout)

    def test_assignment_selection_renders_without_legacy_reviewers(self):
        variables = json.loads((SCRIPTS.parent / "examples/dev-sanity-template-vars.json").read_text())
        self.output.write_text(json.dumps(variables))
        rendered = subprocess.run(["sc-compose", "render", "--strict", "--file",
                                   str(SCRIPTS.parent / "templates/dev-sanity-template.xml.j2"),
                                   "--var-file", str(self.output)], capture_output=True, text=True)
        self.assertEqual(rendered.returncode, 0, rendered.stderr)
        self.assertNotIn("<reviewers>", rendered.stdout)
        self.assertIn("canonical coordinator", rendered.stdout)
        self.assertIn("--llm-vars <sanity-llm-vars.json>", rendered.stdout)
        self.assertIn("send identical assignments to both reviewers concurrently", rendered.stdout)

    def test_conflicting_identity_or_truncated_log_is_not_appended(self):
        HISTORY.append_record(self.log, record(), self.output)
        before = self.log.read_bytes()
        for conflict in (record(commit="b" * 40, reviewer="sanity-jev"), record(findings=1, verdict="FAIL")):
            with self.assertRaises(SystemExit):
                HISTORY.append_record(self.log, conflict, self.output)
            self.assertEqual(self.log.read_bytes(), before)
        self.log.write_bytes(before + b'{"truncated"')
        with self.assertRaises(SystemExit):
            HISTORY.append_record(self.log, record(run_id="new"), self.output)
        self.assertEqual(self.log.read_bytes(), before + b'{"truncated"')

    def test_cli_real_render_concurrent_appends_and_renamed_history(self):
        # Real separate writer processes and installed sc-compose. The only fake
        # external dependency is bd (absent bead -> documented sprint fallback).
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        bd = bin_dir / "bd"
        bd.write_text("#!/bin/sh\nexit 1\n")
        bd.chmod(0o755)
        env = dict(os.environ, PATH=str(bin_dir) + os.pathsep + os.environ["PATH"])
        history_path = self.root / ".sc/sanity-log/sanity-llm.jsonl"
        history_path.parent.mkdir(parents=True)
        legacy = {k: v for k, v in record().items() if k not in {"run_id", "reviewer", "final_verdict"}}
        history_bytes = (json.dumps(legacy) + "\n").encode()
        history_path.write_bytes(history_bytes)

        def run(index):
            values = record(run_id=f"run-{index // 2}", reviewer="sanity-llm" if index % 2 == 0 else "sanity-jev")
            values["findings_count"] = values.pop("findings")
            var_file = self.root / f"vars-{index}.json"
            var_file.write_text(json.dumps(values))
            return subprocess.run([str(SCRIPTS / "sanity-run-history"), "--vars", str(var_file),
                                   "--task", values["task"], "--bead", "dev-d-4", "--pr-number", "42",
                                   "--iteration", "1", "--final-verdict", "PASS",
                                   "--output", str(self.root / f"out-{index}.json")],
                                  cwd=self.root, env=env, capture_output=True, text=True)
        with concurrent.futures.ThreadPoolExecutor(max_workers=6) as executor:
            results = list(executor.map(run, range(12)))
        for result in results:
            self.assertEqual(result.returncode, 0, result.stderr)
        log = self.root / ".sc/sanity-log/phase-d.jsonl"
        rows = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertEqual(len(rows), 12)
        self.assertEqual(len({(r["run_id"], r["reviewer"]) for r in rows}), 12)
        self.assertTrue(all("completed_local" not in r for r in rows))
        self.assertEqual(history_path.read_bytes(), history_bytes)
        result = run(0)  # idempotent retry refreshes the complete display
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(json.loads((self.root / "out-0.json").read_text())["runs"]), 13)
