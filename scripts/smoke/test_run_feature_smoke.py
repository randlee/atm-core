"""Unit tests for the progressive feature smoke dispatcher."""
from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
from unittest import mock
import unittest


def load_runner():
    path = Path(__file__).with_name("run_feature_smoke.py")
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location("run_feature_smoke", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()
TEST_SENDER = "test-agent"
TEST_TEAM = "test-team"


class FeatureSmokeTests(unittest.TestCase):
    def test_local_ip_alias_is_supported(self):
        with mock.patch.object(RUNNER, "run_live", return_value=0) as run_live:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "local-up"]):
                self.assertEqual(RUNNER.main(), 0)
        run_live.assert_called_once_with("local-ip", [])

    def test_crosshost_passes_all_hostnames_to_live_runner(self):
        with mock.patch.object(RUNNER, "run_live", return_value=0) as run_live:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "crosshost", "m5", "fastpc4"]):
                self.assertEqual(RUNNER.main(), 0)
        run_live.assert_called_once_with("crosshost-send", ["m5", "fastpc4"])

    def test_crosshost_ack_passes_all_hostnames_to_live_runner(self):
        with mock.patch.object(RUNNER, "run_live", return_value=0) as run_live:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "crosshost-ack", "m5"]):
                self.assertEqual(RUNNER.main(), 0)
        run_live.assert_called_once_with("crosshost-ack", ["m5"])

    def test_crosshost_curl_tls_passes_all_hostnames_to_live_runner(self):
        with mock.patch.object(RUNNER, "run_live", return_value=0) as run_live:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "crosshost-curl-tls", "m5"]):
                self.assertEqual(RUNNER.main(), 0)
        run_live.assert_called_once_with("crosshost-curl-tls", ["m5"])

    def test_crosshost_feature_requires_a_peer(self):
        with mock.patch.object(RUNNER.sys, "argv", ["smoke", "crosshost-send"]):
            with self.assertRaisesRegex(RUNNER.SmokeError, "requires one or more SSH hostnames"):
                RUNNER.main()

    def test_fixture_level_retains_existing_runner(self):
        completed = mock.Mock(returncode=0)
        with mock.patch.object(RUNNER.subprocess, "run", return_value=completed) as run:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "thorough"]):
                self.assertEqual(RUNNER.main(), 0)
        self.assertEqual(Path(run.call_args.args[0][1]).name, "run.py")

    def test_admission_capacity_reuses_the_feature_smoke_dispatcher(self):
        completed = mock.Mock(returncode=0)
        with mock.patch.object(RUNNER.subprocess, "run", return_value=completed) as run:
            with mock.patch.object(RUNNER.sys, "argv", ["smoke", "admission-capacity"]):
                self.assertEqual(RUNNER.main(), 0)
        self.assertEqual(Path(run.call_args.args[0][1]).name, "run_admission_capacity.py")

    def test_missing_identity_is_a_hard_failure(self):
        with mock.patch.dict(os.environ, {"ATM_IDENTITY": "", "ATM_TEAM": ""}, clear=False):
            with self.assertRaisesRegex(RUNNER.SmokeError, "ATM_IDENTITY"):
                RUNNER.require_environment()

    def test_branch_version_requires_one_shared_cli_daemon_version(self):
        metadata = {"packages": [{"name": "agent-team-mail", "version": "1.3.2-beta.27"}, {"name": "atm-daemon", "version": "1.3.2-beta.27"}]}
        with mock.patch.object(RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(metadata), "stderr": ""}):
            self.assertEqual(RUNNER.branch_version(), "1.3.2-beta.27")

    def test_branch_version_rejects_divergent_cli_daemon_versions(self):
        metadata = {"packages": [{"name": "agent-team-mail", "version": "1.3.2-beta.27"}, {"name": "atm-daemon", "version": "1.3.2-beta.28"}]}
        with mock.patch.object(RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(metadata), "stderr": ""}):
            with self.assertRaisesRegex(RUNNER.SmokeError, "shared"):
                RUNNER.branch_version()

    def test_branch_version_rejects_metadata_without_the_real_cli_package(self):
        metadata = {"packages": [{"name": "atm", "version": "1.3.2-beta.27"}, {"name": "atm-daemon", "version": "1.3.2-beta.27"}]}
        with mock.patch.object(RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(metadata), "stderr": ""}):
            with self.assertRaisesRegex(RUNNER.SmokeError, "shared"):
                RUNNER.branch_version()

    def test_command_redacts_captured_secret_output(self):
        completed = mock.Mock(returncode=1, stdout="token=must-not-appear", stderr="private_key=must-not-appear")
        with mock.patch("smoke_common.subprocess.run", return_value=completed):
            result = RUNNER.command(["atm", "doctor"])
        self.assertNotIn("must-not-appear", result["stdout"])
        self.assertNotIn("must-not-appear", result["stderr"])

    def test_command_preserves_a_large_doctor_json_response_for_parsing(self):
        payload = __import__("json").dumps({"roster": "x" * 9_000})
        completed = mock.Mock(returncode=0, stdout=payload, stderr="")
        with mock.patch("smoke_common.subprocess.run", return_value=completed):
            result = RUNNER.command(["atm", "doctor", "--json"])
        self.assertEqual(RUNNER.parse_json(result, "doctor")["roster"], "x" * 9_000)

    def test_remote_hosts_are_rejected_for_localhost_feature(self):
        with mock.patch.object(RUNNER.sys, "argv", ["smoke", "localhost", "m5"]):
            with self.assertRaisesRegex(RUNNER.SmokeError, "only valid"):
                RUNNER.main()

    def test_report_writes_browser_frame_in_self_contained_platform_host_run_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            with mock.patch.dict(os.environ, {"ATM_SMOKE_RUN_ID": "smoke-42"}, clear=False):
                with mock.patch.object(RUNNER, "ROOT", Path(temp)):
                    with mock.patch.object(RUNNER, "platform") as platform, mock.patch.object(
                        RUNNER, "os"
                    ) as os_module, mock.patch.object(RUNNER, "compose") as compose, mock.patch.object(
                        RUNNER, "update_master_report_index"
                    ) as update_index:
                        platform.system.return_value = "Darwin"
                        platform.node.return_value = "m5.example.test"
                        os_module.environ = os.environ
                        os_module.getpid.return_value = 4242
                        report = RUNNER.write_report(
                            "localhost",
                            [
                                {
                                    "name": "doctor",
                                    "status": "PASS",
                                    "detail": "ready",
                                    "origin": "local.example.test",
                                    "destination": "local.example.test",
                                }
                            ],
                        )
        self.assertEqual(compose.call_count, 3)
        self.assertEqual(
            report,
            Path(temp) / "site/reports/smoke/macos/m5.example.test/smoke-42-pid4242-localhost/localhost.json",
        )
        self.assertEqual(compose.call_args_list[1].args[2], report.with_suffix(".html"))
        self.assertEqual(compose.call_args_list[2].args[2], report.parent / "index.html")
        update_index.assert_called_once_with()

    def test_report_directory_includes_platform_host_and_process_qualified_run_id(self):
        with tempfile.TemporaryDirectory() as temp:
            with mock.patch.dict(os.environ, {}, clear=True), mock.patch.object(RUNNER, "ROOT", Path(temp)), mock.patch.object(
                RUNNER, "platform"
            ) as platform, mock.patch.object(RUNNER, "os") as os_module:
                platform.system.return_value = "Windows"
                platform.node.return_value = "cwin"
                os_module.environ = {}
                os_module.getpid.return_value = 99
                with mock.patch.object(RUNNER, "datetime") as datetime:
                    datetime.now.return_value.strftime.return_value = "20260808T001234567890Z"
                    directory, identity = RUNNER.smoke_report_directory("local-ip")
        self.assertEqual(
            identity,
            {
                "feature": "local-ip",
                "host": "cwin",
                "platform": "windows",
                "run_id": "20260808T001234567890Z",
            },
        )
        self.assertEqual(
            directory,
            Path(temp) / "site/reports/smoke/windows/cwin/20260808T001234567890Z-pid99-local-ip",
        )

    def test_master_index_update_uses_the_existing_generator_and_fails_closed(self):
        completed = mock.Mock(returncode=0, stdout="", stderr="")
        with mock.patch.object(RUNNER.subprocess, "run", return_value=completed) as run:
            RUNNER.update_master_report_index()
        self.assertEqual(
            run.call_args.args[0],
            [
                RUNNER.sys.executable,
                str(RUNNER.ROOT / ".just" / "generate_report_index.py"),
                "--root",
                str(RUNNER.ROOT),
            ],
        )

        failed = mock.Mock(returncode=1, stdout="", stderr="report-index: error: malformed envelope")
        with mock.patch.object(RUNNER.subprocess, "run", return_value=failed):
            with self.assertRaisesRegex(RUNNER.SmokeError, "master smoke report index"):
                RUNNER.update_master_report_index()

    def test_report_writes_metadata_and_all_rendered_outputs_beneath_its_run_directory(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)

            def compose_side_effect(_template, _variables, output):
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_text("rendered\n", encoding="utf-8")

            with mock.patch.dict(os.environ, {"ATM_SMOKE_RUN_ID": "run-1"}, clear=False), mock.patch.object(
                RUNNER, "ROOT", root
            ), mock.patch.object(RUNNER, "platform") as platform, mock.patch.object(RUNNER, "os") as os_module, mock.patch.object(
                RUNNER, "compose", side_effect=compose_side_effect
            ), mock.patch.object(RUNNER, "update_master_report_index") as update_index:
                platform.system.return_value = "Windows"
                platform.node.return_value = "cwin"
                os_module.environ = os.environ
                os_module.getpid.return_value = 7
                report = RUNNER.write_report(
                    "localhost",
                    [{"name": "doctor", "status": "PASS", "detail": "ready", "origin": "cwin", "destination": "cwin"}],
                )
            payload = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual(
                payload,
                {
                    "feature": "localhost",
                    "host": "cwin",
                    "platform": "windows",
                    "run_id": "run-1",
                    "status": "PASS",
                    "cases": [{"name": "doctor", "status": "PASS", "detail": "ready", "origin": "cwin", "destination": "cwin"}],
                },
            )
            self.assertTrue(report.with_suffix(".html").is_file())
            self.assertTrue((report.parent / "index.html").is_file())
            self.assertTrue((report.parent / "cwin-localhost.xhtml").is_file())
            envelope = json.loads((report.parent / "smoke.envelope.json").read_text(encoding="utf-8"))
            self.assertEqual(envelope["report_type"], "smoke")
            self.assertEqual(envelope["host_label"], "cwin")
            self.assertEqual(envelope["status"], "PASS")
            self.assertEqual(
                envelope["report_html"],
                "smoke/windows/cwin/run-1-pid7-localhost/index.html",
            )
            update_index.assert_called_once_with()
            self.assertFalse((root / "site" / "index.html").exists())
            self.assertFalse((root / "site" / "reports" / "localhost.json").exists())

    def test_feature_pane_renders_each_executed_case(self):
        pane = RUNNER.render_feature_pane(
            "localhost",
            [
                {
                    "name": "doctor",
                    "status": "PASS",
                    "detail": "status: healthy\nreadiness: ready",
                    "origin": "m4",
                    "destination": "m4",
                },
                {
                    "name": "localhost send/read",
                    "status": "PASS",
                    "detail": "01TEST",
                    "origin": "m4",
                    "destination": "m4",
                },
                {
                    "name": "mTLS doctor",
                    "status": "PASS",
                    "detail": "HTTP 200",
                    "origin": "m4",
                    "destination": "m5",
                },
            ],
            "m4",
        )
        self.assertIn("localhost send/read", pane)
        self.assertIn("ONE-COMPUTER TEST — m4", pane)
        self.assertIn("CROSS-HOST TEST — m4 ↔ m5", pane)
        self.assertIn("<th>Doctor</th><td>PASS 1/1</td>", pane)

    def test_host_header_reports_advertised_ip_version_and_doctor(self):
        header = RUNNER.render_host_header(
            "m4",
            [
                {"name": "doctor", "status": "PASS", "detail": "READY · ATM 1.4.0-beta-ai"},
                {"name": "advertised host", "status": "PASS", "detail": "192.0.2.10"},
            ],
        )
        self.assertIn("Advertised IP</th><td>192.0.2.10", header)
        self.assertIn("ATM version</th><td>1.4.0-beta-ai", header)
        self.assertIn("Doctor</th><td>PASS 1/1", header)

    def test_cross_host_section_reports_both_endpoint_preflights(self):
        cases = [
            {"name": "doctor", "status": "PASS", "detail": "READY · ATM 1.4.0-beta-ai", "origin": "m4", "destination": "m4"},
            {"name": "advertised host", "status": "PASS", "detail": "192.0.2.10", "origin": "m4", "destination": "m4"},
            {"name": "m5 doctor/version", "status": "PASS", "detail": "client=1.4.0-beta-ai, daemon=1.4.0-beta-ai", "origin": "m5", "destination": "m5"},
            {"name": "m5 advertised host", "status": "PASS", "detail": "192.0.2.20", "origin": "m5", "destination": "m5"},
            {"name": "mTLS doctor", "status": "PASS", "detail": "HTTP 200", "origin": "m4", "destination": "m5"},
        ]
        section = RUNNER.render_cross_host_section("CROSS-HOST TEST — m4 ↔ m5", ["m4", "m5"], cases, [cases[-1]])
        self.assertIn("IP address used", section)
        self.assertIn("192.0.2.10", section)
        self.assertIn("192.0.2.20", section)
        self.assertEqual(section.count("1.4.0-beta-ai"), 2)
        self.assertGreaterEqual(section.count("PASS"), 2)

    def test_repeated_cases_show_pass_count_and_first_failure(self):
        summarized = RUNNER.summarize_cases(
            [
                {"origin": "m4", "destination": "m5", "name": "mTLS doctor", "status": "PASS", "detail": "01"},
                {"origin": "m4", "destination": "m5", "name": "mTLS doctor", "status": "FAIL", "detail": "connection refused"},
                {"origin": "m4", "destination": "m5", "name": "mTLS doctor", "status": "PASS", "detail": "03"},
            ]
        )
        self.assertEqual(summarized[0]["status"], "FAIL")
        self.assertEqual(summarized[0]["detail"], "2/3 PASS · connection refused")

    def test_live_repetitions_default_to_ten_and_reject_invalid_values(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            self.assertEqual(RUNNER.smoke_repetitions(), 10)
        with mock.patch.dict(os.environ, {"ATM_SMOKE_REPETITIONS": "zero"}, clear=False):
            with self.assertRaisesRegex(RUNNER.SmokeError, "positive integer"):
                RUNNER.smoke_repetitions()

    def test_artifact_segment_rejects_path_traversal(self):
        with self.assertRaisesRegex(RUNNER.SmokeError, "ATM_SMOKE_RUN_ID"):
            RUNNER.artifact_segment("../other-run", "ATM_SMOKE_RUN_ID")

    def test_ack_reply_contract_requires_a_sent_reply_ulid(self):
        self.assertEqual(
            RUNNER.reply_message_id(
                {"reply_disposition": {"kind": "sent", "reply_message_id": "01TEST"}}
            ),
            "01TEST",
        )
        with self.assertRaisesRegex(RUNNER.SmokeError, "sent reply"):
            RUNNER.reply_message_id({"reply_disposition": {"kind": "failed"}})

    def test_message_has_text_requires_exact_body(self):
        self.assertTrue(RUNNER.message_has_text({"text": "exact"}, "exact"))
        self.assertFalse(RUNNER.message_has_text({"text": "different"}, "exact"))

    def test_ack_send_failure_records_only_the_reply_row(self):
        cases = []
        sent = {"exit_code": 0, "stdout": '{"message_id":"01NORMAL"}', "stderr": ""}
        required = {"exit_code": 0, "stdout": '{"message_id":"01REQUIRED"}', "stderr": ""}
        failed_ack = {"exit_code": 1, "stdout": "", "stderr": "temporary daemon failure"}
        with mock.patch.object(RUNNER, "command", side_effect=[sent, required, failed_ack]), mock.patch.object(
            RUNNER,
            "wait_for_message",
            side_effect=[
                {"message_id": "01NORMAL", "text": mock.ANY},
                {"message_id": "01REQUIRED", "text": mock.ANY, "requires_ack": True},
            ],
        ), mock.patch.object(RUNNER, "message_has_text", return_value=True):
            RUNNER.send_read_ack(cases, "atm", TEST_SENDER, TEST_TEAM, "localhost", stage="canonical-localhost")
        self.assertEqual(
            [(case["name"], case["status"]) for case in cases],
            [
                ("canonical-localhost send/read/content", "PASS"),
                ("canonical-localhost requires-ack delivery/content", "PASS"),
                ("canonical-localhost acknowledgement reply delivery/content", "FAIL"),
            ],
        )

    def test_send_read_ack_uses_explicit_host_equivalent_to_qualified_recipient(self):
        cases = []
        sent = {"exit_code": 0, "stdout": '{"message_id":"01NORMAL"}', "stderr": ""}
        required = {"exit_code": 0, "stdout": '{"message_id":"01REQUIRED"}', "stderr": ""}
        acknowledged = {
            "exit_code": 0,
            "stdout": '{"reply_disposition":{"kind":"sent","reply_message_id":"01REPLY"}}',
            "stderr": "",
        }
        with mock.patch.object(RUNNER, "command", side_effect=[sent, required, acknowledged]) as command, mock.patch.object(
            RUNNER,
            "wait_for_message",
            side_effect=[
                {"message_id": "01NORMAL", "text": "body"},
                {"message_id": "01REQUIRED", "text": "required", "requires_ack": True},
                {"message_id": "01REPLY", "text": "reply", "acknowledgesMessageId": "01REQUIRED"},
            ],
        ), mock.patch.object(RUNNER, "message_has_text", return_value=True):
            RUNNER.send_read_ack(cases, "atm", TEST_SENDER, TEST_TEAM, "localhost", stage="localhost")
        sent_command = command.call_args_list[0].args[0]
        required_command = command.call_args_list[1].args[0]
        self.assertEqual(sent_command[2], f"{TEST_SENDER}@{TEST_TEAM}")
        self.assertEqual(sent_command[sent_command.index("--host") + 1], "localhost")
        self.assertEqual(required_command[2], f"{TEST_SENDER}@{TEST_TEAM}")
        self.assertEqual(required_command[required_command.index("--host") + 1], "localhost")

    def test_localhost_live_attempt_never_discovers_or_targets_advertised_host(self):
        doctor = {
            "summary": {"status": "healthy"},
            "runtime_status": {"readiness": "ready"},
            "client_context": {"version": "1.4.1-beta-ai-1"},
            "daemon_context": {"version": "1.4.1-beta-ai-1"},
        }
        with mock.patch.object(RUNNER, "require_environment", return_value=("atm", TEST_SENDER, TEST_TEAM)), mock.patch.object(
            RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(doctor), "stderr": ""}
        ), mock.patch.object(RUNNER, "branch_version", return_value="1.4.1-beta-ai-1"), mock.patch.object(
            RUNNER, "advertised_host", side_effect=AssertionError("localhost must not discover advertised host")
        ), mock.patch.object(RUNNER, "send_read_ack") as send_read_ack:
            RUNNER.run_live_attempt(RUNNER.LOCALHOST, [])
        send_read_ack.assert_called_once_with(
            mock.ANY, "atm", TEST_SENDER, TEST_TEAM, "localhost", stage="localhost"
        )

    def test_local_ip_live_attempt_uses_enabled_advertised_address(self):
        doctor = {
            "summary": {"status": "healthy"},
            "runtime_status": {"readiness": "ready"},
            "client_context": {"version": "1.4.1-beta-ai-1"},
            "daemon_context": {"version": "1.4.1-beta-ai-1"},
        }
        with mock.patch.object(RUNNER, "require_environment", return_value=("atm", TEST_SENDER, TEST_TEAM)), mock.patch.object(
            RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(doctor), "stderr": ""}
        ), mock.patch.object(RUNNER, "branch_version", return_value="1.4.1-beta-ai-1"), mock.patch.object(
            RUNNER, "advertised_host", return_value="rand-m4.local"
        ) as advertised_host, mock.patch.object(
            RUNNER, "local_advertised_ipv4", return_value="192.0.2.10"
        ) as local_advertised_ipv4, mock.patch.object(RUNNER, "send_read_ack") as send_read_ack:
            RUNNER.run_live_attempt(RUNNER.LOCAL_IP, [])
        advertised_host.assert_called_once_with("atm")
        local_advertised_ipv4.assert_called_once_with("rand-m4.local")
        send_read_ack.assert_called_once_with(
            mock.ANY, "atm", TEST_SENDER, TEST_TEAM, "192.0.2.10", stage="local-IP"
        )

    def test_peer_preflight_rechecks_canonical_localhost_not_bare_loopback_ip(self):
        doctor = {
            "summary": {"status": "healthy"},
            "runtime_status": {"readiness": "ready"},
            "client_context": {"version": "1.4.1-beta-ai-1"},
            "daemon_context": {"version": "1.4.1-beta-ai-1"},
        }
        with mock.patch.object(RUNNER, "require_environment", return_value=("atm", TEST_SENDER, TEST_TEAM)), mock.patch.object(
            RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(doctor), "stderr": ""}
        ), mock.patch.object(RUNNER, "branch_version", return_value="1.4.1-beta-ai-1"), mock.patch.object(
            RUNNER, "advertised_host", return_value="rand-m4.local"
        ), mock.patch.object(RUNNER, "remote_context", return_value=("arch-ctm", "atm-dev")), mock.patch.object(
            RUNNER, "send_read_ack"
        ) as send_read_ack:
            RUNNER.run_live_attempt(RUNNER.PEER_PREFLIGHT, [])
        self.assertEqual(
            send_read_ack.call_args_list,
            [
                mock.call(mock.ANY, "atm", TEST_SENDER, TEST_TEAM, "rand-m4.local", stage="local-IP"),
                mock.call(mock.ANY, "atm", TEST_SENDER, TEST_TEAM, "localhost", stage="canonical-localhost"),
            ],
        )

    def test_local_advertised_ipv4_rejects_loopback_and_link_local_candidates(self):
        with mock.patch.object(
            RUNNER.socket,
            "getaddrinfo",
            return_value=[(None, None, None, None, ("169.254.1.1", 0)), (None, None, None, None, ("10.0.0.7", 0))],
        ):
            self.assertEqual(RUNNER.local_advertised_ipv4("rand-m4.local"), "10.0.0.7")

    def test_doctor_ready_requires_health_readiness_and_matching_pair(self):
        report = {
            "summary": {"status": "healthy"},
            "runtime_status": {"readiness": "ready"},
            "client_context": {"version": "1.3.2-beta.28"},
            "daemon_context": {"version": "1.3.2-beta.28"},
        }
        self.assertTrue(RUNNER.doctor_ready(report, "1.3.2-beta.28"))
        report["daemon_context"]["version"] = "1.3.2-beta.27"
        self.assertFalse(RUNNER.doctor_ready(report, "1.3.2-beta.28"))

    def test_advertised_host_from_json_requires_enabled_host(self):
        self.assertEqual(
            RUNNER.advertised_host_from_json(
                {"interfaces": [{"enabled": False, "advertise_host": "old"}, {"enabled": True, "advertise_host": "m5.local"}]}
            ),
            "m5.local",
        )
        with self.assertRaisesRegex(RUNNER.SmokeError, "no enabled"):
            RUNNER.advertised_host_from_json({"interfaces": [{"enabled": False, "advertise_host": "old"}]})

    def test_dns_resolution_uses_all_returned_addresses(self):
        with mock.patch.object(
            RUNNER.socket,
            "getaddrinfo",
            return_value=[
                (2, 1, 6, "", ("192.0.2.10", 43101)),
                (30, 1, 6, "", ("2001:db8::10", 43101, 0, 0)),
                (2, 1, 6, "", ("192.0.2.10", 43101)),
            ],
        ):
            self.assertEqual(RUNNER.resolve_dns_addresses("peer.example"), ["192.0.2.10", "2001:db8::10"])

    def test_dns_case_requires_the_advertised_ip(self):
        cases = []
        RUNNER.add_dns_case(
            cases,
            "local DNS resolves m5 peer",
            "m4",
            "m5",
            "m5.example",
            "192.0.2.20",
            lambda _hostname: ["2001:db8::20"],
        )
        self.assertEqual(cases[0]["status"], "FAIL")
        self.assertIn("missing advertised IP 192.0.2.20", cases[0]["detail"])

    def test_remote_command_supplies_configured_peer_identity(self):
        result = {"exit_code": 0, "stdout": "{}", "stderr": ""}
        with mock.patch.dict(
            os.environ,
            {"ATM_SMOKE_REMOTE_IDENTITY": "cm5:smoke", "ATM_SMOKE_REMOTE_TEAM": "atm-m5"},
            clear=False,
        ), mock.patch.object(RUNNER, "command", return_value=result) as command:
            self.assertEqual(RUNNER.remote_command("m5", "/opt/homebrew/bin/atm", ["doctor", "--json"]), result)
        self.assertEqual(
            command.call_args.args[0],
            [
                "ssh",
                "m5",
                "env",
                "ATM_IDENTITY=cm5:smoke",
                "ATM_TEAM=atm-m5",
                "/opt/homebrew/bin/atm",
                "doctor",
                "--json",
            ],
        )

    def test_curl_doctor_records_real_route_evidence_in_both_directions(self):
        doctor = {
            "summary": {"status": "healthy"},
            "runtime_status": {"readiness": "ready"},
            "client_context": {"version": "1.4.0-beta-ai"},
            "daemon_context": {"version": "1.4.0-beta-ai"},
        }
        result = {"exit_code": 0, "stdout": __import__("json").dumps(doctor), "stderr": ""}
        rejected = {"exit_code": 35, "stdout": "000", "stderr": "tls alert"}
        cases = []
        def local_command(argv, timeout=15.0):
            if argv[0] == "curl" and "--cert" not in argv:
                return rejected
            return result

        def remote_command_result(script, timeout=20.0):
            if script == "mktemp -d":
                return {"exit_code": 0, "stdout": "/private/var/folders/atm-smoke-certs-123\n", "stderr": ""}
            if script.startswith("rm -f"):
                return {"exit_code": 0, "stdout": "", "stderr": ""}
            if "curl" in script and "--cert" not in script:
                return rejected
            return result

        with mock.patch.object(RUNNER, "certificate_bundle", return_value="/tmp/local-bundle.pem"), mock.patch.object(
            RUNNER,
            "remote_command",
            return_value={
                "exit_code": 0,
                "stdout": __import__("json").dumps({"private_key_ref": "/tmp/remote-bundle.pem"}),
                "stderr": "",
            },
        ), mock.patch.object(
            RUNNER,
            "remote_shell",
            side_effect=lambda _peer, script, timeout=20.0: remote_command_result(script, timeout),
        ) as remote_shell, mock.patch.object(
            RUNNER, "command", side_effect=local_command
        ) as command, mock.patch.object(
            RUNNER, "certificate_authority", side_effect=["local.example.test", "remote.example.test"]
        ), mock.patch.object(RUNNER, "advertised_host", return_value="192.0.2.10"), mock.patch.object(
            RUNNER, "resolve_dns_addresses", return_value=["192.0.2.20"]
        ), mock.patch.object(RUNNER, "remote_resolve_dns_addresses", return_value=["192.0.2.10"]):
            RUNNER.curl_doctor(
                cases,
                "m5",
                "atm",
                "/opt/homebrew/bin/atm",
                "192.0.2.20",
                "1.4.0-beta-ai",
                plaintext=False,
            )
        self.assertEqual([case["status"] for case in cases], ["PASS"] * 7)
        curl_calls = [call.args[0] for call in command.call_args_list if call.args[0][0] == "curl"]
        self.assertEqual(len(curl_calls), 3)
        self.assertIn("--resolve", curl_calls[0])
        self.assertIn("--cert", curl_calls[0])
        self.assertNotIn("--cert", curl_calls[1])
        self.assertIn("--write-out", curl_calls[1])
        self.assertNotIn("--resolve", curl_calls[2])
        self.assertIn("https://remote.example.test:43101/v1/atm/doctor", curl_calls[2])
        local_ca_path = curl_calls[0][curl_calls[0].index("--cacert") + 1]
        self.assertEqual(Path(local_ca_path).name, "remote-public.pem")
        self.assertTrue(any("unauthenticated mTLS" in case["name"] for case in cases))
        cleanup_script = remote_shell.call_args_list[-1].args[1]
        self.assertIn("rm -f", cleanup_script)
        self.assertIn("local-public.pem", cleanup_script)
        self.assertIn("peer-public.pem", cleanup_script)
        self.assertIn("rmdir", cleanup_script)

    def test_mtls_rejection_requires_nonzero_curl_exit_and_no_http_status(self):
        self.assertTrue(RUNNER.mtls_rejected_before_http({"exit_code": 35, "stdout": "000", "stderr": ""}))
        self.assertFalse(RUNNER.mtls_rejected_before_http({"exit_code": 0, "stdout": "000", "stderr": ""}))
        self.assertFalse(RUNNER.mtls_rejected_before_http({"exit_code": 22, "stdout": "401", "stderr": ""}))

    def test_crosshost_send_requires_remote_exact_ulid_and_body(self):
        sent = {"message_id": "01SEND"}
        remote_read = {"message": {"message_id": "01SEND", "text": "smoke-crosshost-send-m5-STAMP"}}
        cases = []

        class Clock:
            @staticmethod
            def now(_timezone):
                return type("FixedTime", (), {"strftime": lambda self, _format: "STAMP"})()

        with mock.patch.object(RUNNER, "command", return_value={"exit_code": 0, "stdout": __import__("json").dumps(sent), "stderr": ""}), mock.patch.object(
            RUNNER, "remote_command", side_effect=lambda _peer, _atm, args, timeout=20.0: {
                "exit_code": 0,
                "stdout": __import__("json").dumps(remote_read if args[0] == "read" else sent),
                "stderr": "",
            }
        ), mock.patch.object(
            RUNNER,
            "wait_for_message",
            return_value={"message_id": "01SEND", "text": "smoke-crosshost-send-reverse-m5-STAMP"},
        ), mock.patch.object(RUNNER, "datetime", Clock):
            RUNNER.crosshost_send(
                cases,
                "atm",
                "atm",
                "agent",
                "team",
                "peer",
                "peer-team",
                "m5",
                "m5.local",
                "127.0.0.1",
            )
        self.assertEqual([case["status"] for case in cases], ["PASS", "PASS"])
        self.assertEqual(cases[0]["detail"], "01SEND")
        self.assertEqual(cases[1]["detail"], "01SEND")

    def test_crosshost_ack_verifies_reverse_reply_linkage(self):
        sent = {"message_id": "01SEND"}
        remote_read = {
            "message": {
                "message_id": "01SEND",
                "text": "smoke-crosshost-ack-m5-STAMP",
                "requires_ack": True,
            }
        }
        remote_ack = {"reply_disposition": {"kind": "sent", "reply_message_id": "01REPLY"}}
        cases = []
        remote_ack_teams = []

        class Clock:
            @staticmethod
            def now(_timezone):
                return type("FixedTime", (), {"strftime": lambda self, _format: "STAMP"})()

        def local_command(argv, timeout=15.0):
            response = remote_ack if argv[1] == "ack" else sent
            return {"exit_code": 0, "stdout": __import__("json").dumps(response), "stderr": ""}

        def remote(peer, atm, args, timeout=20.0):
            if args[0] == "read":
                response = remote_read if args[args.index("--message-id") + 1] == "01SEND" else {
                    "message": {
                        "message_id": "01REPLY",
                        "text": "smoke-crosshost-reverse-reply-m5-STAMP",
                        "acknowledgesMessageId": "01SEND",
                    }
                }
            elif args[0] == "ack":
                remote_ack_teams.append(args[args.index("--team") + 1])
                response = remote_ack
            else:
                response = sent
            return {"exit_code": 0, "stdout": __import__("json").dumps(response), "stderr": ""}

        def local_reply(atm, team, expected, timeout=12.0):
            if expected == "01REPLY":
                return {
                    "message_id": expected,
                    "text": "smoke-crosshost-reply-m5-STAMP",
                    "acknowledgesMessageId": "01SEND",
                }
            return {
                "message_id": expected,
                "text": "smoke-crosshost-ack-reverse-m5-STAMP",
                "requires_ack": True,
            }

        with mock.patch.object(RUNNER, "command", side_effect=local_command), mock.patch.object(
            RUNNER, "remote_command", side_effect=remote
        ), mock.patch.object(RUNNER, "wait_for_message", side_effect=local_reply), mock.patch.object(
            RUNNER, "datetime", Clock
        ):
            RUNNER.crosshost_ack(
                cases,
                "atm",
                "atm",
                "agent",
                "team",
                "peer",
                "peer-team",
                "m5",
                "m5.local",
                "127.0.0.1",
            )
        self.assertEqual([case["status"] for case in cases], ["PASS", "PASS", "PASS", "PASS"])
        self.assertEqual(cases[1]["detail"], "01REPLY")
        self.assertEqual(remote_ack_teams, ["peer-team"])


if __name__ == "__main__":
    unittest.main()
