"""sanity-merge accepts exactly one result per deliverable, folds in the lint result and re-checks the tree."""
import copy
from functools import partial
import hashlib
import importlib.machinery
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import time
import unittest

from subprocess_helpers import HANG_GUARD_SECONDS, run_command
from jev_receipts import CLIENT, env, receipted, split
from rendering import render

ROOT = Path(__file__).parents[2]
SCRIPT = ROOT / "scripts/sanity-merge"
TEMPLATES = ROOT / "templates"
TASK, DEV, SPRINT, BRANCH = "atm-d-4-sanity", "atm-d-4", "d-4", "sprint/d-4-x"
FINDING = {"kind": "skipped", "file": "crates/x/src/lib.rs", "line": 3, "issue": "no 503 test"}
run_bounded = partial(run_command, timeout=HANG_GUARD_SECONDS)
MERGE_LOADER = importlib.machinery.SourceFileLoader("sanity_merge_test_module", str(SCRIPT))
MERGE_SPEC = importlib.util.spec_from_loader(MERGE_LOADER.name, MERGE_LOADER)
assert MERGE_SPEC is not None
MERGE_MODULE = importlib.util.module_from_spec(MERGE_SPEC)
MERGE_LOADER.exec_module(MERGE_MODULE)


def without(envelope, key):
    return {k: v for k, v in envelope.items() if k != key}


def test_selected_fallback_errors_are_not_counted_twice():
    error = {"code": "SANITY.LLM_API_FAILURE", "message": "unavailable"}
    assert MERGE_MODULE.unique_errors([error, error]) == [error]


def git(cwd, *argv):
    return run_bounded(["git", "-C", str(cwd), *argv], check=True, capture_output=True, text=True).stdout.strip()


def reviewer_vars(run_id, reviewer, results):
    digest = hashlib.sha256(json.dumps(results, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return {"run_id": run_id, "reviewer": reviewer, "reviewer_results": results,
            "reviewer_results_sha256": digest}, digest


class SanityMerge(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        self.wt = self.dir / "wt"
        self.wt.mkdir()
        git(self.wt, "init", "-q", "-b", BRANCH)
        git(self.wt, "config", "user.email", "t@example.com")
        git(self.wt, "config", "user.name", "t")
        (self.wt / "lib.rs").write_text("fn main() {}\n")
        git(self.wt, "add", "-A")
        git(self.wt, "commit", "-q", "-m", "pinned")
        self.sha = git(self.wt, "rev-parse", "HEAD")
        self.log = self.dir / "lint.log"
        self.exit = self.dir / "lint.exit"
        self.manifest = self.dir / "manifest.json"
        self.write_manifest()

    def write_manifest(self, **overrides):
        manifest = {
            "sha": self.sha, "base_sha": "0" * 40, "sanity_bead": TASK, "dev_bead": DEV, "branch": BRANCH,
            "worktree_path": str(self.wt), "deliverables_total": 2, "changed_files": [],
            "files_outside_owned_paths": [],
            "lint": {"command": "just lint", "log": str(self.log), "exit_file": str(self.exit), "pid": 1,
                     "timeout_seconds": 1800},
            "assignments": []}
        manifest.update(overrides)
        self.manifest.write_text(json.dumps(manifest))

    def result(self, number, findings=()):
        return receipted({"success": True, "error": None, "data": {
            "sanity_bead": TASK, "dev_bead": DEV, "deliverable": number, "commit_checked": self.sha,
            "findings": list(findings)}})

    def failure(self, **error):
        base = {"code": "SANITY.TARGET_UNREADABLE", "message": "no such path", "recoverable": False,
                "suggested_action": "re-pin", "deliverable": 2}
        base.update(error)
        return {"success": False, "data": None, "error": {k: v for k, v in base.items() if v is not ...}}

    def lint(self, state, log=""):
        self.log.write_text(log)
        self.exit.write_text(f"{state}\n")

    def merge(self, results, *args):
        argv = args or (str(self.manifest), TASK, DEV, SPRINT)
        body = results if isinstance(results, str) else json.dumps(results)
        return run_bounded([str(SCRIPT), *argv, "--client", str(CLIENT)], input=body, capture_output=True, text=True, env=env())

    def test_paired_reports_share_identity_preserve_conclusions_and_own_timing(self):
        self.write_manifest(run_id="shared-run", reviewers=["sanity-llm", "sanity-jev"],
                            operational_reviewer="sanity-llm")
        self.lint(0)
        reports = []
        for reviewer, start, completed, findings in (("sanity-llm", "1700000000", "1700000003", []),
                                                     ("sanity-jev", "1700000000", "1700000021", [FINDING])):
            results = [self.result(1), self.result(2, findings)]
            out = self.merge(results, str(self.manifest), TASK, DEV, SPRINT,
                             "--reviewer", reviewer, "--started-at", start, "--completed-at", completed)
            self.assertEqual(out.returncode, 0, out.stderr)
            report = json.loads(out.stdout)
            self.assertEqual(report["run_id"], "shared-run")
            self.assertEqual(report["reviewer"], reviewer)
            self.assertEqual(report["commit"], self.sha)
            self.assertEqual(report["operational_reviewer"], "sanity-llm")
            self.assertEqual(report["reviewer_results"], results)
            self.assertEqual(report["reviewer_results_sha256"], hashlib.sha256(
                json.dumps(results, sort_keys=True, separators=(",", ":")).encode()).hexdigest())
            self.assertTrue(report["completed_at"].endswith("Z"))
            reports.append(report)
        self.assertEqual([r["verdict"] for r in reports], ["PASS", "FAIL"])
        self.assertNotEqual(reports[0]["completed_at"], reports[1]["completed_at"])
        self.assertEqual([r["duration_seconds"] for r in reports], [3, 21])
        self.assertEqual([r["llm_elapsed_s"] for r in reports], [3, None])

    def test_jev_envelopes_accept_optional_elapsed_and_preserve_it(self):
        self.write_manifest(run_id="elapsed-run", reviewers=["sanity-jev"], operational_reviewer="sanity-jev")
        self.lint(0)
        results = [{**self.result(1), "elapsed_s": 4.2}, self.result(2)]
        args = (str(self.manifest), TASK, DEV, SPRINT, "--reviewer", "sanity-jev",
                "--started-at", "1700000000", "--completed-at", "1700000001")
        out = self.merge(results, *args)
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(json.loads(out.stdout)["reviewer_results"], results)
        for value in (-1, float("nan"), True, "4.2"):
            bad = [{**self.result(1), "elapsed_s": value}, self.result(2)]
            self.assertNotEqual(self.merge(bad, *args).returncode, 0)

    def test_single_jev_and_cannot_run_are_never_relabelled(self):
        self.write_manifest(run_id="jev-run", reviewers=["sanity-jev"], operational_reviewer="sanity-jev")
        self.lint(0)
        results = [self.result(1), self.failure(code="JEV.UNAVAILABLE")]
        out = self.merge(results, str(self.manifest), TASK, DEV, SPRINT,
                         "--reviewer", "sanity-jev", "--started-at", "1700000000", "--completed-at", "1700000001")
        self.assertEqual(out.returncode, 3, out.stderr)
        report = json.loads(out.stdout)
        self.assertEqual(report["verdict"], "CANNOT_RUN")
        self.assertIsNone(report["findings_count"])
        self.assertEqual(report["reviewer"], "sanity-jev")
        self.assertEqual(report["operational_reviewer"], "sanity-jev")
        self.assertEqual(report["reviewer_results"], results)
        rejected = self.merge([self.result(1), self.result(2)], str(self.manifest), TASK, DEV, SPRINT,
                              "--reviewer", "sanity-llm", "--started-at", "1700000000", "--completed-at", "1700000001")
        self.assertEqual(rejected.returncode, 1)
        self.assertEqual(rejected.stdout, "")

    def test_a_jev_reply_without_a_valid_receipt_becomes_a_result_invalid_failure(self):
        self.write_manifest(run_id="jev-run", reviewers=["sanity-llm", "sanity-jev"], operational_reviewer="sanity-llm")
        self.lint(0)
        good, bare = self.result(1), {**self.result(2), "data": without(self.result(2)["data"], "jev")}
        yes_with_finding = {**self.result(2, [FINDING]), "data": {**self.result(2, [FINDING])["data"], "jev": self.result(2)["data"]["jev"]}}
        tampered = {**self.result(2), "data": {**self.result(2)["data"], "jev": {**self.result(2)["data"]["jev"], "choice": "no"}}}
        reused = {**self.result(2), "data": {**self.result(2)["data"], "jev": good["data"]["jev"]}}
        invented = self.failure(code="SANITY.JEV_UNAVAILABLE", message="Selected model is at capacity")
        args = (str(self.manifest), TASK, DEV, SPRINT, "--reviewer", "sanity-jev", "--started-at", "1700000000", "--completed-at", "1700000001")
        cases = {"no receipt": (bare, "no valid Jev receipt"), "choice contradicts findings": (yes_with_finding, "contradicts"),
                 "tampered receipt": (tampered, "no valid Jev receipt"), "receipt reused": (reused, "reused"),
                 "invented failure message": (invented, "not the client's own text")}
        for name, (reply, reason) in cases.items():
            with self.subTest(name):
                out = self.merge([good, reply], *args)
                self.assertEqual(out.returncode, 3, out.stderr)
                report = json.loads(out.stdout)
                self.assertEqual(report["verdict"], "CANNOT_RUN")
                slot = report["reviewer_results"][1]
                self.assertEqual((slot["success"], slot["error"]["code"], slot["error"]["deliverable"]), (False, "SANITY.RESULT_INVALID", 2))
                self.assertIn(reason, slot["error"]["message"])
                self.assertEqual(report["rejected_results"], [{"deliverable": 2, "reason": slot["error"]["message"], "reply": reply}])
                self.assertEqual(report["reviewer_results"][0], good)
        client_text = self.failure(code="SANITY.JEV_UNAVAILABLE", message="Jev HTTP 529; response body withheld")
        report = json.loads(self.merge([good, client_text], *args).stdout)
        self.assertEqual((report["reviewer_results"][1], report["rejected_results"]), (client_text, []))
        no_key = run_bounded([str(SCRIPT), *args, "--client", str(CLIENT)], input=json.dumps([good, self.result(2)]),
                                capture_output=True, text=True, env={k: v for k, v in env().items() if k != "TYPESAFE_API_KEY"})
        self.assertIn("cannot be verified", json.loads(no_key.stdout)["reviewer_results"][0]["error"]["message"])
        llm = self.merge([{**good, "data": without(good["data"], "jev")}, bare], *args[:4], "--reviewer", "sanity-llm", *args[6:])
        self.assertEqual((llm.returncode, json.loads(llm.stdout)["rejected_results"]), (0, []))

    def test_a_split_reply_stands_on_every_part_receipt_and_the_triaged_final_result(self):
        self.write_manifest(run_id="jev-run", reviewers=["sanity-llm", "sanity-jev"], operational_reviewer="sanity-llm")
        self.lint(0)
        args = (str(self.manifest), TASK, DEV, SPRINT, "--reviewer", "sanity-jev", "--started-at", "1700000000", "--completed-at", "1700000001")
        agreed = split(self.result(1), ["yes", "yes"])
        bare = split({**self.result(2, [FINDING]), "data": without(self.result(2, [FINDING])["data"], "jev")}, ["yes", "no", "yes"])
        labels = ["1 of 3", "2 of 3", "3 of 3"]
        undone = {**bare, "data": {**bare["data"], "jev_final": {
            "choice": "no", "reason": "triage: part 2 shows the hand-off missing; parts 1 and 3 hold unrelated files",
            "parts": labels}}}
        done = {**bare, "data": {**bare["data"], "findings": [], "jev_final": {
            "choice": "yes", "reason": "triage: part 2's no reads a moved test; parts 1 and 3 deliver", "parts": labels}}}
        for name, reply, verdict in (("triaged no", undone, "FAIL"), ("triaged yes", done, "PASS")):
            with self.subTest(name):
                out = self.merge([agreed, reply], *args)
                self.assertEqual(out.returncode, 0, out.stderr)
                report = json.loads(out.stdout)
                self.assertEqual((report["verdict"], report["rejected_results"]), (verdict, []))
                self.assertEqual(report["reviewer_results"], [agreed, reply])

        parts = undone["data"]["jev"]
        def with_parts(new_parts, **data):
            return {**undone, "data": {**undone["data"], "jev": new_parts, **data}}
        forged = dict(parts[1], receipt={**parts[1]["receipt"], "mac": "0" * 64})
        edited = dict(parts[1], receipt={**parts[1]["receipt"], "choice": "yes"})
        response = dict(parts[1], response={**parts[1]["response"], "answers": {"written": {
            **parts[1]["response"]["answers"]["written"], "choice": "yes"}}})
        cases = {
            "forged part receipt": (with_parts([parts[0], forged, parts[2]]), "no valid Jev receipt on part 2 of 3"),
            "edited part receipt": (with_parts([parts[0], edited, parts[2]]), "no valid Jev receipt on part 2 of 3"),
            "edited part response": (with_parts([parts[0], response, parts[2]]), "does not match its receipt"),
            "a part dropped": (with_parts([parts[0], parts[2]]), "part 1 of 2 is missing or mislabelled"),
            "disagreement not triaged": (with_parts(parts, jev_final=None), "parts disagree"),
            "final cites some parts": (with_parts(parts, jev_final={**undone["data"]["jev_final"], "parts": labels[1:]}), "parts disagree"),
            "findings contradict the final": (with_parts(parts, findings=[]), "contradicts the reply's findings"),
            "final contradicts agreeing parts": ({**agreed, "data": {**agreed["data"], "deliverable": 2, "jev_final": {
                "choice": "no", "reason": "r", "parts": ["1 of 2", "2 of 2"]}}}, "contradicts every Jev part"),
        }
        for name, (reply, reason) in cases.items():
            with self.subTest(name):
                out = self.merge([agreed, reply], *args)
                self.assertEqual(out.returncode, 3, out.stderr)
                slot = json.loads(out.stdout)["reviewer_results"][1]
                self.assertEqual((slot["error"]["code"], slot["error"]["deliverable"]), ("SANITY.RESULT_INVALID", 2))
                self.assertIn(reason, slot["error"]["message"])
        reused = split(self.result(2), ["yes", "yes"])
        reused["data"]["jev"] = agreed["data"]["jev"]
        out = self.merge([agreed, reused], *args)
        self.assertIn("reused", json.loads(out.stdout)["reviewer_results"][1]["error"]["message"])

    def test_pass(self):
        self.lint(0, "ok\n")
        out = self.merge([self.result(2), self.result(1)])
        self.assertEqual(out.returncode, 0, out.stderr)
        vars_ = json.loads(out.stdout)
        self.assertEqual((vars_["verdict"], vars_["findings_count"], vars_["commit"], vars_["branch"], vars_["sprint"]),
                         ("PASS", 0, self.sha, BRANCH, SPRINT))
        self.assertEqual(vars_["findings_md"], "D1: done\nD2: done")
        self.assertEqual(vars_["lint_md"], "`just lint` exit 0")
        self.assertRegex(vars_["generated_at"], r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$")
        self.assertIsNone(vars_["selection"])

    def test_fail_from_skipped_finding(self):
        self.lint(0)
        out = self.merge([self.result(1), self.result(2, [FINDING])])
        self.assertEqual(out.returncode, 0, out.stderr)
        vars_ = json.loads(out.stdout)
        self.assertEqual((vars_["verdict"], vars_["findings_count"]), ("FAIL", 1))
        self.assertEqual(vars_["findings_md"], "D1: done\nD2:\n- `crates/x/src/lib.rs:3` skipped: no 503 test")
        self.assertIsNone(vars_["selection"])

    def selected_merge_args(self):
        self.write_manifest(run_id="selected-run",
                            reviewers=["sanity-llm", "sanity-jev", "sanity-selected"],
                            operational_reviewer="sanity-selected")
        llm_path = self.dir / "llm.json"
        jev_path = self.dir / "jev.json"
        selection_path = self.dir / "selection.json"
        llm_vars, llm_sha256 = reviewer_vars("selected-run", "sanity-llm", [self.result(1), self.result(2)])
        jev_vars, jev_sha256 = reviewer_vars("selected-run", "sanity-jev", [self.result(1), self.result(2, [FINDING])])
        llm_path.write_text(json.dumps(llm_vars))
        jev_path.write_text(json.dumps(jev_vars))
        selection_path.write_text(json.dumps([
            {"deliverable": 1, "llm": "done", "jev": "done", "selected": "llm",
             "reason": "", "rerun": None,
             "llm_sha256": llm_sha256, "jev_sha256": jev_sha256},
            {"deliverable": 2, "llm": "done", "jev": "undone", "selected": "jev",
             "reason": "JEV has pinned evidence", "rerun": None,
             "llm_sha256": llm_sha256, "jev_sha256": jev_sha256},
        ]))
        return (str(self.manifest), TASK, DEV, SPRINT,
                "--reviewer", "sanity-selected", "--started-at", "1700000000",
                "--completed-at", "1700000001", "--llm-vars", str(llm_path),
                "--jev-vars", str(jev_path), "--selection", str(selection_path))

    def test_selected_vars_render_the_completion_template(self):
        selected_args = self.selected_merge_args()
        waiting = self.merge([], *selected_args)
        self.assertEqual((waiting.returncode, waiting.stdout), (4, ""))
        self.lint(0)
        out = self.merge([], *selected_args)
        self.assertEqual(out.returncode, 0, out.stderr)
        vars_ = json.loads(out.stdout)
        self.assertEqual(vars_["reviewer"], "sanity-selected")
        self.assertIsInstance(vars_["selection"], list)
        self.assertEqual(vars_["selection"], json.loads((self.dir / "selection.json").read_text()))
        self.render_complete(vars_, "# Dev Sanity Check FAIL")

    def test_selection_rejects_removed_checker_defect_key(self):
        selected_args = self.selected_merge_args()
        self.lint(0)
        selection_path = self.dir / "selection.json"
        selection = json.loads(selection_path.read_text())
        for value in (False, True):
            with self.subTest(value=value):
                selection[1]["checker_defect"] = value
                selection_path.write_text(json.dumps(selection))
                out = self.merge([], *selected_args)
                self.assertEqual(out.returncode, 1, out.stderr)
                self.assertIn("selection entry has incorrect fields", out.stderr)

    def test_selected_undone_reply_keeps_its_findings(self):
        selected_args = self.selected_merge_args()
        self.lint(0)
        out = self.merge([], *selected_args)
        self.assertEqual(out.returncode, 0, out.stderr)
        report = json.loads(out.stdout)
        self.assertEqual((report["verdict"], report["findings_count"]), ("FAIL", 1))
        self.assertEqual(len(report["findings"]), 1)
        self.assertEqual(report["findings"][0]["deliverable"], 2)
        for key, value in FINDING.items():
            self.assertEqual(report["findings"][0][key], value)
        self.assertEqual(report["findings_md"], "D1: done\nD2:\n- `crates/x/src/lib.rs:3` skipped: no 503 test")

    def test_rejects_multiple_or_non_skipped_findings(self):
        self.lint(0)
        multiple = [FINDING, dict(FINDING, file="crates/x/src/other.rs", line=4)]
        for findings, expected in ((multiple, "at most one finding"),
                                   ([dict(FINDING, kind="error")], "kind: skipped")):
            with self.subTest(findings=findings):
                out = self.merge([self.result(1), self.result(2, findings)])
                self.assertEqual(out.returncode, 1, out.stderr)
                self.assertIn(expected, out.stderr)

    def test_lint_failure_parsed_to_file_and_line(self):
        self.lint(101, f"warning: unused variable\n  --> {self.wt}/crates/x/src/lib.rs:9:5\n"
                       f"Diff in {self.wt}/crates/x/src/fmt.rs:12:\nerror: could not compile\n")
        out = self.merge([self.result(1), self.result(2)])
        self.assertEqual(out.returncode, 0, out.stderr)
        vars_ = json.loads(out.stdout)
        self.assertEqual((vars_["verdict"], vars_["findings_count"]), ("FAIL", 2))
        self.assertIn("- `crates/x/src/lib.rs:9` lint:", vars_["lint_md"])
        self.assertIn("- `crates/x/src/fmt.rs:12` lint:", vars_["lint_md"])
        self.assertTrue(vars_["lint_md"].startswith("`just lint` exit 101"))

    def test_lint_failure_without_locations_still_fails(self):
        self.lint(2, "just: recipe failed\n")
        vars_ = json.loads(self.merge([self.result(1), self.result(2)]).stdout)
        self.assertEqual((vars_["verdict"], vars_["findings_count"]), ("FAIL", 1))
        self.assertIn("without locatable diagnostics", vars_["lint_md"])

    def test_rejects(self):
        self.lint(0)
        wrong_sha = copy.deepcopy(self.result(2))
        wrong_sha["data"]["commit_checked"] = "1" * 40
        contradictory = {**self.result(2), "error": {"code": "SANITY.TARGET_UNREADABLE", "recoverable": False}}
        cases = {
            "missing deliverable": ([self.result(1)], "deliverable 2: no result"),
            "duplicate": ([self.result(1), self.result(1)], "deliverable 1: more than one result"),
            "wrong sha": ([self.result(1), wrong_sha], "deliverable 2: commit_checked"),
            "wrong task": ([self.result(1), {**self.result(2), "data": {**self.result(2)["data"], "sanity_bead": "o"}}],
                           "deliverable 2: sanity_bead"),
            "out of range": ([self.result(1), self.result(3)], "result 1: deliverable"),
            "finding without line": ([self.result(1), self.result(2, [dict(FINDING, line=None)])],
                                     "deliverable 2: finding"),
            "success with an error object": ([self.result(1), contradictory], "deliverable 2: a success envelope must have error null"),
            "failure without deliverable": ([self.result(1), self.failure(deliverable=...)], "result 1: failure envelope must name"),
            "failure with out-of-range deliverable": ([self.result(1), self.failure(deliverable=7)], "result 1: failure envelope must name"),
            "failure without message": ([self.result(1), self.failure(message=...)], "result 1: failure envelope must have code"),
            "failure with data": ([self.result(1), {**self.failure(), "data": {}}], "result 1: failure envelope must have data null"),
            "success 0": ([self.result(1), {**self.result(2), "success": 0}], "result 1: not a success or failure envelope"),
            "success 1": ([self.result(1), {**self.result(2), "success": 1}], "result 1: not a success or failure envelope"),
            "success without error key": ([self.result(1), without(self.result(2), "error")], "result 1: not a success or failure envelope"),
            "failure without data key": ([self.result(1), without(self.failure(), "data")], "result 1: not a success or failure envelope"),
            "not a result": (["```json\n\"not a result\"\n```"], "result 0: not a success or failure envelope"),
            "reply text not JSON": (["not a result"], "result 0: reply is not fenced JSON"),
            "not an array": ({"success": True}, "JSON array"),
            "not json": ("PASS", "not JSON"),
        }
        for name, (body, message) in cases.items():
            with self.subTest(name):
                out = self.merge(body)
                self.assertEqual(out.returncode, 1, out.stderr)
                self.assertIn(message, out.stderr)

    def test_failure_envelope_is_classified(self):
        self.lint(0)
        for recoverable, label in ((True, "recoverable"), (False, "fatal")):
            with self.subTest(label):
                out = self.merge([self.result(1), self.failure(recoverable=recoverable)])
                self.assertEqual((out.returncode, out.stdout.strip()), (3, f"SANITY.TARGET_UNREADABLE {label} 2"))

    def test_lint_still_running(self):
        out = self.merge([self.result(1), self.result(2)])
        self.assertEqual((out.returncode, out.stderr.strip()), (4, "lint still running"))

    def test_completed_at_is_required_validated_and_stable_across_exit_four_retry(self):
        self.write_manifest(run_id="timed-run", reviewers=["sanity-llm"], operational_reviewer="sanity-llm")
        results = [self.result(1), self.result(2)]
        prefix = (str(self.manifest), TASK, DEV, SPRINT, "--reviewer", "sanity-llm", "--started-at", "1700000000")
        missing = self.merge(results, *prefix)
        self.assertEqual(missing.returncode, 1)
        self.assertIn("--started-at/--completed-at", missing.stderr)
        backwards = self.merge(results, *prefix, "--completed-at", "1699999999")
        self.assertEqual(backwards.returncode, 1)
        self.assertIn("must not precede", backwards.stderr)
        future = self.merge(results, *prefix, "--completed-at", str(time.time() + 60))
        self.assertEqual(future.returncode, 1)
        self.assertIn("more than five seconds", future.stderr)
        retry_args = (*prefix, "--completed-at", "1700000005")
        first = self.merge(results, *retry_args)
        self.assertEqual((first.returncode, first.stdout), (4, ""))
        self.lint(0)
        retry = self.merge(results, *retry_args)
        self.assertEqual(retry.returncode, 0, retry.stderr)
        report = json.loads(retry.stdout)
        self.assertEqual((report["completed_at"], report["duration_seconds"]), ("2023-11-14T22:13:25Z", 5))
        later = self.merge(results, *prefix, "--completed-at", "1700000012")
        self.assertEqual(later.returncode, 0, later.stderr)
        later_report = json.loads(later.stdout)
        self.assertNotEqual(later_report["completed_at"], report["completed_at"])
        self.assertEqual(later_report["duration_seconds"], 12)

    def test_lint_unavailable(self):
        for state in ("timeout", "cancelled", "error"):
            with self.subTest(state):
                self.lint(state)
                out = self.merge([self.result(1), self.result(2)])
                self.assertEqual((out.returncode, out.stdout.strip()), (3, "SANITY.LINT_UNAVAILABLE fatal 0"), out.stderr)

    def test_worktree_must_still_be_pinned(self):
        self.lint(0)
        with self.subTest("unreadable"):
            self.write_manifest(worktree_path=str(self.dir / "nowhere"))
            out = self.merge([self.result(1), self.result(2)])
            self.assertEqual((out.returncode, out.stdout.strip()), (3, "SANITY.TARGET_UNREADABLE fatal 0"))
        self.write_manifest()
        with self.subTest("tool lock and log files are not dirt"):
            (self.wt / ".beads.gate.lock").write_text("")
            (self.wt / ".sc-compose").mkdir()
            (self.wt / ".sc-compose" / "log.jsonl").write_text("{}")
            out = self.merge([self.result(1), self.result(2)])
            self.assertEqual(out.returncode, 0, out.stderr)
            (self.wt / ".beads.gate.lock").unlink()
            shutil.rmtree(self.wt / ".sc-compose")
        with self.subTest("dirty"):
            (self.wt / "new.rs").write_text("x")
            out = self.merge([self.result(1), self.result(2)])
            self.assertEqual((out.returncode, out.stdout.strip()), (3, "SANITY.COMMIT_MISMATCH fatal 0"))
            (self.wt / "new.rs").unlink()
        with self.subTest("other branch"):
            git(self.wt, "checkout", "-q", "-b", "other")
            out = self.merge([self.result(1), self.result(2)])
            self.assertEqual((out.returncode, out.stdout.strip()), (3, "SANITY.COMMIT_MISMATCH fatal 0"))
            git(self.wt, "checkout", "-q", BRANCH)
        with self.subTest("HEAD moved"):
            (self.wt / "lib.rs").write_text("fn main() { moved() }\n")
            git(self.wt, "commit", "-q", "-am", "moved")
            out = self.merge([self.result(1), self.result(2)])
            self.assertEqual((out.returncode, out.stdout.strip()), (3, "SANITY.COMMIT_MISMATCH fatal 0"))

    def test_split_only_manifest_is_rejected(self):
        self.write_manifest(lint=None)
        out = self.merge([self.result(1), self.result(2)])
        self.assertEqual(out.returncode, 1)
        self.assertIn("--split-only", out.stderr)

    def test_usage(self):
        self.assertEqual(self.merge([], str(self.manifest), TASK).returncode, 1)

    def render_complete(self, vars_, expected_heading):
        out = render(TEMPLATES / "dev-sanity-complete.md.j2", vars_)
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn(expected_heading, out.stdout)
        self.assertIn(f'"verdict": "{vars_["verdict"]}"', out.stdout)
        self.assertIn(vars_["findings_md"].splitlines()[-1], out.stdout)


if __name__ == "__main__":
    unittest.main()
