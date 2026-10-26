"""Exercise the installed sc-compose -> typed JSON -> locked append pipeline."""
import concurrent.futures
from functools import partial
import importlib.machinery
import importlib.util
import json
import os
import shutil
import socket
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import MagicMock, patch

from subprocess_helpers import HANG_GUARD_SECONDS, run_command
from jev_receipts import CLIENT, env as receipt_env, receipted, split
from rendering import render

ROOT = Path(__file__).parents[2]
SCRIPTS = ROOT / "scripts"
LOADER = importlib.machinery.SourceFileLoader("sanity_history", str(SCRIPTS / "sanity-run-history"))
HISTORY_SPEC = importlib.util.spec_from_loader(LOADER.name, LOADER)
assert HISTORY_SPEC is not None
HISTORY = importlib.util.module_from_spec(HISTORY_SPEC)
LOADER.exec_module(HISTORY)
SPLIT_LOADER = importlib.machinery.SourceFileLoader("sanity_split_tests", str(Path(__file__).parent / "test_sanity_split.py"))
SPLIT_SPEC = importlib.util.spec_from_loader(SPLIT_LOADER.name, SPLIT_LOADER)
assert SPLIT_SPEC is not None
SPLIT_TESTS = importlib.util.module_from_spec(SPLIT_SPEC)
SPLIT_LOADER.exec_module(SPLIT_TESTS)
run_bounded = partial(run_command, timeout=HANG_GUARD_SECONDS)


def record(**changes):
    value = dict(run_id="run-one", reviewer="sanity-llm", commit="a" * 40,
                 task='task-"quote"\nnext', sprint="d-4", phase="d", started_at="2026-09-30T16:00:00Z",
                 completed_at="2026-09-30T16:01:05Z", duration="1m05s", duration_seconds=65,
                 pr_number=42, iteration=1, verdict="PASS", final_verdict="PASS", findings=0,
                 error=None, errors=[], jev_receipts=[], completed_local="09-30 09:01",
                 dev=None, jev_elapsed_s=None, llm_elapsed_s=None, kind="verdict")
    return dict(value, **changes)


def jev_reply(number, findings=()):
    return receipted({"success": True, "error": None, "data": {
        "sanity_bead": "s", "dev_bead": "d", "deliverable": number, "commit_checked": "a" * 40, "findings": list(findings)}})


RECEIPT = jev_reply(1)["data"]["jev"]


class SanityHistory(unittest.TestCase):
    def test_result_kind_uses_extensible_reviewer_matchers(self):
        cases = (
            ("sanity-llm", "SANITY.LLM_API_FAILURE", "API_FAILURE"),
            ("sanity-llm", "SANITY.LLM_API_FAILURE_EXTRA", "verdict"),
            ("sanity-jev", "SANITY.JEV_UNAVAILABLE", "API_FAILURE"),
            ("sanity-jev", "SANITY.JEV_REQUEST_TOO_LARGE", "API_FAILURE"),
            ("sanity-llm", "SANITY.JEV_UNAVAILABLE", "verdict"),
            ("sanity-jev", "SANITY.LLM_API_FAILURE", "verdict"),
            ("sanity-jev", None, "verdict"),
            ("other", "SANITY.LLM_API_FAILURE", "verdict"),
        )
        for reviewer, code, expected in cases:
            with self.subTest(reviewer=reviewer, code=code):
                self.assertEqual(HISTORY.result_kind({"reviewer": reviewer, "error": {"code": code}}), expected)
        self.assertEqual(HISTORY.result_kind({"reviewer": "sanity-llm", "error": None}), "verdict")
        matchers = HISTORY.API_FAILURE_MATCHERS["sanity-llm"] + (lambda code: code == "SANITY.LLM_OTHER",)
        with patch.dict(HISTORY.API_FAILURE_MATCHERS, {"sanity-llm": matchers}):
            self.assertEqual(HISTORY.result_kind({"reviewer": "sanity-llm", "error": {"code": "SANITY.LLM_OTHER"}}),
                             "API_FAILURE")

    def test_non_git_cwd_reports_clean_failure(self):
        values = {"sprint": "bj-4", "run_id": "x", "reviewer": "sanity-llm", "commit": "a" * 40,
                  "started_at": "2026-10-02T00:00:00Z", "completed_at": "2026-10-02T00:01:00Z",
                  "verdict": "PASS", "findings_count": 0}
        vars_path = self.root / "vars.json"
        vars_path.write_text(json.dumps(values))
        args = ["sanity-run-history", "--vars", str(vars_path), "--task", "task", "--bead", "bead",
                "--pr-number", "1", "--iteration", "1", "--final-verdict", "PASS"]
        for error in (OSError("git unavailable"), subprocess.CalledProcessError(128, ["git"])):
            with self.subTest(error=error), patch.object(sys, "argv", args), \
                    patch.object(HISTORY, "primary_root", side_effect=error), self.assertRaises(SystemExit) as raised:
                HISTORY.main()
            self.assertIn("sanity-run-history: cannot locate sanity ledger", str(raised.exception))

    def test_shared_elapsed_validator_and_embedded_bead_data(self):
        for value in (0, 1.25):
            self.assertTrue(HISTORY.valid_elapsed_s(value))
        for value in (True, -1, float("nan"), float("inf"), "1"):
            self.assertFalse(HISTORY.valid_elapsed_s(value))
        bead = {"assignee": "dev-one", "metadata": {"phase": "phase-bj"}}
        self.assertEqual(HISTORY.checked_dev(bead), "dev-one")
        self.assertEqual(HISTORY.phase_from_bead(bead, "bj-4"), "bj")

    def test_llm_elapsed_is_derived_and_manifest_bead_avoids_lookup(self):
        values = {"sprint": "bj-4", "run_id": "x", "reviewer": "sanity-llm", "commit": "a" * 40,
                  "started_at": "2026-10-02T00:00:00Z", "completed_at": "2026-10-02T00:01:05Z",
                  "verdict": "PASS", "findings_count": 0, "llm_elapsed_s": 999,
                  "checked_bead_data": {"assignee": "dev-one", "metadata": {"phase": "bj"}}}
        vars_path = self.root / "elapsed-vars.json"
        vars_path.write_text(json.dumps(values))
        args = ["sanity-run-history", "--vars", str(vars_path), "--task", "task", "--bead", "bead",
                "--pr-number", "1", "--iteration", "1", "--final-verdict", "PASS"]
        with patch.object(sys, "argv", args), \
                patch.object(HISTORY, "primary_root", return_value=self.root), \
                patch.object(HISTORY, "append_record") as append:
            HISTORY.main()
        row = append.call_args.args[1]
        self.assertEqual(row["llm_elapsed_s"], 65)
        self.assertEqual(row["dev"], "dev-one")

    def test_jev_elapsed_uses_maximum_and_missing_is_null(self):
        self.assertEqual(HISTORY.reviewer_elapsed({"reviewer": "sanity-jev", "reviewer_results": [
            {"elapsed_s": 1.2}, {"elapsed_s": 4.2}, {"elapsed_s": 2.0}]}), 4.2)
        self.assertIsNone(HISTORY.reviewer_elapsed({"reviewer": "sanity-jev", "reviewer_results": [{}, {}]}))

    def test_pre_new_fields_row_is_idempotent(self):
        old = {k: v for k, v in record().items() if k not in ("dev", "jev_elapsed_s", "llm_elapsed_s", "kind")}
        self.log.write_text(json.dumps(old) + "\n")
        HISTORY.append_record(self.log, record(dev="alice", llm_elapsed_s=4.2))
        self.assertEqual(json.loads(self.log.read_text()), old)

    def test_new_fields_round_trip_and_conflicting_retry_fails(self):
        row = record(dev="alice", llm_elapsed_s=1.2, jev_elapsed_s=4.2, kind="API_FAILURE", verdict="CANNOT_RUN", findings=None,
                     error={"code": "SANITY.JEV_UNAVAILABLE", "message": "unavailable"})
        rendered = HISTORY.render_record(row)
        self.assertEqual({key: rendered[key] for key in ("dev", "jev_elapsed_s", "llm_elapsed_s", "kind", "error")},
                         {key: row[key] for key in ("dev", "jev_elapsed_s", "llm_elapsed_s", "kind", "error")})
        HISTORY.append_record(self.log, rendered)
        HISTORY.append_record(self.log, rendered)
        with self.assertRaises(SystemExit):
            HISTORY.append_record(self.log, {**rendered, "dev": "bob"})
        self.assertEqual([json.loads(line) for line in self.log.read_text().splitlines()], [row])

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.log = self.root / "phase-d.jsonl"

    def test_real_render_compact_append_escaping_types_and_retry(self):
        original = record()
        rendered = HISTORY.render_record(original)
        self.assertEqual(rendered, original)
        HISTORY.append_record(self.log, rendered)
        HISTORY.append_record(self.log, rendered)
        lines = self.log.read_text().splitlines()
        self.assertEqual(len(lines), 1)
        self.assertEqual(json.loads(lines[0]), original)
        self.assertNotIn(': ', lines[0])
        self.assertIs(type(rendered["findings"]), int)
        unavailable = record(reviewer="sanity-jev", verdict="CANNOT_RUN", findings=None,
                             error={"code": "JEV.UNAVAILABLE", "message": 'failed "transport"\nretry'})
        HISTORY.append_record(self.log, HISTORY.render_record(unavailable))
        self.assertEqual(json.loads(self.log.read_text().splitlines()[1]), unavailable)

    def test_real_render_or_validation_failure_leaves_log_unchanged(self):
        HISTORY.append_record(self.log, HISTORY.render_record(record()))
        before = self.log.read_bytes()
        missing = record()
        del missing["reviewer"]
        missing_final = record()
        del missing_final["final_verdict"]
        for invalid in (missing, missing_final, record(pr_number="42"), record(completed_at="2026-09-30T09:01:05-07:00")):
            with self.subTest(invalid=invalid), self.assertRaises(SystemExit):
                HISTORY.append_record(self.log, HISTORY.render_record(invalid))
            self.assertEqual(self.log.read_bytes(), before)
        template = self.root / "invalid.json.j2"
        template.write_text('---\nname: invalid\nformat: json\n---\n{broken json')
        with patch.object(HISTORY, "RECORD_TEMPLATE", template), self.assertRaises(SystemExit):
            HISTORY.append_record(self.log, HISTORY.render_record(record()))
        self.assertEqual(self.log.read_bytes(), before)

    def test_assignment_selection_renders_without_legacy_reviewers(self):
        variables = json.loads((SCRIPTS.parent / "examples/dev-sanity-template-vars.json").read_text())
        variables["sanity_close_mode"] = "legacy"  # the split/merge coordinator path; daemon mode has its own step b
        rendered = render(SCRIPTS.parent / "templates/dev-sanity-template.xml.j2", variables)
        self.assertEqual(rendered.returncode, 0, rendered.stderr)
        self.assertNotIn("<reviewers>", rendered.stdout)
        self.assertIn("canonical coordinator", rendered.stdout)
        self.assertIn("--llm-vars <sanity-llm-vars.json>", rendered.stdout)
        self.assertIn("send identical assignments to both reviewers concurrently", rendered.stdout)

    def test_conflicting_identity_or_truncated_log_is_not_appended(self):
        HISTORY.append_record(self.log, record())
        before = self.log.read_bytes()
        for conflict in (record(commit="b" * 40, reviewer="sanity-jev", jev_receipts=[RECEIPT]), record(findings=1, verdict="FAIL")):
            with self.assertRaises(SystemExit):
                HISTORY.append_record(self.log, conflict)
            self.assertEqual(self.log.read_bytes(), before)
        self.log.write_bytes(before + b'{"truncated"')
        with self.assertRaises(SystemExit):
            HISTORY.append_record(self.log, record(run_id="new"))
        self.assertEqual(self.log.read_bytes(), before + b'{"truncated"')

    def test_a_log_without_a_final_newline_is_refused_with_the_fix(self):
        HISTORY.append_record(self.log, record())
        before = self.log.read_bytes().rstrip(b"\n")
        self.log.write_bytes(before)
        with self.assertRaises(SystemExit) as caught:
            HISTORY.append_record(self.log, record(run_id="new"))
        self.assertIn("append the newline", str(caught.exception))
        self.assertIn("ERR_RENDER_APPEND_NO_FINAL_NEWLINE", str(caught.exception))
        self.assertEqual(self.log.read_bytes(), before)
        with self.log.open("ab") as log:
            log.write(b"\n")
        HISTORY.append_record(self.log, record(run_id="new"))
        self.assertEqual([json.loads(line)["run_id"] for line in self.log.read_text().splitlines()], ["run-one", "new"])

    def partial_compose(self):
        fake = self.root / "fake_sc_compose.py"
        fake.write_text("""import json, os, socket, sys
fd = int(sys.argv[1])
args = sys.argv[2:]
record = json.load(open(args[args.index('--var-file') + 1]))
path = args[args.index('--append') + 1]
data = json.dumps(record, separators=(',', ':')) + '\\n'
with socket.socket(fileno=fd) as control, open(path, 'a') as output:
    midpoint = len(data) // 2
    output.write(data[:midpoint])
    output.flush()
    control.sendall(b'P')
    if control.recv(1) != b'G':
        raise RuntimeError('test did not release append')
    output.write(data[midpoint:])
""")
        return fake

    def test_partial_append_blocks_reader_and_duplicate_retry_is_noop(self):
        reader_attempted = threading.Event()
        reader_thread = threading.local()
        real_flock = HISTORY.fcntl.flock
        real_run = subprocess.run
        calls = []
        fake = self.partial_compose()

        def observed_flock(fd, operation):
            if getattr(reader_thread, "is_reader", False):
                reader_attempted.set()
            return real_flock(fd, operation)

        def fake_compose(argv, **kwargs):
            calls.append(argv)
            return real_run([sys.executable, str(fake), str(child.fileno()), *argv[1:]],
                            pass_fds=(child.fileno(),), **kwargs)

        def retry():
            reader_thread.is_reader = True
            HISTORY.append_record(self.log, record())

        parent, child = socket.socketpair()
        with parent, child:
            parent.settimeout(HANG_GUARD_SECONDS)
            executor = concurrent.futures.ThreadPoolExecutor(max_workers=2)
            try:
                with patch.object(HISTORY.fcntl, "flock", side_effect=observed_flock), \
                     patch.object(HISTORY.subprocess, "run", side_effect=fake_compose):
                    writer = executor.submit(HISTORY.append_record, self.log, record())
                    try:
                        self.assertEqual(parent.recv(1), b"P", "writer never reached partial append")
                        with self.assertRaises(json.JSONDecodeError):
                            json.loads(self.log.read_text())
                        reader = executor.submit(retry)
                        self.assertTrue(reader_attempted.wait(HANG_GUARD_SECONDS), "retry never attempted the ledger lock")
                    finally:
                        parent.sendall(b"G")
                    writer.result(timeout=HANG_GUARD_SECONDS)
                    reader.result(timeout=HANG_GUARD_SECONDS)
            finally:
                executor.shutdown(wait=False, cancel_futures=True)
        self.assertEqual(len(calls), 1)
        self.assertEqual([json.loads(line) for line in self.log.read_text().splitlines()], [record()])

    def test_append_timeout_is_typed_and_cleans_up_vars(self):
        with patch.object(HISTORY.subprocess, "run", side_effect=subprocess.TimeoutExpired("sc-compose", 60)) as run:
            with self.assertRaisesRegex(SystemExit, "SANITY.LOG_APPEND_TIMEOUT.*sc-compose"):
                HISTORY.append_record(self.log, record())
        argv = run.call_args.args[0]
        self.assertEqual(run.call_args.kwargs["timeout"], HISTORY.APPEND_TIMEOUT_SECONDS)
        self.assertFalse(Path(argv[argv.index("--var-file") + 1]).exists())
        HISTORY.append_record(self.log, record())
        self.assertEqual(json.loads(self.log.read_text()), record())

    def test_timeout_rolls_back_partial_append_and_allows_retry(self):
        fake = self.partial_compose()
        for existing in (False, True):
            with self.subTest(existing=existing):
                self.log.unlink(missing_ok=True)
                if existing:
                    HISTORY.append_record(self.log, record())
                before = self.log.read_bytes() if existing else b""
                next_record = record(run_id="next")
                parent, child = socket.socketpair()
                calls = []
                with parent, child:
                    parent.settimeout(HANG_GUARD_SECONDS)

                    def timeout_after_partial(argv, **kwargs):
                        calls.append((argv, kwargs))
                        proc = subprocess.Popen([sys.executable, str(fake), str(child.fileno()), *argv[1:]],
                                                pass_fds=(child.fileno(),), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                        try:
                            self.assertEqual(parent.recv(1), b"P")
                            self.assertTrue(self.log.read_bytes().startswith(before))
                            self.assertGreater(len(self.log.read_bytes()), len(before))
                        finally:
                            proc.kill()
                            proc.communicate(timeout=HANG_GUARD_SECONDS)
                        raise subprocess.TimeoutExpired(argv, kwargs["timeout"])

                    with patch.object(HISTORY.subprocess, "run", side_effect=timeout_after_partial), \
                            self.assertRaisesRegex(SystemExit, "SANITY.LOG_APPEND_TIMEOUT.*sc-compose"):
                        HISTORY.append_record(self.log, next_record)
                argv, kwargs = calls[0]
                self.assertEqual(kwargs["timeout"], HISTORY.APPEND_TIMEOUT_SECONDS)
                self.assertFalse(Path(argv[argv.index("--var-file") + 1]).exists())
                self.assertEqual(self.log.read_bytes(), before)
                HISTORY.append_record(self.log, next_record)
                self.assertEqual([json.loads(line) for line in self.log.read_text().splitlines()],
                                 ([record()] if existing else []) + [next_record])

    def test_retry_under_another_time_zone_is_the_same_row(self):
        HISTORY.append_record(self.log, record())
        HISTORY.append_record(self.log, record(completed_local="09-30 18:01"))
        self.assertEqual(len(self.log.read_text().splitlines()), 1)

    def test_retry_of_a_row_logged_before_errors_existed_is_the_same_row(self):
        failed = record(errors=[{"code": "SANITY.JEV_UNAVAILABLE", "message": "Jev retry budget exhausted",
                                 "recoverable": True, "deliverable": 1}])
        legacy = {key: value for key, value in failed.items() if key != "errors"}
        self.log.write_text(json.dumps(legacy) + "\n")
        HISTORY.append_record(self.log, HISTORY.render_record(failed))
        self.assertEqual([json.loads(line) for line in self.log.read_text().splitlines()], [legacy])
        with self.assertRaises(SystemExit):
            HISTORY.append_record(self.log, record(errors=failed["errors"], findings=1, verdict="FAIL"))

    def test_jev_receipts_are_required_on_a_jev_verdict_and_absent_on_llm(self):
        vars = {"reviewer": "sanity-jev", "reviewer_results": [jev_reply(1), jev_reply(2, [{"kind": "skipped"}]),
                                                               {"success": False, "data": None, "error": {}}]}
        self.assertEqual(HISTORY.jev_receipts(vars), [jev_reply(1)["data"]["jev"], jev_reply(2, [{"kind": "skipped"}])["data"]["jev"]])
        self.assertEqual(HISTORY.jev_receipts(dict(vars, reviewer="sanity-llm")), [])
        parted = split(jev_reply(3), ["yes", "no"])
        self.assertEqual(HISTORY.jev_receipts({"reviewer": "sanity-jev", "reviewer_results": [jev_reply(1), parted]}),
                         [RECEIPT] + [part["receipt"] for part in parted["data"]["jev"]])  # every part's receipt
        jev = record(reviewer="sanity-jev", jev_receipts=[RECEIPT])
        self.assertEqual(HISTORY.render_record(jev), jev)
        for invalid in (record(reviewer="sanity-jev"), record(reviewer="sanity-jev", verdict="FAIL", findings=1),
                        record(jev_receipts=[RECEIPT]), record(reviewer="sanity-jev", jev_receipts=[None])):
            with self.subTest(invalid=invalid), self.assertRaises(SystemExit):
                HISTORY.render_record(invalid)
        unavailable = record(reviewer="sanity-jev", verdict="CANNOT_RUN", findings=None,
                             error={"code": "SANITY.JEV_UNAVAILABLE", "message": "Jev retry budget exhausted"})
        self.assertEqual(HISTORY.render_record(unavailable), unavailable)

    def test_retry_of_a_row_logged_before_jev_receipts_existed_is_the_same_row(self):
        jev = record(reviewer="sanity-jev", jev_receipts=[RECEIPT])
        legacy = {key: value for key, value in jev.items() if key not in ("jev_receipts", "errors")}
        self.log.write_text(json.dumps(legacy) + "\n")
        HISTORY.append_record(self.log, jev)
        self.assertEqual([json.loads(line) for line in self.log.read_text().splitlines()], [legacy])

    def test_malformed_failure_envelopes_never_block_the_cannot_run_row(self):
        error = {"code": "SANITY.RESULT_INVALID", "message": "reply had no fence", "recoverable": False}
        vars = {"reviewer_results": [
            {"success": False, "data": None, "error": error},
            {"success": False, "data": None, "error": {"message": "no code"}},
            {"success": False, "data": None, "error": dict(error, deliverable=2)}]}
        errors = HISTORY.slot_errors(vars)
        self.assertEqual(errors, [dict(error, deliverable=None), dict(error, deliverable=2)])
        row = record(reviewer="sanity-jev", verdict="CANNOT_RUN", final_verdict="PASS", findings=None,
                     error={"code": "SANITY.RESULT_INVALID", "message": "deliverable 1 envelope invalid"}, errors=errors)
        HISTORY.append_record(self.log, HISTORY.render_record(row))
        self.assertEqual(json.loads(self.log.read_text()), row)


def fenced(task, bead, number, sha, findings=()):
    return "```json\n" + json.dumps(receipted({"success": True, "data": {
        "deliverable": number, "sanity_bead": task, "dev_bead": bead, "commit_checked": sha,
        "findings": list(findings)}, "error": None}), indent=2) + "\n```"


class ShippedFlow(unittest.TestCase):
    """Fenced replies -> sanity-split manifest -> sanity-merge (LLM, JEV, selected) ->
    sanity-run-history, all shipped scripts on a real pushed repo."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        real_run = subprocess.run

        # The shared Repo helper also spawns git commands during construction.
        with patch.object(SPLIT_TESTS.subprocess, "run", side_effect=partial(run_bounded, runner=real_run)):
            self.repo = SPLIT_TESTS.Repo(self.root)
        with (self.repo.wt / ".git/info/exclude").open("a") as exclude:
            exclude.write(".sc/\n")
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        (bin_dir / "bd").write_text("#!/bin/sh\nexit 1\n")  # absent bead -> phase from the sprint
        (bin_dir / "bd").chmod(0o755)
        self.env = dict(os.environ, PATH=str(bin_dir) + os.pathsep + os.environ["PATH"])
        self.primary = self.root / "primary"  # where the console command runs: .claude/skills/... resolves
        shutil.copytree(ROOT / "templates", self.primary / ".claude/skills/atm-bd-orchestration/templates")
        bead = SPLIT_TESTS.bead()
        self.bead, self.task = bead[0]["id"], "t-1-sanity"
        (self.root / "bead.json").write_text(json.dumps(bead))
        split = run_bounded([str(SCRIPTS / "sanity-split"), "--task", self.task, "--bead", self.bead,
                                "--worktree", str(self.repo.wt), "--branch", "sprint/x", "--commit", self.repo.sha,
                                "--base", "develop", "--lint-command", "true", "--split-only", "--scratch", str(self.root / "scratch"),
                                "--bead-json", str(self.root / "bead.json")], capture_output=True, text=True)
        self.assertEqual(split.returncode, 0, split.stderr)
        self.manifest = self.root / "manifest.json"
        # The separate lint-supervisor tests cover process execution; this flow
        # drives the merge with a controlled completed lint result.
        manifest = json.loads(split.stdout)
        exit_file = self.root / "lint.exit"
        exit_file.write_text("0\n")
        manifest["lint"] = {"command": "true", "exit_file": str(exit_file), "log": str(self.root / "lint.log")}
        self.manifest.write_text(json.dumps(manifest))

    def merge(self, reviewer, *extra, stdin=None):
        now = time.time()
        return run_bounded([str(SCRIPTS / "sanity-merge"), str(self.manifest), self.task, self.bead, "d-4",
                               "--reviewer", reviewer, "--started-at", str(now - 60), "--completed-at", str(now),
                               *extra, "--client", str(CLIENT)], input=stdin, capture_output=True, text=True, env=receipt_env())

    def run_flow(self, select, jev_failure=None, llm_failure=None):
        sha = self.repo.sha
        skipped = {"kind": "skipped", "file": "crates/types/src/retry.rs", "line": 1, "issue": "404 test not written"}
        replies = {"sanity-llm": [fenced(self.task, self.bead, 1, sha), llm_failure or fenced(self.task, self.bead, 2, sha)],
                   "sanity-jev": [fenced(self.task, self.bead, 1, sha),
                                  jev_failure or fenced(self.task, self.bead, 2, sha, [skipped])]}
        reviewer_vars = {}
        for reviewer, texts in replies.items():
            merged = self.merge(reviewer, stdin=json.dumps(texts))  # reply text kept unchanged as strings
            failure = jev_failure if reviewer == "sanity-jev" else llm_failure
            self.assertEqual(merged.returncode, 3 if failure else 0, merged.stderr)
            reviewer_vars[reviewer] = self.root / f"{reviewer}-vars.json"
            reviewer_vars[reviewer].write_text(merged.stdout)
        digests = {r: json.loads(v.read_text())["reviewer_results_sha256"] for r, v in reviewer_vars.items()}
        selection = [{"deliverable": n, "llm": "done", "jev": "done" if n == 1 else "undone", "selected": "llm",
                      "reason": "" if n == 1 else "retry.rs tests cover 404", "rerun": None,
                      "llm_sha256": digests["sanity-llm"], "jev_sha256": digests["sanity-jev"]} for n in (1, 2)]
        select(selection)
        (self.root / "selection.json").write_text(json.dumps(selection))
        selected = self.merge("sanity-selected", "--llm-vars", str(reviewer_vars["sanity-llm"]),
                              "--jev-vars", str(reviewer_vars["sanity-jev"]),
                              "--selection", str(self.root / "selection.json"))
        final = json.loads(selected.stdout)["verdict"]
        logs = set()
        for reviewer in ("sanity-llm", "sanity-jev"):
            history = run_bounded([str(SCRIPTS / "sanity-run-history"), "--vars", str(reviewer_vars[reviewer]),
                                      "--task", self.task, "--bead", self.bead, "--pr-number", "42",
                                      "--iteration", "1", "--final-verdict", final],
                                     cwd=self.repo.wt, env=self.env, capture_output=True, text=True)
            self.assertEqual(history.returncode, 0, history.stderr)
            logs.add(history.stdout.strip())
        self.assertEqual(len(logs), 1)
        log = logs.pop()
        self.assertEqual(Path(log), self.repo.wt.resolve() / ".sc/sanity-log/phase-d.jsonl")
        rows = [json.loads(line) for line in Path(log).read_text().splitlines()]
        return final, rows, log

    def test_selected_verdict_differs_from_one_reviewer(self):
        final, rows, _ = self.run_flow(lambda selection: None)
        self.assertEqual(final, "PASS")
        self.assertEqual([(r["reviewer"], r["verdict"], r["final_verdict"]) for r in rows],
                         [("sanity-llm", "PASS", "PASS"), ("sanity-jev", "FAIL", "PASS")])

    def test_selected_merge_cannot_run_keeps_reviewer_verdicts(self):
        def invalid(selection):
            selection[1]["reason"] = ""  # a disagreement without a reason: selected merge rejects it
        final, rows, _ = self.run_flow(invalid)
        self.assertEqual(final, "CANNOT_RUN")
        self.assertEqual([(r["reviewer"], r["verdict"], r["final_verdict"]) for r in rows],
                         [("sanity-llm", "PASS", "CANNOT_RUN"), ("sanity-jev", "FAIL", "CANNOT_RUN")])

    def test_failed_jev_slot_error_is_logged_verbatim_while_llm_is_selected(self):
        error = {"code": "SANITY.JEV_UNAVAILABLE", "message": "TYPESAFE_API_KEY is missing; no Jev evaluation ran",
                 "recoverable": True, "suggested_action": "set TYPESAFE_API_KEY", "deliverable": 2}
        failure = {"success": False, "data": None, "error": error}

        def take_llm(selection):
            selection[1].update(jev="cannot_run", reason="JEV unavailable: SANITY.JEV_UNAVAILABLE")
        final, rows, _ = self.run_flow(take_llm, jev_failure=failure)
        self.assertEqual(final, "PASS")
        self.assertEqual([(r["reviewer"], r["verdict"], r["errors"]) for r in rows], [
            ("sanity-llm", "PASS", []),
            ("sanity-jev", "CANNOT_RUN", [{k: error[k] for k in ("code", "message", "recoverable", "deliverable")}])])
        self.assertEqual(rows[1]["kind"], "API_FAILURE")
        self.assertEqual(rows[1]["error"]["message"], error["message"])

    def test_exhausted_llm_api_failure_has_its_own_kind(self):
        error = {"code": "SANITY.LLM_API_FAILURE", "message": "upstream API error: busy",
                 "recoverable": True, "suggested_action": "retry later", "deliverable": 2}
        failure = {"success": False, "data": None, "error": error}

        def take_jev(selection):
            selection[1].update(llm="cannot_run", selected="jev", reason=error["message"])

        _, rows, _ = self.run_flow(take_jev, llm_failure=failure)
        self.assertEqual(rows[0]["kind"], "API_FAILURE")
        self.assertEqual(rows[0]["error"]["message"], error["message"])
        self.assertEqual(rows[0]["errors"][0]["message"], error["message"])

    def test_oversized_jev_failure_reaches_ledger_with_reason(self):
        spec = importlib.util.spec_from_file_location("jev_client_for_history", CLIENT)
        assert spec is not None and spec.loader is not None
        client = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(client)
        request = client.written_request("delivery", "x" * 50000, part=("1 of 1", ["x"]))
        connection = MagicMock()
        connection.getresponse.return_value.status = 413
        connection.getresponse.return_value.read.return_value = b"{}"
        with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
             patch.object(client.http.client, "HTTPSConnection", return_value=connection), \
             self.assertRaises(client.JevError) as raised:
            client.evaluate_parts([request], "test-only-key")
        self.assertEqual(connection.request.call_count, 4)
        failure = client.failure(raised.exception)
        failure["error"]["deliverable"] = 2

        def take_llm(selection):
            selection[1].update(jev="cannot_run", reason="JEV request oversized")

        final, rows, _ = self.run_flow(take_llm, jev_failure=failure)
        self.assertEqual(final, "PASS")
        row = rows[1]
        self.assertEqual((row["reviewer"], row["verdict"], row["kind"]),
                         ("sanity-jev", "CANNOT_RUN", "API_FAILURE"))
        self.assertEqual(row["error"]["code"], "SANITY.JEV_REQUEST_TOO_LARGE")
        self.assertIn("oversized", row["error"]["message"])
        self.assertEqual(row["errors"][0]["message"], failure["error"]["message"])

    def test_all_ok_rows_log_no_errors(self):
        _, rows, _ = self.run_flow(lambda selection: None)
        self.assertEqual([r["errors"] for r in rows], [[], []])
        jev = [json.loads(text.split("\n", 1)[1].rsplit("\n", 1)[0])["data"]["jev"] for text in (
            fenced(self.task, self.bead, 1, self.repo.sha),
            fenced(self.task, self.bead, 2, self.repo.sha, [{"kind": "skipped", "file": "crates/types/src/retry.rs", "line": 1, "issue": "404 test not written"}]))]
        self.assertEqual([r["jev_receipts"] for r in rows], [[], jev])
