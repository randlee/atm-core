from __future__ import annotations

import argparse
import contextlib
import io
import importlib.machinery
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPTS = Path(__file__).parents[1]
sys.path.insert(0, str(SCRIPTS))


def load(name: str):
    loader = importlib.machinery.SourceFileLoader(name, str(SCRIPTS / name.replace("_", "-")))
    spec = importlib.util.spec_from_loader(name, loader)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


merge = load("sanity_merge")
history = load("sanity_run_history")
split = load("sanity_split")
findings = load("sanity_create_findings")


def reply(number: int, findings: list[dict] | None = None) -> dict:
    return {
        "success": True,
        "data": {
            "deliverable": number,
            "sanity_bead": "sanity",
            "dev_bead": "dev",
            "commit_checked": "a" * 40,
            "findings": findings or [],
        },
        "error": None,
    }


class SelectedMergeTests(unittest.TestCase):
    manifest = {"run_id": "run", "deliverables_total": 1, "sha": "a" * 40}

    def selected(self, llm, jev, selection):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            llm_results, jev_results = [llm], [jev]
            llm_sha256, jev_sha256 = merge.canonical_sha256(llm_results), merge.canonical_sha256(jev_results)
            (root / "llm.json").write_text(json.dumps({"run_id": "run", "reviewer": "sanity-llm", "reviewer_results": llm_results, "reviewer_results_sha256": llm_sha256}))
            (root / "jev.json").write_text(json.dumps({"run_id": "run", "reviewer": "sanity-jev", "reviewer_results": jev_results, "reviewer_results_sha256": jev_sha256}))
            selection = [{**entry, "llm_sha256": llm_sha256, "jev_sha256": jev_sha256} for entry in selection]
            (root / "selection.json").write_text(json.dumps(selection))
            args = argparse.Namespace(
                llm_vars=root / "llm.json", jev_vars=root / "jev.json", selection=root / "selection.json"
            )
            return merge.selected_results(args, self.manifest, "sanity", "dev")

    def test_selected_uses_whole_jev_envelope_and_keeps_source(self):
        selected, selection, sources = self.selected(
            reply(1), reply(1, [{"kind": "skipped", "file": "a.rs", "line": 1, "issue": "missing"}]),
            [{"deliverable": 1, "llm": "done", "jev": "undone", "selected": "jev", "reason": "JEV has pinned evidence", "rerun": None, "checker_defect": False}],
        )
        self.assertEqual(selected[0]["data"]["findings"][0]["issue"], "missing")
        self.assertEqual(selection[0]["selected"], "jev")
        self.assertEqual(sources, {1: "sanity-jev"})

    def test_split_manifest_always_selects_all_three_reviewers(self):
        argv = ["sanity-split", "--task", "sanity", "--bead", "dev", "--worktree", "/worktree",
                "--branch", "branch", "--commit", "a" * 40, "--base", "b" * 40,
                "--lint-command", "true", "--scratch", "/scratch"]
        output = io.StringIO()
        with mock.patch.object(sys, "argv", argv), \
             mock.patch.object(split, "load_bead", return_value={"id": "dev", "description": "ignored"}), \
             mock.patch.object(split, "parse_deliverables", return_value=[{"number": 1, "text": "Do it."}]), \
             mock.patch.object(split, "pin", return_value=("a" * 40, "b" * 40)), \
             mock.patch.object(split, "changed_files", return_value=[]), \
             mock.patch.object(split, "outside_fence", return_value=[]), \
             mock.patch.object(split, "render", return_value={"deliverable": {"number": 1}}), \
             mock.patch.object(split, "start_lint", return_value={"command": "true"}), \
             contextlib.redirect_stdout(output):
            self.assertEqual(split.main(), 0)
        manifest = json.loads(output.getvalue())
        self.assertEqual(manifest["reviewers"], ["sanity-llm", "sanity-jev", "sanity-selected"])
        self.assertEqual(manifest["operational_reviewer"], "sanity-selected")

        rejected = io.StringIO()
        with mock.patch.object(sys, "argv", [*argv, "--reviewers", "both"]), contextlib.redirect_stderr(rejected):
            self.assertEqual(split.main(), 1)
        self.assertIn("unrecognized arguments: --reviewers both", rejected.getvalue())

    def test_checker_defect_requires_reason(self):
        undone = reply(1, [{"kind": "skipped", "file": "a.rs", "line": 1, "issue": "missing"}])
        with self.assertRaisesRegex(merge.Reject, "requires a reason"):
            self.selected(undone, undone, [{"deliverable": 1, "llm": "undone", "jev": "undone", "selected": "llm", "reason": "", "rerun": None, "checker_defect": True}])

    def test_selected_rejects_invalid_selection_shapes_and_statuses(self):
        done, undone = reply(1), reply(1, [{"kind": "skipped", "file": "a.rs", "line": 1, "issue": "missing"}])
        valid = {"deliverable": 1, "llm": "done", "jev": "undone", "selected": "jev",
                 "reason": "JEV has evidence", "rerun": None, "checker_defect": False}
        rerun_reply = reply(1, [{"kind": "skipped", "file": "rerun.rs", "line": 2, "issue": "rerun missing"}])
        cases = (
            ([], "missing selection"),
            ([valid, valid], "selection must appear once"),
            ([{**valid, "llm": "undone"}], "status does not match reply"),
            ([{**valid, "reason": ""}], "reason required"),
            ([{**valid, "selected": "rerun", "rerun": None}], "rerun must match selected"),
            ([{**valid, "selected": "rerun", "rerun": {"reviewer": "sanity-jev", "context": [], "reply": rerun_reply}}], "nonempty repo-relative context"),
        )
        for selection, message in cases:
            with self.subTest(message=message), self.assertRaisesRegex(merge.Reject, message):
                self.selected(done, undone, selection)

    def test_selected_rejects_tampered_reviewer_results_and_extra_reply_keys(self):
        clean = [reply(1, [{"kind": "skipped", "file": "a.rs", "line": 1, "issue": "missing"}])]
        edited = [reply(1)]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            digest = merge.canonical_sha256(clean)
            (root / "llm.json").write_text(json.dumps({"run_id": "run", "reviewer": "sanity-llm",
                                                        "reviewer_results": edited,
                                                        "reviewer_results_sha256": digest}))
            jev_digest = merge.canonical_sha256(clean)
            (root / "jev.json").write_text(json.dumps({"run_id": "run", "reviewer": "sanity-jev",
                                                        "reviewer_results": clean,
                                                        "reviewer_results_sha256": jev_digest}))
            (root / "selection.json").write_text("[]")
            args = argparse.Namespace(llm_vars=root / "llm.json", jev_vars=root / "jev.json",
                                      selection=root / "selection.json")
            with self.assertRaisesRegex(merge.Reject, "reviewer_results_sha256 does not match"):
                merge.selected_results(args, self.manifest, "sanity", "dev")
        extra = {**reply(1), "unexpected": True}
        with self.assertRaisesRegex(merge.Reject, "not a success or failure envelope"):
            merge.reply_status(extra, 0, self.manifest, "sanity", "dev")

    def test_selected_missing_jev_result_cannot_runs_and_appends_three_rows(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = {
                "run_id": "shared", "reviewers": ["sanity-llm", "sanity-jev", "sanity-selected"],
                "operational_reviewer": "sanity-selected", "deliverables_total": 2,
                "sha": "a" * 40, "branch": "branch", "lint": {"command": "lint"},
            }
            llm, jev = [reply(1), reply(2)], [reply(1)]
            llm_digest, jev_digest = merge.canonical_sha256(llm), merge.canonical_sha256(jev)
            for name, reviewer, results, digest in (("llm.json", "sanity-llm", llm, llm_digest),
                                                     ("jev.json", "sanity-jev", jev, jev_digest)):
                (root / name).write_text(json.dumps({"run_id": "shared", "reviewer": reviewer,
                                                     "reviewer_results": results,
                                                     "reviewer_results_sha256": digest}))
            selection = [
                {"deliverable": number, "llm": "done", "jev": "done", "selected": "llm",
                 "reason": "", "rerun": None, "checker_defect": False,
                 "llm_sha256": llm_digest, "jev_sha256": jev_digest}
                for number in (1, 2)
            ]
            (root / "selection.json").write_text(json.dumps(selection))
            (root / "manifest.json").write_text(json.dumps(manifest))
            now = "1"
            output, errors = io.StringIO(), io.StringIO()
            argv = ["sanity-merge", str(root / "manifest.json"), "sanity", "dev", "d",
                    "--reviewer", "sanity-selected", "--started-at", "0", "--completed-at", now,
                    "--llm-vars", str(root / "llm.json"), "--jev-vars", str(root / "jev.json"),
                    "--selection", str(root / "selection.json")]
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                self.assertEqual(merge.main(argv), 1)
            selected = json.loads(output.getvalue())
            self.assertEqual((selected["verdict"], selected["selection"]), ("CANNOT_RUN", selection))

            log, table = root / "ledger.jsonl", root / "table.json"
            for reviewer, selection_value in (("sanity-llm", None), ("sanity-jev", None),
                                              ("sanity-selected", selected["selection"])):
                record = {
                    "run_id": "shared", "reviewer": reviewer, "commit": "a" * 40,
                    "task": "sanity", "sprint": "d", "phase": "d",
                    "started_at": "2026-01-01T00:00:00Z", "completed_at": "2026-01-01T00:00:01Z",
                    "duration": "1s", "duration_seconds": 1, "pr_number": 1, "iteration": 1,
                    "verdict": "CANNOT_RUN", "final_verdict": "CANNOT_RUN", "findings": None,
                    "error": {"code": "SANITY.RESULT_INVALID", "message": "D2 missing"},
                    "selection": selection_value,
                }
                history.append_record(log, history.render_record(record), table)
            self.assertEqual([json.loads(line)["reviewer"] for line in log.read_text().splitlines()],
                             ["sanity-llm", "sanity-jev", "sanity-selected"])

    def test_non_selected_merge_rejects_selection_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "manifest.json").write_text(json.dumps({
                "run_id": "run", "reviewers": ["sanity-llm"], "operational_reviewer": "sanity-llm",
                "deliverables_total": 1, "sha": "a" * 40, "branch": "branch", "lint": {"command": "lint"},
            }))
            (root / "llm.json").write_text("[]")
            stderr = io.StringIO()
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(stderr):
                result = merge.main(["sanity-merge", str(root / "manifest.json"), "sanity", "dev", "d",
                                     "--reviewer", "sanity-llm", "--started-at", "0", "--completed-at", "1",
                                     "--llm-vars", str(root / "llm.json")])
            self.assertEqual(result, 1)
            self.assertIn("selection inputs are only valid for sanity-selected", stderr.getvalue())

    def test_pick_counts_agreements_and_non_agreement_choices(self):
        rows = history.display_runs([{
            "run_id": "one", "reviewer": "sanity-selected", "sprint": "d", "pr_number": 1,
            "completed_at": "2026-01-01T00:00:00Z", "duration": "1s", "iteration": 1,
            "verdict": "PASS", "findings": 0,
            "selection": [
                {"llm": "done", "jev": "done", "selected": "llm", "checker_defect": False},
                {"llm": "done", "jev": "undone", "selected": "llm", "checker_defect": False},
                {"llm": "done", "jev": "undone", "selected": "rerun", "checker_defect": True},
            ],
        }], 10)
        self.assertEqual(rows[0]["pick"], "=1 L1 R1 D1")

    def test_selected_ledger_record_accepts_selection_list(self):
        record = {
            "run_id": "one", "reviewer": "sanity-selected", "commit": "a" * 40,
            "task": "sanity", "sprint": "d", "phase": "d",
            "started_at": "2026-01-01T00:00:00Z", "completed_at": "2026-01-01T00:00:01Z",
            "duration": "1s", "duration_seconds": 1, "pr_number": 1, "iteration": 1,
            "verdict": "PASS", "final_verdict": "PASS", "findings": 0, "error": None,
            "selection": [{"deliverable": 1}],
        }
        self.assertEqual(history.validate_record(record), record)

    def merged(self, llm, jev, selection):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = {
                "run_id": "run", "reviewers": ["sanity-llm", "sanity-jev", "sanity-selected"],
                "operational_reviewer": "sanity-selected", "deliverables_total": len(llm),
                "sha": "a" * 40, "branch": "branch", "lint": {"command": "lint"},
            }
            llm_sha256, jev_sha256 = merge.canonical_sha256(llm), merge.canonical_sha256(jev)
            selection = [{**entry, "llm_sha256": llm_sha256, "jev_sha256": jev_sha256} for entry in selection]
            llm_vars = {"run_id": "run", "reviewer": "sanity-llm", "reviewer_results": llm,
                        "reviewer_results_sha256": llm_sha256}
            jev_vars = {"run_id": "run", "reviewer": "sanity-jev", "reviewer_results": jev,
                        "reviewer_results_sha256": jev_sha256}
            for name, value in (("manifest.json", manifest), ("llm.json", llm_vars), ("jev.json", jev_vars), ("selection.json", selection)):
                (root / name).write_text(json.dumps(value))
            output = io.StringIO()
            argv = ["sanity-merge", str(root / "manifest.json"), "sanity", "dev", "d",
                    "--reviewer", "sanity-selected", "--started-at", "0", "--completed-at", "1",
                    "--llm-vars", str(root / "llm.json"), "--jev-vars", str(root / "jev.json"),
                    "--selection", str(root / "selection.json")]
            with mock.patch.object(merge, "lint_result", return_value=(0, [], "")), \
                 mock.patch.object(merge, "verify_worktree"), \
                 mock.patch.object(merge, "context_path_exists", return_value=True), \
                 contextlib.redirect_stdout(output):
                self.assertEqual(merge.main(argv), 0)
            return json.loads(output.getvalue())

    def test_selected_rejects_rerun_context_directory_at_manifest_commit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            worktree = root / "worktree"
            worktree.mkdir()
            for command in (("git", "init", "-q", str(worktree)),
                            ("git", "-C", str(worktree), "config", "user.email", "test@example.com"),
                            ("git", "-C", str(worktree), "config", "user.name", "test")):
                subprocess.run(command, check=True)
            (worktree / "present.rs").write_text("present\n")
            (worktree / "context-dir").mkdir()
            (worktree / "context-dir" / "nested.rs").write_text("nested\n")
            subprocess.run(("git", "-C", str(worktree), "add", "present.rs"), check=True)
            subprocess.run(("git", "-C", str(worktree), "commit", "-qm", "pinned"), check=True)
            sha = subprocess.run(("git", "-C", str(worktree), "rev-parse", "HEAD"), check=True,
                                 capture_output=True, text=True).stdout.strip()
            manifest = {"run_id": "run", "deliverables_total": 1, "sha": sha, "worktree_path": str(worktree)}
            llm = reply(1)
            jev = reply(1)
            for value in (llm, jev):
                value["data"]["commit_checked"] = sha
            llm_sha256, jev_sha256 = merge.canonical_sha256([llm]), merge.canonical_sha256([jev])
            (root / "llm.json").write_text(json.dumps({"run_id": "run", "reviewer": "sanity-llm", "reviewer_results": [llm], "reviewer_results_sha256": llm_sha256}))
            (root / "jev.json").write_text(json.dumps({"run_id": "run", "reviewer": "sanity-jev", "reviewer_results": [jev], "reviewer_results_sha256": jev_sha256}))
            (root / "selection.json").write_text(json.dumps([{
                "deliverable": 1, "llm": "done", "jev": "done", "selected": "rerun",
                "reason": "Need the directory", "checker_defect": False,
                "rerun": {"reviewer": "sanity-jev", "context": ["context-dir"], "reply": jev},
                "llm_sha256": llm_sha256, "jev_sha256": jev_sha256,
            }]))
            args = argparse.Namespace(llm_vars=root / "llm.json", jev_vars=root / "jev.json",
                                      selection=root / "selection.json")
            with self.assertRaisesRegex(merge.Reject, "rerun context path is not a file at manifest sha: context-dir"):
                merge.selected_results(args, manifest, "sanity", "dev")

    def test_checker_defect_only_is_pass_without_findings(self):
        undone = reply(1, [{"kind": "skipped", "file": "a.rs", "line": 1, "issue": "missing"}])
        report = self.merged([undone], [undone], [{"deliverable": 1, "llm": "undone", "jev": "undone", "selected": "llm", "reason": "finding is a checker defect", "rerun": None, "checker_defect": True}])
        self.assertEqual((report["verdict"], report["findings_count"], report["findings"]), ("PASS", 0, []))
        self.assertTrue(report["selection"][0]["checker_defect"])

    def test_selected_happy_paths_recompute_verdict_and_finding_provenance(self):
        jev_finding = {"kind": "skipped", "file": "jev.rs", "line": 2, "issue": "JEV missing"}
        rerun_finding = {"kind": "skipped", "file": "rerun.rs", "line": 3, "issue": "rerun missing"}
        report = self.merged(
            [reply(1), reply(2), reply(3)],
            [reply(1), reply(2, [jev_finding]), reply(3)],
            [
                {"deliverable": 1, "llm": "done", "jev": "done", "selected": "llm", "reason": "", "rerun": None, "checker_defect": False},
                {"deliverable": 2, "llm": "done", "jev": "undone", "selected": "jev", "reason": "JEV has evidence", "rerun": None, "checker_defect": False},
                {"deliverable": 3, "llm": "done", "jev": "done", "selected": "rerun", "reason": "rerun received context", "rerun": {"reviewer": "sanity-jev", "context": ["rerun.rs"], "reply": reply(3, [rerun_finding])}, "checker_defect": False},
            ],
        )
        self.assertEqual((report["verdict"], report["findings_count"]), ("FAIL", 2))
        self.assertEqual([(item["issue"], item["reviewer"]) for item in report["findings"]], [
            ("JEV missing", "sc-sanity-jev"), ("rerun missing", "sc-sanity-jev"),
        ])

        pass_report = self.merged(
            [reply(1, [jev_finding])], [reply(1)],
            [{"deliverable": 1, "llm": "undone", "jev": "done", "selected": "jev", "reason": "JEV disproved it", "rerun": None, "checker_defect": False}],
        )
        self.assertEqual((pass_report["verdict"], pass_report["findings_count"], pass_report["findings"]), ("PASS", 0, []))

    def test_selected_child_provenance_uses_explicit_finding_reviewer(self):
        report = {
            "task_id": "sanity", "checked_bead": "checked", "verdict": "FAIL", "run_id": "run",
            "reviewer": "sanity-selected", "operational_reviewer": "sanity-selected", "commit": "a" * 40,
            "findings": [
                {"finding_ref": "D1-F1", "deliverable": 1, "kind": "skipped", "file": "jev.rs", "line": 1,
                 "issue": "JEV finding", "depends_on": [], "deliverable_text": "Implement D1.", "reviewer": "sc-sanity-jev"},
                {"finding_ref": "D2-F1", "deliverable": 2, "kind": "skipped", "file": "llm.rs", "line": 2,
                 "issue": "LLM finding", "depends_on": [], "deliverable_text": "Implement D2.", "reviewer": "sc-sanity-llm"},
            ],
        }
        parent = {"labels": ["stage:sprint"], "priority": 2, "metadata": {
            "phase": "d", "sprint": "d-1", "stack": "stack", "layer": 1, "difficulty": "normal",
        }}
        with tempfile.TemporaryDirectory() as directory:
            vars_path = Path(directory) / "vars.json"
            vars_path.write_text(json.dumps(report))
            created = []

            def command(argv, actor, capture=False):
                if argv[1] == "list":
                    return "[]"
                if argv[1] == "show":
                    return json.dumps([parent])
                if argv[1] == "create":
                    created.append(json.loads(argv[argv.index("--metadata") + 1]))
                    return f"child-{len(created)}"
                if argv[1] == "update":
                    return ""
                raise AssertionError(argv)

            with mock.patch.object(sys, "argv", ["sanity-create-findings", "--task", "sanity", "--bead", "checked",
                                                   "--vars", str(vars_path), "--reviewer", "sc-sanity-selected", "--actor", "a"]), \
                 mock.patch.object(findings, "command", side_effect=command), \
                 contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(findings.main(), 0)
        self.assertEqual([item["reviewer"] for item in created], ["sc-sanity-jev", "sc-sanity-llm"])

    def test_history_rejects_invalid_selection_for_reviewer_kind(self):
        base = {
            "sprint": "d", "run_id": "run", "reviewer": "sanity-llm", "commit": "a" * 40,
            "started_at": "2026-01-01T00:00:00Z", "completed_at": "2026-01-01T00:00:01Z",
            "verdict": "PASS", "findings_count": 0, "error": None,
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            comparison = root / "comparison.json"
            comparison.write_text(json.dumps({**base, "selection": []}))
            selected = root / "selected.json"
            selected.write_text(json.dumps({**base, "reviewer": "sanity-selected", "selection": None}))
            with self.assertRaisesRegex(SystemExit, "comparison reviewers require null selection"):
                history.read_vars(comparison)
            with self.assertRaisesRegex(SystemExit, "sanity-selected requires a selection list"):
                history.read_vars(selected)
            selected.write_text(json.dumps({**base, "reviewer": "sanity-selected", "selection": []}))
            with self.assertRaisesRegex(SystemExit, "empty selection only for CANNOT_RUN"):
                history.read_vars(selected)
            selected.write_text(json.dumps({**base, "reviewer": "sanity-selected", "verdict": "CANNOT_RUN",
                                            "findings_count": None,
                                            "error": {"code": "SANITY.RESULT_INVALID", "message": "selection failed"},
                                            "selection": []}))
            self.assertEqual(history.read_vars(selected)["selection"], [])

    def test_mixed_checker_defect_and_real_finding_only_emits_real_finding(self):
        defective = reply(1, [{"kind": "skipped", "file": "a.rs", "line": 1, "issue": "wrong"}])
        real = reply(2, [{"kind": "skipped", "file": "b.rs", "line": 2, "issue": "missing"}])
        report = self.merged(
            [defective, real], [defective, real],
            [
                {"deliverable": 1, "llm": "undone", "jev": "undone", "selected": "llm", "reason": "wrong finding", "rerun": None, "checker_defect": True},
                {"deliverable": 2, "llm": "undone", "jev": "undone", "selected": "jev", "reason": "", "rerun": None, "checker_defect": False},
            ],
        )
        self.assertEqual((report["verdict"], report["findings_count"]), ("FAIL", 1))
        self.assertEqual(report["findings"][0]["deliverable"], 2)


if __name__ == "__main__":
    unittest.main()
