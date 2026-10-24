"""Transport/contract failures must not become a successful sanity check."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch

spec = importlib.util.spec_from_file_location("jev_client", Path(__file__).parents[1] / "jev_client.py")
assert spec is not None and spec.loader is not None
client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(client)
sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "atm-beads/tests"))
from phase_fixtures import write_phase


def reply():
    return {"model": client.MODEL, "answers": {"startup": {
        "type": "choice", "choice": "ready", "confidence": 0.99,
        "probabilities": {"ready": 1.0, "other": 0.0},
    }}, "usage": {"input_tokens": 1, "output_tokens": 1}}


def no_sleep(delay):
    raise AssertionError(f"Unexpected retry delay: {delay}")


class FakeClock:
    def __init__(self):
        self.now = 100.0
        self.waits = []

    def __call__(self):
        return self.now

    def advance(self, seconds):
        self.now += seconds

    def sleep(self, seconds):
        self.waits.append(seconds)
        self.advance(seconds)


class JevClientTests(unittest.TestCase):
    def test_missing_key_no_network(self):
        with patch.dict(client.os.environ, {}, clear=True), patch.object(client.http.client, "HTTPSConnection") as http:
            with self.assertRaises(client.JevError):
                client.evaluate(client.startup_request(), sleep=no_sleep)
            http.assert_not_called()

    def startup(self, argv, team, daemon, environ=None, phase_lead="appointed-lead"):
        """Run the startup probe without a key; `atm escalation list` answers with `team` / `daemon` recipients."""
        def run(cmd, **kwargs):
            listed = {"--team": team} if "--team" in cmd else {"": daemon}
            recipients = next(iter(listed.values()))
            return MagicMock(returncode=0 if recipients is not None else 1, stdout=json.dumps({"recipients": recipients or []}))
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            write_phase(repo, "x", lead=phase_lead)
            config = repo / ".claude/project/atm-bd-orchestration.yaml"
            config.parent.mkdir(parents=True)
            config.write_text("lead: stale-repository-lead\n")
            return self.run_startup(argv, environ, repo, run)

    def run_startup(self, argv, environ, repo, run):
        with patch.dict(client.os.environ, environ if environ is not None else {"ATM_TEAM": "t"}, clear=True), \
             patch.object(client.repo_config, "repo_root", return_value=repo), \
             patch.object(client.subprocess, "run", side_effect=run) as send:
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                rc = client.main(argv, sleep=no_sleep)
        sends = [(c.args[0][2], c.kwargs["input"]) for c in send.call_args_list if c.args[0][:2] == ["atm", "send"]]
        return rc, json.loads(output.getvalue()), sends

    def test_startup_announces_to_escalation_recipients_else_lead(self):
        rc, result, sends = self.startup(["--startup", "--announce", "--phase", "x"], ["oversight@team"], ["daemon@host"])
        self.assertEqual(rc, 2)
        self.assertTrue(result["error"]["recoverable"])
        self.assertEqual(set(result["error"]), {"code", "message", "recoverable", "suggested_action"})
        self.assertEqual([to for to, _ in sends], ["oversight@team"])
        self.assertIn(result["error"]["message"], sends[0][1])
        _, _, sends = self.startup(["--startup", "--announce", "--phase", "x"], [], ["daemon@host"])
        self.assertEqual([to for to, _ in sends], ["daemon@host"])
        _, _, sends = self.startup(["--startup", "--announce", "--phase", "x"], [], None)
        self.assertEqual([to for to, _ in sends], ["appointed-lead"])
        self.assertTrue(sends[0][1].endswith("No escalation recipient is set."))
        _, _, sends = self.startup(["--startup", "--announce", "--phase", "x"], None, ["daemon@host"], environ={})
        self.assertEqual([to for to, _ in sends], ["daemon@host"])

    def test_startup_without_announce_sends_nothing(self):
        rc, _, sends = self.startup(["--startup"], ["oversight@team"], ["daemon@host"])
        self.assertEqual((rc, sends), (2, []))
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            client.main(["--startup", "--announce"], sleep=no_sleep)

    def test_error_announces_a_child_failure_verbatim_without_probing(self):
        error = "SANITY.JEV_UNAVAILABLE: Jev HTTP 503; response body withheld"
        with patch.object(client.http.client, "HTTPSConnection") as http:
            rc, result, sends = self.startup(["--announce", "--error", error, "--phase", "x"], ["oversight@team"], ["daemon@host"],
                                             environ={"ATM_TEAM": "t", "TYPESAFE_API_KEY": "test-only-key"})
            http.assert_not_called()
        self.assertEqual((rc, result), (0, {"success": True, "data": {"announced": error}, "error": None}))
        self.assertEqual([to for to, _ in sends], ["oversight@team"])
        self.assertIn(error, sends[0][1])
        _, _, sends = self.startup(["--announce", "--error", error, "--phase", "x"], [], None)
        self.assertEqual([to for to, _ in sends], ["appointed-lead"])
        self.assertIn(error, sends[0][1])
        self.assertTrue(sends[0][1].endswith("No escalation recipient is set."))
        for argv in (["--error", error], ["--announce", "--error", "", "--phase", "x"],
                     ["--announce", "--error", error], ["--startup", "--error", error, "--announce", "--phase", "x"],
                     ["--request", "r.json", "--announce", "--phase", "x"]):
            with self.subTest(argv=argv), self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
                client.main(argv, sleep=no_sleep)

    def test_announcement_uses_the_selected_phase_lead_despite_environment_and_yaml(self):
        for lead in ("phase-owner-one", "phase-owner-two"):
            with self.subTest(lead=lead):
                _, _, sends = self.startup(["--startup", "--announce", "--phase", "x"], [], None,
                                           environ={"ATM_IDENTITY": "wrong-environment-lead"}, phase_lead=lead)
                self.assertEqual([recipient for recipient, _ in sends], [lead])

    def test_missing_phase_settings_stop_before_probe_or_send(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            path = write_phase(repo, "x")
            for invalid in ("lead = ''\n", "not valid TOML"):
                with self.subTest(invalid=invalid):
                    path.write_text(invalid)
                    with patch.object(client.repo_config, "repo_root", return_value=repo), \
                         patch.object(client, "evaluate") as evaluate, \
                         patch.object(client.subprocess, "run") as send, \
                         contextlib.redirect_stderr(io.StringIO()) as stderr, self.assertRaises(SystemExit):
                        client.main(["--startup", "--announce", "--phase", "x"], sleep=no_sleep)
                    self.assertIn("phase-x.toml", stderr.getvalue())
                    evaluate.assert_not_called()
                    send.assert_not_called()

    def test_legacy_lead_override_is_rejected(self):
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()) as stderr:
            client.main(["--startup", "--announce", "--lead", "wrong-lead"], sleep=no_sleep)
        self.assertIn("unrecognized arguments: --lead", stderr.getvalue())

    def test_successful_transport(self):
        conn = MagicMock()
        conn.getresponse.return_value.status = 200
        conn.getresponse.return_value.read.return_value = json.dumps(reply()).encode()
        with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), patch.object(client.http.client, "HTTPSConnection", return_value=conn):
            self.assertEqual(client.evaluate(client.startup_request(), sleep=no_sleep), reply())
        self.assertEqual(conn.request.call_args.args[:2], ("POST", "/v1/systemone"))
        conn.close.assert_called_once()

    def test_auth_error_does_not_echo_body_or_key(self):
        conn = MagicMock()
        conn.getresponse.return_value.status = 401
        conn.getresponse.return_value.read.return_value = b"test-only-key server details"
        with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), patch.object(client.http.client, "HTTPSConnection", return_value=conn):
            with self.assertRaises(client.JevError) as raised:
                client.evaluate(client.startup_request(), sleep=no_sleep)
        self.assertNotIn("test-only-key", str(raised.exception))
        self.assertFalse(raised.exception.recoverable)
        self.assertEqual(conn.request.call_count, 1)

    def test_retry_is_bounded(self):
        self.assertEqual(client.MAX_JEV_ATTEMPTS, 4)
        for transient in (429, 502, 529, OSError("down"), client.socket.timeout("late"),
                          client.http.client.HTTPException("broken")):
            with self.subTest(transient=transient):
                conn = MagicMock()
                if isinstance(transient, Exception):
                    conn.request.side_effect = transient
                else:
                    conn.getresponse.return_value.status = transient
                conn.getresponse.return_value.read.return_value = b"{}"
                conn.getresponse.return_value.getheader.return_value = "0"
                with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                     patch.object(client.http.client, "HTTPSConnection", return_value=conn), \
                     self.assertRaises(client.JevError) as raised:
                    client.evaluate(client.startup_request(), sleep=no_sleep, max_attempts=99)
                self.assertEqual(conn.request.call_count, 4)
                self.assertEqual(conn.close.call_count, 4)
                self.assertTrue(raised.exception.recoverable)

    def test_transient_statuses_and_transport_retry_to_success(self):
        for transient in (429, 502, 529, "transport"):
            with self.subTest(transient=transient):
                conn = MagicMock()
                good = MagicMock(status=200)
                good.read.return_value = json.dumps(reply()).encode()
                bad = MagicMock(status=transient, getheader=MagicMock(return_value="0"))
                bad.read.return_value = b"{}"
                if transient == "transport":
                    conn.request.side_effect = [OSError("unavailable"), None]
                    conn.getresponse.return_value = good
                else:
                    conn.getresponse.side_effect = [bad, good]
                with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                     patch.object(client.http.client, "HTTPSConnection", return_value=conn):
                    self.assertEqual(client.evaluate(client.startup_request(), sleep=no_sleep), reply())
                self.assertEqual(conn.request.call_count, 2)

    def test_other_http_errors_are_terminal(self):
        for status in sorted(set(range(400, 600)) - {413, 429, 502, 529}):
            with self.subTest(status=status):
                conn = MagicMock()
                conn.getresponse.return_value.status = status
                conn.getresponse.return_value.read.return_value = b"private server response"
                with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                     patch.object(client.http.client, "HTTPSConnection", return_value=conn), \
                     self.assertRaises(client.JevError) as raised:
                    client.evaluate(client.startup_request(), sleep=no_sleep)
                self.assertEqual(conn.request.call_count, 1)
                self.assertFalse(raised.exception.recoverable)
                self.assertEqual(raised.exception.message, f"Jev HTTP {status}; response body withheld")

    def test_invalid_responses_are_terminal(self):
        for raw in (b"x" * (client.MAX_RESPONSE_BYTES + 1), b"not-json", b"\xff",
                    json.dumps({**reply(), "model": "wrong"}).encode(),
                    json.dumps({**reply(), "answers": {}}).encode()):
            with self.subTest(raw_size=len(raw)):
                conn = MagicMock()
                conn.getresponse.return_value.status = 200
                conn.getresponse.return_value.read.return_value = raw
                with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                     patch.object(client.http.client, "HTTPSConnection", return_value=conn), \
                     self.assertRaises(client.JevError) as raised:
                    client.evaluate(client.startup_request(), sleep=no_sleep)
                self.assertEqual(conn.request.call_count, 1)
                self.assertEqual(raised.exception.code, "SANITY.JEV_RESPONSE_INVALID")
                self.assertFalse(raised.exception.recoverable)

    def test_retry_after_is_honoured_and_capped_without_sleeping(self):
        conn = MagicMock()
        good = MagicMock(status=200)
        good.read.return_value = json.dumps(reply()).encode()
        busy = MagicMock(status=529, getheader=MagicMock(return_value="99"))
        busy.read.return_value = b"{}"
        conn.getresponse.side_effect = [busy, good]
        waits = []
        with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
             patch.object(client.http.client, "HTTPSConnection", return_value=conn):
            self.assertEqual(client.evaluate(client.startup_request(), sleep=waits.append), reply())
        self.assertEqual(waits, [30.0])
        self.assertEqual(conn.request.call_count, 2)

    def test_parts_have_independent_four_attempt_budgets(self):
        request = client.written_request("delivery", "evidence", part=("1 of 3", ["x"]))
        answer = {"model": client.MODEL, "answers": {"written": {
            "type": "choice", "choice": "yes", "confidence": 1.0, "probabilities": {"yes": 1.0, "no": 0.0}}}}
        good = MagicMock(status=200)
        good.read.return_value = json.dumps(answer).encode()
        busy = MagicMock(status=529, getheader=MagicMock(return_value="0"))
        busy.read.return_value = b"{}"
        for statuses, count in (([good, good, good], 3), ([busy, busy, busy, good] * 3, 12)):
            conn = MagicMock()
            conn.getresponse.side_effect = statuses
            with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                 patch.object(client.http.client, "HTTPSConnection", return_value=conn):
                result = client.evaluate_parts([request, request, request], "test-only-key", sleep=no_sleep)
            self.assertEqual(conn.request.call_count, count)
            self.assertEqual([p["part"] for p in result["parts"]], ["1 of 3", "2 of 3", "3 of 3"])

    def test_server_oversize_splits_with_remaining_budget_and_eventually_succeeds(self):
        request = client.written_request("delivery", "x" * 20000)
        answer = {"model": client.MODEL, "answers": {"written": {
            "type": "choice", "choice": "yes", "confidence": 1.0, "probabilities": {"yes": 1.0, "no": 0.0}}}}
        good = MagicMock(status=200)
        good.read.return_value = json.dumps(answer).encode()
        too_large = MagicMock(status=413)
        too_large.read.return_value = b"{}"
        conn = MagicMock()
        conn.getresponse.side_effect = [too_large, good, good, good]
        with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
             patch.object(client.http.client, "HTTPSConnection", return_value=conn):
            result = client.evaluate_parts([request], "test-only-key", ["x"], sleep=no_sleep)
        self.assertGreaterEqual(len(result["parts"]), 2)
        self.assertTrue(all(part["changed_files"] == ["x"] for part in result["parts"]))
        sent = [json.loads(call.kwargs["body"]) for call in conn.request.call_args_list[1:]]
        self.assertEqual("".join(part["state"]["evidence"] for part in sent), "x" * 20000)

    def test_split_preserves_empty_context_and_external_evidence(self):
        request = client.written_request("delivery", "x" * 20000, {"empty.txt": ""},
                                         external=[{"kind": "bead_notes", "ref": "b", "text": ""}])
        pieces = client.split_request(request, client.size(request) // 2, ["x"])
        self.assertTrue(any(piece["state"].get("context") == {"empty.txt": ""} for piece in pieces))
        self.assertTrue(any(piece["state"].get("external_evidence") == [
            {"kind": "bead_notes", "ref": "b", "text": ""}] for piece in pieces))

    def test_four_oversize_attempts_never_issue_a_fifth(self):
        request = client.written_request("delivery", "x" * 100000, part=("1 of 1", ["x"]))
        # Keep the initial request within the local cap so the injected server supplies every failure.
        request["state"]["evidence"] = "x" * 50000
        conn = MagicMock()
        conn.getresponse.return_value.status = 413
        conn.getresponse.return_value.read.return_value = b"{}"
        with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
             patch.object(client.http.client, "HTTPSConnection", return_value=conn), \
             self.assertRaises(client.JevError) as raised:
            client.evaluate_parts([request], "test-only-key", sleep=no_sleep)
        self.assertEqual(conn.request.call_count, 4)
        self.assertIn("oversized", raised.exception.message)

    def test_shortened_children_share_the_original_parts_remaining_attempts(self):
        request = client.written_request("delivery", "x" * 20000, part=("1 of 1", ["x"]))
        conn = MagicMock()
        too_large = MagicMock(status=413)
        too_large.read.return_value = b"{}"
        busy = MagicMock(status=529)
        busy.read.return_value = b"{}"
        busy.getheader.return_value = "0"
        good = MagicMock(status=200)
        good.read.return_value = json.dumps({"model": client.MODEL, "answers": {"written": {
            "type": "choice", "choice": "yes", "confidence": 1.0, "probabilities": {"yes": 1.0, "no": 0.0}}}}).encode()
        conn.getresponse.side_effect = [too_large, busy, busy, good, good]
        with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
             patch.object(client.http.client, "HTTPSConnection", return_value=conn), \
             self.assertRaises(client.JevError) as raised:
            client.evaluate_parts([request], "test-only-key", sleep=no_sleep)
        self.assertEqual(conn.request.call_count, 4)
        self.assertEqual(raised.exception.message, "Jev retry budget exhausted")

    def test_elapsed_includes_retries_and_is_present_on_failure(self):
        for outcomes, elapsed, status in [([529, 200], 4.5, 0), ([OSError("down")] * 4, 5.0, 2),
                                           ([502] * 4, 11.0, 2), ([529] * 4, 11.0, 2), ([429] * 4, 11.0, 2),
                                           ([503], 1.25, 2), (["invalid"], 1.25, 2)]:
            with self.subTest(outcomes=outcomes):
                clock = FakeClock()
                conn = MagicMock()
                pending = iter(outcomes)

                def send(*args, **kwargs):
                    clock.advance(1.25)
                    outcome = next(pending)
                    if isinstance(outcome, Exception):
                        raise outcome
                    response = MagicMock(status=200 if outcome == "invalid" else outcome)
                    response.read.return_value = b"not-json" if outcome == "invalid" else json.dumps(reply()).encode()
                    response.getheader.return_value = "2"
                    conn.getresponse.return_value = response

                conn.request.side_effect = send
                output = io.StringIO()
                original_clock = client.time.monotonic
                with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                     patch.object(client.http.client, "HTTPSConnection", return_value=conn), \
                     contextlib.redirect_stdout(output):
                    self.assertEqual(client.main(["--startup"], clock=clock, sleep=clock.sleep), status)
                result = json.loads(output.getvalue())
                self.assertEqual(result["elapsed_s"], elapsed)
                self.assertEqual(conn.request.call_count, len(outcomes))
                self.assertEqual(clock.waits, [2.0] * (len(outcomes) - 1) if isinstance(outcomes[0], int)
                                 and outcomes[0] in (429, 502, 529) else [])
                self.assertIs(client.time.monotonic, original_clock)
                if status:
                    self.assertIsNone(result["data"])
                    self.assertTrue(client.is_client_message(result["error"]["message"]))

    def test_bad_answer_shapes(self):
        for change in [lambda r: r.update(model="wrong"),
                       lambda r: r.update(answers={}),
                       lambda r: r["answers"]["startup"].update(choice={}),
                       lambda r: r["answers"]["startup"].update(confidence=float("nan")),
                       lambda r: r["answers"]["startup"].update(probabilities={"ready": 0.1, "other": 0.1})]:
            value = reply()
            change(value)
            with self.assertRaises(client.JevError):
                client.validate_response(value, client.startup_request())

    def test_local_oversize_is_shortened_before_sending_with_independent_part_budgets(self):
        header = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n"
        body = "+" + "x" * client.MAX_REQUEST_BYTES + "\n"
        request = client.written_request("delivery", header + body, part=("1 of 1", ["f"]))
        expected_parts = client.split_request(request, client.MAX_REQUEST_BYTES, ["f"])
        conn = MagicMock()
        busy = MagicMock(status=529)
        busy.read.return_value = b"{}"
        busy.getheader.return_value = "0"
        good = MagicMock(status=200)
        good.read.return_value = json.dumps({"model": client.MODEL, "answers": {"written": {
            "type": "choice", "choice": "yes", "confidence": 1.0, "probabilities": {"yes": 1.0, "no": 0.0}}}}).encode()
        conn.getresponse.side_effect = [busy, busy, busy, good] * len(expected_parts)
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / "request.json"
            file.write_text(json.dumps(request))
            output = io.StringIO()
            clock = FakeClock()
            conn.request.side_effect = lambda *args, **kwargs: clock.advance(1.0)
            with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                 patch.object(client.http.client, "HTTPSConnection", return_value=conn), \
                 patch.object(client, "diff_pieces", wraps=client.diff_pieces) as split, \
                 contextlib.redirect_stdout(output):
                self.assertEqual(client.main(["--request", str(file)], clock=clock, sleep=no_sleep), 0)
        split.assert_called()
        self.assertEqual(conn.request.call_count, 4 * len(expected_parts))
        sent = [json.loads(call.kwargs["body"]) for call in conn.request.call_args_list[3::4]]
        self.assertEqual(sent, expected_parts)
        self.assertTrue(all(client.size(part) <= client.MAX_REQUEST_BYTES for part in sent))
        self.assertEqual("".join(part["state"]["evidence"].split("@@ -1 +1 @@\n", 1)[1] for part in sent), body)
        result = json.loads(output.getvalue())
        self.assertEqual(result["elapsed_s"], conn.request.call_count)
        self.assertTrue(all(client.verify_receipt(part["receipt"], "test-only-key") for part in result["data"]["parts"]))

    def test_unsplittable_local_request_reports_too_large_without_sending(self):
        request = client.written_request("d" * client.MAX_REQUEST_BYTES, "evidence")
        with tempfile.TemporaryDirectory() as directory:
            file = Path(directory) / "request.json"
            file.write_text(json.dumps(request))
            output = io.StringIO()
            with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                 patch.object(client.http.client, "HTTPSConnection") as http, \
                 contextlib.redirect_stdout(output):
                self.assertEqual(client.main(["--request", str(file)], clock=FakeClock(), sleep=no_sleep), 2)
        http.assert_not_called()
        result = json.loads(output.getvalue())
        self.assertEqual(result["elapsed_s"], 0.0)
        self.assertEqual(result["error"]["code"], "SANITY.JEV_REQUEST_TOO_LARGE")
        self.assertIn("oversized", result["error"]["message"])

    def test_non_text_oversize_state_is_terminal_without_network(self):
        for state in ("x" * client.MAX_REQUEST_BYTES,
                      {"deliverable": "d", "evidence": {"blob": "x" * client.MAX_REQUEST_BYTES}},
                      {"deliverable": "d", "context": ["x" * client.MAX_REQUEST_BYTES]},
                      {"deliverable": "d", "external_evidence": {"blob": "x" * client.MAX_REQUEST_BYTES}}):
            with self.subTest(state_type=type(state).__name__):
                request = {**client.startup_request(), "state": state}
                with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                     patch.object(client.http.client, "HTTPSConnection") as http, \
                     self.assertRaises(client.JevError) as raised:
                    client.evaluate_parts([request], "test-only-key", sleep=no_sleep)
                http.assert_not_called()
                self.assertEqual(raised.exception.code, "SANITY.JEV_REQUEST_TOO_LARGE")

    def test_finite_request_values_only(self):
        request = client.startup_request()
        request["state"] = {"bad": float("inf")}
        with self.assertRaises(client.JevError):
            client.validate_request(request)

    def test_request_prints_a_receipt_only_the_call_can_write(self):
        request = {"model": client.MODEL, "state": {"deliverable": "d", "evidence": "e"}, "questions": {"written": {
            "type": "choice", "instructions": "Delivered?", "criteria": {"yes": "delivered", "no": "not delivered"}}}}
        response = {"model": client.MODEL, "id": "resp-1", "answers": {"written": {
            "type": "choice", "choice": "no", "confidence": 0.9, "probabilities": {"yes": 0.1, "no": 0.9}}}}
        conn = MagicMock()
        conn.getresponse.return_value.status = 200
        conn.getresponse.return_value.read.return_value = json.dumps(response).encode()
        import tempfile
        with tempfile.NamedTemporaryFile("w", suffix=".json") as file:
            json.dump(request, file)
            file.flush()
            output = io.StringIO()
            with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                 patch.object(client.http.client, "HTTPSConnection", return_value=conn), contextlib.redirect_stdout(output):
                self.assertEqual(client.main(["--request", file.name], sleep=no_sleep), 0)
        receipt = json.loads(output.getvalue())["data"]["receipt"]
        self.assertEqual({k: receipt[k] for k in ("model", "question", "choice", "probabilities", "response_id")},
                         {"model": client.MODEL, "question": "written", "choice": "no",
                          "probabilities": {"yes": 0.1, "no": 0.9}, "response_id": "resp-1"})
        self.assertEqual(receipt["request_sha256"], client.hashlib.sha256(client.canonical(request)).hexdigest())
        self.assertTrue(client.verify_receipt(receipt, "test-only-key"))
        self.assertFalse(client.verify_receipt(receipt, "another-key"))
        self.assertFalse(client.verify_receipt(dict(receipt, choice="yes"), "test-only-key"))
        self.assertFalse(client.verify_receipt({k: v for k, v in receipt.items() if k != "mac"}, "test-only-key"))

    def test_assignment_request_carries_the_deliverable_verbatim_and_the_committed_diff(self):
        import subprocess, tempfile
        with tempfile.TemporaryDirectory() as tmp:
            def git(*args):
                return subprocess.run(["git", "-C", tmp, "-c", "user.name=t", "-c", "user.email=t@t", *args],
                                      check=True, capture_output=True, text=True).stdout.strip()
            git("init", "-q")
            Path(tmp, "server.py").write_text("bind()\ndrop()\nrebind()\n")
            Path(tmp, "notes.md").write_text("context note\n")
            git("add", "-A"); git("commit", "-qm", "base")
            base = git("rev-parse", "HEAD")
            Path(tmp, "server.py").write_text("bind()\nhand_off_fd()\n")
            git("commit", "-qam", "fix")
            commit = git("rev-parse", "HEAD")
            assignment = {"deliverable": {"number": 1, "text": "Keep the listener open and hand its fd to the child."},
                          "worktree_path": tmp, "commit": commit, "base_sha": base,
                          "changed_files": ["server.py"], "context": ["notes.md"]}
            (request,) = client.assignment_requests(assignment)
            self.assertEqual(request["state"]["deliverable"], assignment["deliverable"]["text"])
            self.assertIn("+hand_off_fd()", request["state"]["evidence"])
            self.assertIn("-rebind()", request["state"]["evidence"])
            self.assertEqual(request["state"]["context"], {"notes.md": "context note\n"})
            client.validate_request(request)

            response = {"model": client.MODEL, "answers": {"written": {
                "type": "choice", "choice": "yes", "confidence": 0.9, "probabilities": {"yes": 0.95, "no": 0.05}}}}
            conn = MagicMock()
            conn.getresponse.return_value.status = 200
            conn.getresponse.return_value.read.return_value = json.dumps(response).encode()
            file = Path(tmp, "..", Path(tmp).name + "-assignment.json").resolve()
            file.write_text(json.dumps(assignment))
            try:
                output = io.StringIO()
                with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                     patch.object(client.http.client, "HTTPSConnection", return_value=conn), contextlib.redirect_stdout(output):
                    self.assertEqual(client.main(["--assignment", str(file)], sleep=no_sleep), 0)
                sent = json.loads(conn.request.call_args.kwargs["body"])
                self.assertEqual(sent, request)
                receipt = json.loads(output.getvalue())["data"]["receipt"]
                self.assertEqual(receipt["request_sha256"], client.hashlib.sha256(client.canonical(request)).hexdigest())
                self.assertTrue(client.verify_receipt(receipt, "test-only-key"))

                for broken, message in (({**assignment, "commit": "0" * 40}, "Committed evidence unreadable at the pinned commits"),
                                        ({k: v for k, v in assignment.items() if k != "base_sha"},
                                         "Assignment lacks deliverable text, worktree, commits or file lists")):
                    file.write_text(json.dumps(broken))
                    output = io.StringIO()
                    with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), contextlib.redirect_stdout(output):
                        self.assertEqual(client.main(["--assignment", str(file)], sleep=no_sleep), 2)
                    error = json.loads(output.getvalue())["error"]
                    self.assertEqual((error["code"], error["message"]), ("VALIDATION.INPUT", message))
            finally:
                file.unlink()

    def test_oversize_assignment_splits_and_preserves_context(self):
        import subprocess, tempfile
        with tempfile.TemporaryDirectory() as tmp:
            def git(*args):
                return subprocess.run(["git", "-C", tmp, "-c", "user.name=t", "-c", "user.email=t@t", *args],
                                      check=True, capture_output=True, text=True).stdout.strip()
            git("init", "-q")
            Path(tmp, "server.py").write_text("bind()\n")
            big = "".join(f"line {i} " + "x" * 90 + "\n" for i in range(1, 1201))  # ~120 KB, over the cap
            Path(tmp, "big.md").write_text(big)
            git("add", "-A"); git("commit", "-qm", "base")
            base = git("rev-parse", "HEAD")
            Path(tmp, "server.py").write_text("bind()\nhand_off_fd()\n")
            git("commit", "-qam", "fix")
            assignment = {"deliverable": {"number": 1, "text": "Hand the fd to the child."}, "worktree_path": tmp,
                          "commit": git("rev-parse", "HEAD"), "base_sha": base,
                          "changed_files": ["server.py"], "context": ["big.md"]}
            file = Path(tmp, "..", Path(tmp).name + "-assignment.json").resolve()
            file.write_text(json.dumps(assignment))
            try:
                requests = client.assignment_requests(assignment)
                self.assertGreater(len(requests), 1)
                self.assertEqual("".join(r["state"].get("context", {}).get("big.md", "") for r in requests), big)
                self.assertTrue(all(client.size(r) <= client.MAX_REQUEST_BYTES for r in requests))
                (rebuilt,) = client.assignment_requests({**assignment, "context": ["big.md:600-602"]})  # the rebuild fits
                client.validate_request(rebuilt)
                self.assertEqual(rebuilt["state"]["context"], {"big.md:600-602": "".join(big.splitlines(keepends=True)[599:602])})
                (retry,) = client.assignment_requests({**assignment, "context": []})
                client.validate_request(retry)
            finally:
                file.unlink()

    def test_a_line_range_context_entry_sends_only_those_lines_and_a_bad_range_is_rejected(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            base, commit = self.split_repo(tmp, {"a.py": "start\nchanged\n"})
            Path(tmp, "notes.md").write_text("one\ntwo\nthree\nfour\n")
            client.subprocess.run(["git", "-C", tmp, "add", "notes.md"], check=True)
            client.subprocess.run(["git", "-C", tmp, "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "notes"],
                                  check=True, capture_output=True)
            commit = client.subprocess.run(["git", "-C", tmp, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
            assignment = {"deliverable": {"number": 1, "text": "d"}, "worktree_path": tmp, "commit": commit,
                          "base_sha": base, "changed_files": ["a.py"], "context": ["notes.md:2-3", "notes.md"]}
            (request,) = client.assignment_requests(assignment)
            self.assertEqual(request["state"]["context"], {"notes.md:2-3": "two\nthree\n", "notes.md": "one\ntwo\nthree\nfour\n"})
            for entry in ("notes.md:0-2", "notes.md:3-2", "notes.md:4-5"):
                with self.subTest(entry=entry), self.assertRaises(client.JevError) as raised:
                    client.assignment_requests({**assignment, "context": [entry]})
                self.assertEqual((raised.exception.code, raised.exception.message),
                                 ("VALIDATION.INPUT", f"Context entry {entry} names lines outside its file at the pinned commit"))
                self.assertIn("Fix that `context` entry", client.failure(raised.exception)["error"]["suggested_action"])
            with self.assertRaises(client.JevError) as raised:
                client.assignment_requests({**assignment, "context": ["missing.md:1-2"]})
            self.assertEqual(raised.exception.message, "Committed evidence unreadable at the pinned commits")

    def split_repo(self, tmp, files):
        """A repository whose one commit changes `files` (name -> new text) from a one-line base."""
        import subprocess
        def git(*args):
            return subprocess.run(["git", "-C", tmp, "-c", "user.name=t", "-c", "user.email=t@t", *args],
                                  check=True, capture_output=True, text=True).stdout.strip()
        git("init", "-q")
        for name in files:
            Path(tmp, name).write_text("start\n" + "\n".join(f"keep {i}" for i in range(0, 4000, 40)) + "\n")
        git("add", "-A"); git("commit", "-qm", "base")
        base = git("rev-parse", "HEAD")
        for name, text in files.items():
            Path(tmp, name).write_text(text)
        git("commit", "-qam", "change")
        return base, git("rev-parse", "HEAD")

    def run_split(self, assignment, choices):
        """Run `--assignment` with Jev answering each part in turn from `choices`; the sent requests and the output."""
        sent, replies = [], iter(choices)
        def connection(*_args, **_kwargs):
            conn = MagicMock()
            def request(_method, _path, body, headers):
                sent.append(body)
                choice = next(replies)
                conn.getresponse.return_value.read.return_value = json.dumps({"model": client.MODEL, "id": f"r{len(sent)}",
                    "answers": {"written": {"type": "choice", "choice": choice, "confidence": 0.9, "probabilities":
                        {"yes": 0.8 if choice == "yes" else 0.3, "no": 0.2 if choice == "yes" else 0.7}}}}).encode()
            conn.request.side_effect = request
            conn.getresponse.return_value.status = 200
            return conn
        file = Path(assignment["worktree_path"], "..", Path(assignment["worktree_path"]).name + "-assignment.json").resolve()
        file.write_text(json.dumps(assignment))
        try:
            output = io.StringIO()
            with patch.dict(client.os.environ, {"TYPESAFE_API_KEY": "test-only-key"}), \
                 patch.object(client.http.client, "HTTPSConnection", side_effect=connection), contextlib.redirect_stdout(output):
                rc = client.main(["--assignment", str(file)], sleep=no_sleep)
        finally:
            file.unlink()
        return rc, sent, json.loads(output.getvalue())

    def test_a_diff_over_the_cap_is_split_by_hunks_and_every_part_returns_its_own_result(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            # one file whose diff alone is over the cap: 100 changed hunks of ~1.5 KB each, plus a small file
            big = "start\n" + "\n".join(f"keep {i}\n" + "\n".join(f"added {i}.{j} " + "y" * 60 for j in range(20))
                                       for i in range(0, 4000, 40)) + "\n"
            base, commit = self.split_repo(tmp, {"big.py": big, "small.py": "start\nsmall change\n"})
            assignment = {"deliverable": {"number": 1, "text": "Add the generated lines."}, "worktree_path": tmp,
                          "commit": commit, "base_sha": base, "changed_files": ["big.py", "small.py"], "context": []}
            whole = client.subprocess.run(["git", "-C", tmp, "diff", "--no-color", f"{base}...{commit}", "--", "big.py"],
                                          capture_output=True, text=True).stdout
            self.assertGreater(len(whole.encode()), client.MAX_REQUEST_BYTES)
            requests = client.assignment_requests(assignment)
            self.assertGreater(len(requests), 1)
            for i, request in enumerate(requests, 1):
                self.assertLessEqual(len(client.validate_request(request)), client.MAX_REQUEST_BYTES)
                self.assertEqual(request["state"]["deliverable"], assignment["deliverable"]["text"])
                self.assertEqual(request["state"]["part"], f"{i} of {len(requests)}")
                self.assertEqual(request["state"]["changed_files"], ["big.py", "small.py"])
            evidence = "".join(r["state"]["evidence"] for r in requests)
            for line in whole.splitlines():
                if line.startswith(("+", "-")) and not line.startswith(("+++", "---")):
                    self.assertIn(line, evidence)
            self.assertIn("+small change", evidence)

            for instruction in (r["questions"]["written"]["instructions"] for r in requests):
                self.assertNotIn("every part", instruction)  # a part is asked only about its own share

            choices = ["yes"] + ["no"] + ["yes"] * (len(requests) - 2)
            rc, sent, result = self.run_split(assignment, choices)
            self.assertEqual((rc, result["error"]), (0, None))
            self.assertEqual([json.loads(b) for b in sent], requests)
            data = result["data"]
            self.assertEqual(set(data), {"model", "parts"})  # no combined choice, answer or receipt
            self.assertEqual(len(data["parts"]), len(requests))
            for i, (part, request, choice) in enumerate(zip(data["parts"], requests, choices), 1):
                self.assertEqual(set(part), {"part", "changed_files", "response", "receipt"})
                self.assertEqual((part["part"], part["changed_files"]), (f"{i} of {len(requests)}", ["big.py", "small.py"]))
                self.assertEqual(part["response"]["id"], f"r{i}")
                self.assertEqual(part["response"]["answers"]["written"]["choice"], choice)
                receipt = part["receipt"]
                self.assertEqual((receipt["question"], receipt["choice"], receipt["response_id"]), ("written", choice, f"r{i}"))
                self.assertEqual(receipt["probabilities"], part["response"]["answers"]["written"]["probabilities"])
                self.assertEqual(receipt["request_sha256"], client.hashlib.sha256(client.canonical(request)).hexdigest())
                self.assertTrue(client.verify_receipt(receipt, "test-only-key"))

    def test_a_hunk_over_the_cap_is_cut_by_lines_and_a_line_by_characters(self):
        pieces = client.diff_pieces("diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1,2 @@\n" + "+" + "z" * 9000 + "\n"
                                    + "".join(f"+line {i}\n" for i in range(2000)), 4000)
        self.assertGreater(len(pieces), 3)
        for piece in pieces:
            self.assertLessEqual(len(json.dumps(piece)) - 2, 4000)
            self.assertTrue(piece.startswith("diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1,2 @@\n"))
        body = "".join(p.split("@@ -1 +1,2 @@\n", 1)[1] for p in pieces)
        self.assertEqual(body, "+" + "z" * 9000 + "\n" + "".join(f"+line {i}\n" for i in range(2000)))

    def test_deliverable_text_alone_over_the_cap_names_the_plan_fix(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            base, commit = self.split_repo(tmp, {"a.py": "start\nchanged\n"})
            with self.assertRaises(client.JevError) as raised:
                client.assignment_requests({"deliverable": {"number": 1, "text": "d" * client.MAX_REQUEST_BYTES},
                                            "worktree_path": tmp, "commit": commit, "base_sha": base,
                                            "changed_files": ["a.py"], "context": []})
            action = client.failure(raised.exception)["error"]["suggested_action"]
            self.assertIn("split into smaller numbered deliverables", action)
            self.assertNotIn("unchanged", action)

    REFS = [{"kind": "bead_notes", "bead": "lint-k7", "text": "Ran `just test`: 412 passed at abc123."},
            {"kind": "pr_body", "repo": "o/r", "pr": 460},
            {"kind": "pr_comment", "repo": "o/r", "comment_id": 77}]

    def fetched(self, failing=()):
        """`subprocess.run` answering bd/gh for REFS (git passes through); a command naming a `failing` id exits 1."""
        real = client.subprocess.run
        answers = {"lint-k7": [{"id": "lint-k7", "notes": "Ran `just test`: 412 passed at abc123.", "updated_at": "2026-10-13T01:00:00Z"}],
                   "repos/o/r/pulls/460": {"body": "Evidence: smoke log attached.", "html_url": "https://github.com/o/r/pull/460",
                                           "updated_at": "2026-10-13T02:00:00Z", "user": {"login": "dev"}},
                   "repos/o/r/issues/comments/77": {"body": "Re-ran on Windows: green.", "html_url": "https://github.com/o/r/pull/460#c77",
                                                    "updated_at": "2026-10-13T03:00:00Z", "user": {"login": "qa"}}}

        def run(argv, **kwargs):
            if argv[0] == "git":
                return real(argv, **kwargs)
            key = argv[2]
            if key in failing:
                return MagicMock(returncode=1, stdout="", stderr="not found")
            return MagicMock(returncode=0, stdout=json.dumps(answers[key]), stderr="")
        return patch.object(client.subprocess, "run", side_effect=run)

    def test_evidence_only_deliverable_sends_bead_notes_and_pr_text_with_their_source(self):
        assignment = {"deliverable": {"number": 1, "text": "Record the Windows re-run result."}, "worktree_path": "/nonexistent",
                      "commit": "c" * 40, "base_sha": "b" * 40, "changed_files": [], "context": [], "external_evidence": self.REFS}
        with self.fetched() as run:
            (request,) = client.assignment_requests(assignment)
        self.assertEqual([c.args[0][:2] for c in run.call_args_list], [["gh", "api"], ["gh", "api"]])
        self.assertTrue(all(c.kwargs["cwd"] == "/nonexistent" for c in run.call_args_list))
        client.validate_request(request)
        self.assertEqual(request["state"]["evidence"], "")
        self.assertEqual(request["state"]["external_evidence"], [
            {"kind": "bead_notes", "ref": "lint-k7", "url": None, "updated_at": None, "author": None,
             "text": "Ran `just test`: 412 passed at abc123."},
            {"kind": "pr_body", "ref": "o/r#460", "url": "https://github.com/o/r/pull/460", "updated_at": "2026-10-13T02:00:00Z",
             "author": "dev", "text": "Evidence: smoke log attached."},
            {"kind": "pr_comment", "ref": "o/r#77", "url": "https://github.com/o/r/pull/460#c77", "updated_at": "2026-10-13T03:00:00Z",
             "author": "qa", "text": "Re-ran on Windows: green."}])
        with self.fetched():
            (without,) = client.assignment_requests({k: v for k, v in assignment.items() if k != "external_evidence"})
        self.assertNotIn("external_evidence", without["state"])

    def test_external_evidence_that_cannot_be_read_or_is_malformed_fails_before_any_jev_call(self):
        assignment = {"deliverable": {"number": 1, "text": "d"}, "worktree_path": "/nonexistent", "commit": "c" * 40,
                      "base_sha": "b" * 40, "changed_files": [], "context": [], "external_evidence": self.REFS}
        with self.fetched(failing=("repos/o/r/pulls/460",)), patch.object(client.http.client, "HTTPSConnection") as https:
            with self.assertRaises(client.JevError) as raised:
                client.assignment_requests(assignment)
            https.assert_not_called()
        error = client.failure(raised.exception)["error"]
        self.assertEqual((error["code"], error["message"]), ("VALIDATION.INPUT", "External evidence unreadable: pr_body o/r#460"))
        self.assertTrue(client.is_client_message(error["message"]))
        self.assertIn("gh auth status", error["suggested_action"])
        for bad in ([{"kind": "pr_body", "repo": "o/r"}], [{"kind": "pr_body", "repo": "o/r", "pr": "460"}],
                    [{"kind": "issue", "repo": "o/r", "pr": 1}], [{"kind": "bead_notes", "bead": "--all"}],
                    [{"kind": "bead_notes", "bead": "x", "text": 3}], {"kind": "bead_notes", "bead": "x"}):
            with self.subTest(bad=bad), self.fetched() as run, self.assertRaises(client.JevError) as raised:
                client.assignment_requests({**assignment, "external_evidence": bad})
            run.assert_not_called()
            self.assertEqual(raised.exception.message,
                             "Assignment external_evidence entry is not a bead_notes, pr_body or pr_comment reference")
            self.assertIn("pr_comment", client.failure(raised.exception)["error"]["suggested_action"])

    def test_layered_source_reads_lower_layer_context_at_the_pinned_top(self):
        import subprocess, tempfile
        with tempfile.TemporaryDirectory() as tmp:
            def git(*args):
                return subprocess.run(["git", "-C", tmp, "-c", "user.name=t", "-c", "user.email=t@t", *args],
                                      check=True, capture_output=True, text=True).stdout.strip()
            git("init", "-q")
            Path(tmp, "lib.py").write_text("def old(): pass\n")
            Path(tmp, "server.py").write_text("old()\n")
            git("add", "-A"); git("commit", "-qm", "trunk")
            trunk = git("rev-parse", "HEAD")
            Path(tmp, "lib.py").write_text("def hand_off(fd): pass\n")  # lower layer
            git("commit", "-qam", "layer 1")
            lower = git("rev-parse", "HEAD")
            Path(tmp, "server.py").write_text("hand_off(3)\n")  # top layer, the one checked
            git("commit", "-qam", "layer 2")
            top = git("rev-parse", "HEAD")
            assignment = {"deliverable": {"number": 1, "text": "Call hand_off with the listener fd."}, "worktree_path": tmp,
                          "commit": top, "base_sha": lower, "changed_files": ["server.py"], "context": ["lib.py"]}
            (request,) = client.assignment_requests(assignment)
            self.assertEqual(request["state"]["context"], {"lib.py": "def hand_off(fd): pass\n"})
            self.assertIn("+hand_off(3)", request["state"]["evidence"])
            self.assertNotIn("lib.py", request["state"]["evidence"])
            (layered,) = client.assignment_requests({**assignment, "base_sha": trunk, "changed_files": ["lib.py", "server.py"]})
            self.assertIn("+def hand_off(fd): pass", layered["state"]["evidence"])
            self.assertIn("+hand_off(3)", layered["state"]["evidence"])

    def test_external_evidence_over_the_cap_splits_without_dropping_text(self):
        assignment = {"deliverable": {"number": 1, "text": "d"}, "worktree_path": "/nonexistent", "commit": "c" * 40,
                      "base_sha": "b" * 40, "changed_files": [], "context": [],
                      "external_evidence": [{"kind": "bead_notes", "bead": "lint-k7", "text": "n" * client.MAX_REQUEST_BYTES}]}
        real = client.subprocess.run
        huge = MagicMock(returncode=0, stdout=json.dumps({"id": "lint-k7", "notes": "n" * client.MAX_REQUEST_BYTES}))
        with patch.object(client.subprocess, "run", return_value=huge):
            requests = client.assignment_requests(assignment)
        self.assertIs(client.subprocess.run, real)
        entries = [e for r in requests for e in r["state"].get("external_evidence", [])]
        self.assertEqual("".join(e["text"] for e in entries), "n" * client.MAX_REQUEST_BYTES)
        self.assertTrue(all(e["ref"] == "lint-k7" for e in entries))

    def test_each_known_error_returns_its_fix(self):
        cases = {
            ("SANITY.JEV_UNAVAILABLE", "Jev connection failed or timed out", True): "retry budget is exhausted",
            ("SANITY.JEV_UNAVAILABLE", "Jev HTTP 503; response body withheld", True): "retry budget is exhausted",
            ("SANITY.JEV_UNAVAILABLE", "Jev HTTP 401; response body withheld", False): "Report this error unchanged",
            ("VALIDATION.INPUT", "Committed evidence unreadable at the pinned commits", False): "git -C <worktree_path> fetch origin",
            ("VALIDATION.INPUT", "Request file unavailable or invalid JSON", False): "Write the file as the JSON object",
            ("SANITY.JEV_RESPONSE_INVALID", "Invalid Choice answer", False): "Report this error unchanged",
        }
        for (code, message, recoverable), fix in cases.items():
            with self.subTest(message=message):
                self.assertIn(fix, client.failure(client.JevError(code, message, recoverable))["error"]["suggested_action"])

    def test_every_failure_message_the_client_writes_is_recognised(self):
        source = (Path(__file__).parents[1] / "jev_client.py").read_text()
        import re
        literals = re.findall(r'JevError\("[A-Z_.]+", f?"([^"]+)"', source)
        self.assertGreaterEqual(len(literals), 15)
        for literal in literals:
            with self.subTest(message=literal):
                self.assertTrue(client.is_client_message(literal.replace("{status}", "503")))
        self.assertFalse(client.is_client_message("Selected model is at capacity"))


if __name__ == "__main__":
    unittest.main()
