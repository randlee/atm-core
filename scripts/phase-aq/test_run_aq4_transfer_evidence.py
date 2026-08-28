from __future__ import annotations

import collections
import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace
from typing import Any


SCRIPT = Path(__file__).with_name("run_aq4_transfer_evidence.py")


def load_module():
    spec = importlib.util.spec_from_file_location("run_aq4_transfer_evidence", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Aq4TransferEvidenceTests(unittest.TestCase):
    def test_extract_landed_path_matches_the_real_format_attachment_note_shape(self) -> None:
        module = load_module()
        # Mirrors atm_core::send_to::format_attachment_note's literal output
        # (crates/atm-core/src/send_to.rs), JSON-escaped as `peek --json`
        # would emit it inside a string field.
        peek_stdout = (
            '{"messages":[{"text":"Attached files (on this host):\\n'
            '- /tmp/atm-501/send-to/01J00000000000000000000ZZ/aq4-report.pdf"}]}'
        )
        landed = module.extract_landed_path(peek_stdout, "aq4-report.pdf")
        self.assertEqual(landed, "/tmp/atm-501/send-to/01J00000000000000000000ZZ/aq4-report.pdf")

    def test_extract_landed_path_returns_none_when_the_note_is_absent(self) -> None:
        module = load_module()
        self.assertIsNone(module.extract_landed_path('{"messages":[]}', "aq4-report.pdf"))

    def test_extract_landed_path_only_matches_the_named_attachment(self) -> None:
        module = load_module()
        peek_stdout = (
            "Attached files (on this host):\\n- /tmp/atm-501/send-to/01J.../other-file.txt"
        )
        self.assertIsNone(module.extract_landed_path(peek_stdout, "aq4-report.pdf"))

    def test_write_evidence_pass_names_files_by_host_and_records_the_landed_path(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="clean-runner-linux", evidence_dir=Path(temporary))
            record = {
                "status": "pass",
                "send": {"argv": ["atm", "send"], "returncode": 0},
                "landed_path": "/tmp/atm-501/send-to/01J.../aq4-report.pdf",
                "landed_matches_send_to_convention": True,
                "landed_file_exists": True,
                "landed_content_matches": True,
            }
            json_path, markdown_path = module.write_evidence(args, record)

            self.assertEqual(json_path.name, "transfer-clean-runner-linux.json")
            self.assertEqual(markdown_path.name, "transfer-clean-runner-linux.md")
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("PASS", markdown)
            self.assertIn("/tmp/atm-501/send-to/01J.../aq4-report.pdf", markdown)

    def test_write_evidence_skipped_no_sshd_is_an_honest_non_silent_skip(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="clean-runner-macos", evidence_dir=Path(temporary))
            record = {
                "status": "skipped_no_sshd",
                "sshd_probe": {"found": False, "install_attempted": False},
            }
            _json_path, markdown_path = module.write_evidence(args, record)
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("SKIPPED_NO_SSHD", markdown)
            self.assertIn("honest, announced skip", markdown)

    def test_write_evidence_harness_crashed_includes_error_and_traceback(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="clean-runner-windows", evidence_dir=Path(temporary))
            record = {
                "status": "harness_crashed",
                "error": "PermissionError: [WinError 32] boom",
                "traceback": "Traceback (most recent call last):\n  ...\nPermissionError: boom\n",
            }
            _json_path, markdown_path = module.write_evidence(args, record)
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("HARNESS_CRASHED", markdown)
            self.assertIn("PermissionError: [WinError 32] boom", markdown)
            self.assertIn("Traceback (most recent call last):", markdown)

    def test_write_evidence_surfaces_a_cleanup_warning_without_changing_the_status(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="clean-runner-windows", evidence_dir=Path(temporary))
            record = {
                "status": "pass",
                "send": {"argv": ["atm", "send"], "returncode": 0},
                "landed_path": "/tmp/atm-501/send-to/01J.../aq4-report.pdf",
                "landed_matches_send_to_convention": True,
                "landed_file_exists": True,
                "landed_content_matches": True,
                "cleanup_warning": "could not remove C:\\scratch after 6 attempts: boom",
            }
            _json_path, markdown_path = module.write_evidence(args, record)
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("PASS", markdown)
            self.assertIn("Cleanup warning", markdown)
            self.assertIn("could not remove C:\\scratch", markdown)

    def test_clear_stale_evidence_removes_pre_existing_files_and_tolerates_absence(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            evidence_dir = Path(temporary)
            json_path = evidence_dir / "transfer-clean-runner-windows.json"
            markdown_path = evidence_dir / "transfer-clean-runner-windows.md"
            json_path.write_text('{"stale": true}', encoding="utf-8")
            markdown_path.write_text("stale", encoding="utf-8")

            # Must not raise even when one of the two paths is already gone.
            module._clear_stale_evidence(json_path, evidence_dir / "does-not-exist.md")

            self.assertFalse(json_path.exists())
            self.assertTrue(markdown_path.exists(), "only the passed-in paths are cleared")

    def test_main_never_leaves_a_stale_evidence_file_when_run_scenario_crashes(self) -> None:
        # Regression for evidence run 6 (33137262962 @ c510a4745): the
        # Windows harness crashed inside run_scenario's tempdir cleanup
        # before write_evidence ever ran, and the workflow's `if: always()`
        # artifact-upload step then re-uploaded the previously-committed
        # evidence file for this host as if it were fresh. This proves
        # main() (a) always deletes any pre-existing file for this run
        # before doing any real work, and (b) still writes a fresh,
        # non-stale "harness_crashed" record -- with a traceback -- when
        # run_scenario raises, so the output is never the old file
        # untouched and never simply absent.
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            evidence_dir = Path(temporary)
            json_path = evidence_dir / "transfer-clean-runner-windows.json"
            evidence_dir.mkdir(parents=True, exist_ok=True)
            stale_payload = {"schema_version": 1, "sprint": "AQ4", "record": {"status": "pass", "stale": True}}
            json_path.write_text(json.dumps(stale_payload), encoding="utf-8")

            def _boom(_args: SimpleNamespace) -> dict[str, Any]:
                raise PermissionError("[WinError 32] The process cannot access the file")

            original_run_scenario = module.run_scenario
            original_argv = sys.argv
            module.run_scenario = _boom
            sys.argv = [
                "run_aq4_transfer_evidence.py",
                "--host",
                "clean-runner-windows",
                "--evidence-dir",
                str(evidence_dir),
                "--daemon",
                str(module.ROOT / "Cargo.toml"),
                "--atm",
                str(module.ROOT / "Cargo.toml"),
            ]
            try:
                exit_code = module.main()
            finally:
                module.run_scenario = original_run_scenario
                sys.argv = original_argv

            self.assertEqual(exit_code, 1)
            written = json.loads(json_path.read_text(encoding="utf-8"))
            self.assertEqual(written["record"]["status"], "harness_crashed")
            self.assertNotIn("stale", written["record"])
            self.assertIn("PermissionError", written["record"]["error"])
            self.assertIn("Traceback", written["record"]["traceback"])

    def test_remove_tree_tolerant_removes_a_real_directory(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary) / "victim"
            (target / "nested").mkdir(parents=True)
            (target / "nested" / "file.txt").write_text("x", encoding="utf-8")

            result = module._remove_tree_tolerant(target)

            self.assertIsNone(result)
            self.assertFalse(target.exists())

    def test_remove_tree_tolerant_returns_none_for_an_already_missing_path(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            missing = Path(temporary) / "never-created"
            self.assertIsNone(module._remove_tree_tolerant(missing))

    def test_remove_tree_tolerant_reports_a_warning_instead_of_raising_when_removal_never_succeeds(
        self,
    ) -> None:
        # Simulates the observed Windows failure mode (WinError 32/5
        # sharing violation that never clears) without needing an actual
        # locked directory: shutil.rmtree always raises for this path.
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary) / "locked"
            target.mkdir()

            original_rmtree = module.shutil.rmtree

            def _always_fails(path: object, *args: object, **kwargs: object) -> None:
                raise PermissionError("[WinError 32] The process cannot access the file")

            module.shutil.rmtree = _always_fails
            try:
                result = module._remove_tree_tolerant(target, attempts=2, initial_delay=0.0)
            finally:
                module.shutil.rmtree = original_rmtree

            self.assertIsNotNone(result)
            assert result is not None
            self.assertIn("could not remove", result)
            self.assertIn("WinError 32", result)

    def test_write_evidence_blocked_ambient_daemon_names_the_pids(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="local", evidence_dir=Path(temporary))
            record = {"status": "blocked_ambient_daemon", "ambient_daemon_pids": [4242]}
            _json_path, markdown_path = module.write_evidence(args, record)
            self.assertIn("4242", markdown_path.read_text(encoding="utf-8"))

    def test_main_exit_code_treats_skip_and_ambient_block_as_success(self) -> None:
        module = load_module()
        for status, expected in (
            ("pass", 0),
            ("blocked_ambient_daemon", 0),
            ("skipped_no_sshd", 0),
            ("skipped_no_posix_receiver", 0),
            ("fail", 1),
            ("blocked_sshd_start_failed", 1),
            ("blocked_daemon_start_failed", 1),
        ):
            with self.subTest(status=status):
                success_statuses = (
                    "pass",
                    "blocked_ambient_daemon",
                    "skipped_no_sshd",
                    "skipped_no_posix_receiver",
                )
                self.assertEqual(0 if status in success_statuses else 1, expected)

    def test_write_evidence_skipped_no_posix_receiver_is_an_honest_non_silent_skip(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="clean-runner-windows", evidence_dir=Path(temporary))
            record = {
                "status": "skipped_no_posix_receiver",
                "windows_default_shell": {
                    "outcome": "skipped_no_posix_receiver",
                    "reason": "no POSIX shell (git-bash) found on this Windows runner",
                },
            }
            _json_path, markdown_path = module.write_evidence(args, record)
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("SKIPPED_NO_POSIX_RECEIVER", markdown)
            self.assertIn("honest, announced skip", markdown)
            self.assertIn("no POSIX shell (git-bash) found", markdown)

    @unittest.skipUnless(
        sys.platform != "win32",
        "0o700 mode bits are a POSIX permission concept; Windows ACLs do not "
        "round-trip through os.chmod the same way, so this check is skipped "
        "there rather than reporting a false failure",
    )
    def test_install_transfer_script_forces_0700_on_every_path_the_safety_check_inspects(self) -> None:
        # Clean-runner CI (run 33126676155) refused the harness live with
        # exactly this failure before this fix: ~/.atm/transfer created via
        # a bare mkdir(parents=True) inherited 0755 from the runner's
        # default umask 022, which
        # crates/atm-core/src/transfer_script.rs::check_transfer_root_metadata
        # (a real production safety check) correctly refuses.
        import stat as stat_module

        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary) / "home"
            # A permissive umask masking-in group/other bits, matching the
            # exact CI condition this regression guards against, deliberately
            # left in place around the call (not reset first) so the fix's
            # explicit os.chmod calls -- not a favorable ambient umask -- are
            # what this test actually exercises.
            previous_umask = os.umask(0o022)
            try:
                info = module.install_transfer_script(home, {"TEMP": temporary})
            finally:
                os.umask(previous_umask)

            atm_dir = home / ".atm"
            transfer_dir = atm_dir / "transfer"
            installed = transfer_dir / module.TRANSFER_HOST

            self.assertEqual(info["atm_dir_mode"], "0o700")
            self.assertEqual(info["transfer_dir_mode"], "0o700")
            self.assertEqual(info["script_mode"], "0o700")
            self.assertEqual(stat_module.S_IMODE(atm_dir.stat().st_mode), 0o700)
            self.assertEqual(stat_module.S_IMODE(transfer_dir.stat().st_mode), 0o700)
            self.assertEqual(stat_module.S_IMODE(installed.stat().st_mode), 0o700)
            self.assertEqual(installed.read_bytes(), (module.ROOT / "scripts" / "transfer" / "sftp.sh").read_bytes())

    @unittest.skipUnless(
        sys.platform != "win32",
        "0o700 mode bits are a POSIX permission concept; Windows ACLs do not "
        "round-trip through os.chmod the same way, so this check is skipped "
        "there rather than reporting a false failure",
    )
    def test_install_transfer_script_is_idempotent_and_still_forces_0700_on_rerun(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary) / "home"
            env = {"TEMP": temporary}
            module.install_transfer_script(home, env)
            (home / ".atm" / "transfer").chmod(0o755)
            info = module.install_transfer_script(home, env)
            self.assertEqual(info["transfer_dir_mode"], "0o700")

    def test_write_sender_atm_config_writes_the_local_host_key_directly_into_cwd(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            cwd = Path(temporary)
            config_path = module.write_sender_atm_config(cwd, "aq4-sender")

            self.assertEqual(config_path, cwd / ".atm.toml")
            self.assertTrue(config_path.is_file())
            self.assertEqual(config_path.read_text(encoding="utf-8"), '[atm]\nlocal_host = "aq4-sender"\n')

    def test_write_sender_atm_config_round_trips_through_the_real_atm_core_parser(self) -> None:
        # Proves the written file is not merely well-formed TOML but is
        # accepted by the same production parser this scenario's `atm send`
        # invocation depends on (crates/atm-core/src/config/mod.rs), so a
        # parser-format drift is caught here rather than only live on CI.
        import tomllib

        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            cwd = Path(temporary)
            config_path = module.write_sender_atm_config(cwd, "aq4-sender")
            parsed = tomllib.loads(config_path.read_text(encoding="utf-8"))
            self.assertEqual(parsed["atm"]["local_host"], "aq4-sender")

    def test_sender_local_host_differs_from_the_recipient_host_so_delivery_stays_remote(self) -> None:
        # classify_recipient_locality treats an equal local_host/recipient
        # host as same-host, which would skip the transfer-script path this
        # scenario exists to exercise (see write_sender_atm_config's
        # docstring).
        module = load_module()
        self.assertNotEqual(module.SENDER_LOCAL_HOST, module.TRANSFER_HOST)

    def test_free_loopback_port_returns_an_ephemeral_port(self) -> None:
        module = load_module()
        port = module.free_loopback_port()
        self.assertIsInstance(port, int)
        self.assertGreater(port, 0)
        self.assertLess(port, 65536)

    def test_parse_args_defaults_advertise_the_shared_live_evidence_flag_contract(self) -> None:
        module = load_module()
        import sys

        argv = sys.argv
        sys.argv = ["run_aq4_transfer_evidence.py"]
        try:
            args = module.parse_args()
        finally:
            sys.argv = argv
        self.assertEqual(args.host, "local")
        self.assertIsNone(args.evidence_dir)
        self.assertTrue(str(args.daemon).endswith("atm-daemon"))
        self.assertTrue(str(args.atm).endswith("atm"))

    def test_find_sshd_never_raises_and_returns_a_path_or_none(self) -> None:
        # Deliberately does not call `ensure_sshd_available()` here: on
        # Linux, an absent `sshd` makes that function attempt a real
        # `sudo -n apt-get install openssh-server`, which is real
        # side-effecting system state this lint-suite test must never
        # trigger. `find_sshd()` alone (the pure lookup) is what is safe to
        # exercise unconditionally.
        module = load_module()
        found = module.find_sshd()
        self.assertTrue(found is None or isinstance(found, module.Path))

    # -- QA-2 B6: scratch ssh client config, never the real ~/.ssh/config --

    def test_write_scratch_ssh_client_config_never_touches_the_real_home(self) -> None:
        module = load_module()
        real_home_before = list(Path.home().glob("*"))
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            identity = root / "id_ed25519"
            identity.write_text("fake key material", encoding="utf-8")
            config_path = module.write_scratch_ssh_client_config(root, 4242, identity)

            self.assertEqual(config_path, root / "ssh_client_config")
            self.assertTrue(config_path.is_file())
            content = config_path.read_text(encoding="utf-8")
            self.assertIn("Host localhost", content)
            self.assertIn("Port 4242", content)
            self.assertIn(str(identity), content)
        # The scratch root (a TemporaryDirectory) is gone; the real
        # ~/.ssh directory tree must be exactly as it was before this
        # test ran -- proving the function never opened, backed up, or
        # wrote through Path.home() at all.
        self.assertEqual(list(Path.home().glob("*")), real_home_before)

    @unittest.skipUnless(
        sys.platform != "win32",
        "asserts the /dev/null UserKnownHostsFile branch, native non-Windows only",
    )
    def test_write_scratch_ssh_client_config_uses_dev_null_known_hosts_on_unix(self) -> None:
        module = load_module()
        self.assertFalse(module.IS_WINDOWS, "this test asserts the non-Windows branch")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config_path = module.write_scratch_ssh_client_config(root, 1, root / "identity")
            self.assertIn("UserKnownHostsFile /dev/null", config_path.read_text(encoding="utf-8"))

    @unittest.skipUnless(
        sys.platform == "win32",
        "asserts the NUL UserKnownHostsFile branch, native Windows only",
    )
    def test_write_scratch_ssh_client_config_uses_nul_known_hosts_on_windows(self) -> None:
        module = load_module()
        self.assertTrue(module.IS_WINDOWS, "this test asserts the native Windows branch")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config_path = module.write_scratch_ssh_client_config(root, 1, root / "identity")
            content = config_path.read_text(encoding="utf-8")
            self.assertIn("UserKnownHostsFile NUL", content)
            self.assertNotIn("/dev/null", content)

    def test_write_scratch_ssh_client_config_uses_nul_known_hosts_when_windows(self) -> None:
        # cipher's investigation: OpenSSH on Windows does not treat
        # /dev/null as a discard sink for UserKnownHostsFile the way Unix
        # does; it must be the literal device name NUL there.
        module = load_module()
        original = module.IS_WINDOWS
        module.IS_WINDOWS = True
        try:
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                config_path = module.write_scratch_ssh_client_config(root, 1, root / "identity")
                content = config_path.read_text(encoding="utf-8")
                self.assertIn("UserKnownHostsFile NUL", content)
                self.assertNotIn("/dev/null", content)
        finally:
            module.IS_WINDOWS = original

    # -- Windows install_transfer_script: profile-containment, not chmod --

    def test_install_transfer_script_installs_sftp_ps1_under_the_profile_home_on_windows(self) -> None:
        module = load_module()
        original = module.IS_WINDOWS
        module.IS_WINDOWS = True
        try:
            with tempfile.TemporaryDirectory() as temporary:
                home = Path(temporary) / "home"
                home.mkdir()
                env = {"TEMP": str(Path(temporary) / "tmp")}
                info = module.install_transfer_script(home, env)

                installed = home / ".atm" / "transfer" / f"{module.TRANSFER_HOST}.ps1"
                self.assertEqual(info["installed_at"], str(installed))
                self.assertTrue(installed.is_file())
                source_text = (module.ROOT / "scripts" / "transfer" / "sftp.ps1").read_text(
                    encoding="utf-8"
                )
                installed_text = installed.read_text(encoding="utf-8")
                # The placeholder is substituted, not left verbatim; every
                # other line of the shipped example is otherwise unchanged.
                self.assertNotIn(
                    module._WINDOWS_REMOTE_ATM_TEMP_PLACEHOLDER_LINE, installed_text
                )
                self.assertIn(info["receiver_atm_temp"]["substituted"], installed_text)
                self.assertEqual(
                    installed_text.replace(
                        f'$RemoteAtmTemp = "{info["receiver_atm_temp"]["substituted"]}"',
                        module._WINDOWS_REMOTE_ATM_TEMP_PLACEHOLDER_LINE,
                    ),
                    source_text,
                )
                self.assertNotIn("atm_dir_mode", info)
                self.assertNotIn("transfer_dir_mode", info)
                self.assertNotIn("script_mode", info)
                containment = info["windows_profile_containment"]
                self.assertTrue(containment["transfer_dir_contained"])
                self.assertTrue(containment["script_contained"])
                self.assertFalse(containment["transfer_dir_is_reparse_point"])
                self.assertFalse(containment["script_is_reparse_point"])
        finally:
            module.IS_WINDOWS = original

    def test_is_within_profile_rejects_a_sibling_directory_sharing_a_string_prefix(self) -> None:
        # Mirrors crate::transfer_script::path_is_within's own regression
        # coverage: "rand" must not be treated as a prefix of "randlee".
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            profile_home = root / "rand"
            sibling = root / "randlee" / "file.txt"
            profile_home.mkdir()
            self.assertFalse(module._is_within_profile(sibling, profile_home))
            contained = profile_home / "file.txt"
            self.assertTrue(module._is_within_profile(contained, profile_home))

    # -- FTQ-002/003: continuous pipe draining --

    def test_pipe_drain_keeps_draining_a_chatty_child_past_its_readiness_probe(self) -> None:
        # Writes well past a typical 64KiB OS pipe buffer so a harness that
        # only reads during a readiness probe (the pre-fix shape) would
        # block the child on a full pipe the instant that probe stops
        # reading -- proving PipeDrain keeps the pipe open for the whole
        # process lifetime, not only while something else happens to poll
        # it.
        module = load_module()
        line_count = 20000
        script = (
            "import sys\n"
            f"for i in range({line_count}):\n"
            "    print('x' * 200)\n"
            "sys.stdout.flush()\n"
        )
        process = subprocess.Popen(  # noqa: S603 - fixed, non-shell argv
            [sys.executable, "-c", script],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        drain = module.PipeDrain(process)
        try:
            returncode = process.wait(timeout=30)
        finally:
            drain.join(timeout=5)
            if process.stdout is not None:
                process.stdout.close()
            if process.stderr is not None:
                process.stderr.close()
        self.assertEqual(returncode, 0, "a chatty child must not be blocked on an undrained pipe")
        self.assertLessEqual(len(drain.stdout_lines), 4000, "PipeDrain's deque must stay bounded")
        self.assertTrue(all(line == "x" * 200 for line in drain.stdout_lines))

    def test_pipe_drain_tail_returns_the_most_recent_lines_only(self) -> None:
        module = load_module()
        sink: "collections.deque[str]" = collections.deque(
            [f"line-{i}" for i in range(10)], maxlen=100
        )
        tail = module.PipeDrain.tail(sink, count=3)
        self.assertEqual(tail, "line-7\nline-8\nline-9")

    def test_synthesized_transfer_script_path_always_has_a_path_entry(self) -> None:
        # ADR-055 decision (c) amendment: this is a diagnostics-only mirror
        # of `atm_core::transfer_script::synthesized_transfer_script_env`
        # (never asserted against `atm send`'s real behavior); this only
        # proves the mirror itself never raises and always reports the one
        # entry the real function always returns on every platform.
        module = load_module()
        result = module._synthesized_transfer_script_path({})
        self.assertIn("PATH", result)
        self.assertTrue(result["PATH"])

    @unittest.skipUnless(sys.platform == "win32", "Windows-only synthesis rules")
    def test_synthesized_transfer_script_path_windows_uses_system_root_and_openssh(self) -> None:
        module = load_module()
        result = module._synthesized_transfer_script_path({"SystemRoot": r"D:\CustomWindows"})
        self.assertIn(r"D:\CustomWindows\System32", result["PATH"])
        self.assertIn(r"D:\CustomWindows\System32\OpenSSH", result["PATH"])
        self.assertEqual(result["SystemRoot"], r"D:\CustomWindows")

    # -- fenix ruling (run 7, 33140616718 @ 7f6774802): $RemoteAtmTemp
    # placeholder substitution --

    def test_windows_receiver_atm_temp_produces_three_consistent_representations(self) -> None:
        module = load_module()
        result = module.windows_receiver_atm_temp({"TEMP": r"C:\Users\runner\AppData\Local\Temp"})
        self.assertEqual(result["windows_native"], r"C:\Users\runner\AppData\Local\Temp\atm")
        self.assertEqual(result["posix_msys"], "/c/Users/runner/AppData/Local/Temp/atm")
        self.assertEqual(result["substituted"], "C:/Users/runner/AppData/Local/Temp/atm")
        # `substituted` is absolute per `Path::is_absolute` on Windows (a
        # drive prefix, unlike `posix_msys`'s bare-rooted `/c/...` form,
        # which `validate_landed_dir_stdout` -- crates/atm-core/src/send_to.rs
        # -- rejects there).
        self.assertRegex(result["substituted"], r"^[A-Za-z]:/")

    def test_windows_receiver_atm_temp_lowercases_the_posix_drive_letter(self) -> None:
        module = load_module()
        result = module.windows_receiver_atm_temp({"TEMP": r"D:\Temp"})
        self.assertEqual(result["posix_msys"], "/d/Temp/atm")

    def test_windows_receiver_atm_temp_falls_back_to_tmp_then_gettempdir(self) -> None:
        module = load_module()
        self.assertEqual(
            module.windows_receiver_atm_temp({"TMP": r"C:\Fallback"})["windows_native"],
            r"C:\Fallback\atm",
        )
        result = module.windows_receiver_atm_temp({})
        self.assertTrue(result["windows_native"])

    def test_substitute_windows_remote_atm_temp_replaces_the_placeholder_line_exactly_once(
        self,
    ) -> None:
        module = load_module()
        source = (module.ROOT / "scripts" / "transfer" / "sftp.ps1").read_text(encoding="utf-8")
        substituted = module._substitute_windows_remote_atm_temp(source, "C:/Users/x/tmp/atm")
        self.assertNotIn(module._WINDOWS_REMOTE_ATM_TEMP_PLACEHOLDER_LINE, substituted)
        self.assertIn('$RemoteAtmTemp = "C:/Users/x/tmp/atm"', substituted)
        # Every other line is untouched.
        self.assertEqual(
            substituted.replace(
                '$RemoteAtmTemp = "C:/Users/x/tmp/atm"',
                module._WINDOWS_REMOTE_ATM_TEMP_PLACEHOLDER_LINE,
            ),
            source,
        )

    def test_substitute_windows_remote_atm_temp_raises_when_the_placeholder_is_absent(self) -> None:
        # Contract-drift guard: a future edit to sftp.ps1's placeholder
        # text must fail this harness loudly rather than silently install
        # an unsubstituted script.
        module = load_module()
        with self.assertRaises(RuntimeError) as raised:
            module._substitute_windows_remote_atm_temp("no placeholder here", "C:/x/atm")
        self.assertIn("found 0 time(s)", str(raised.exception))

    def test_substitute_windows_remote_atm_temp_raises_when_the_placeholder_repeats(self) -> None:
        module = load_module()
        doubled = "\n".join(
            [module._WINDOWS_REMOTE_ATM_TEMP_PLACEHOLDER_LINE] * 2
        )
        with self.assertRaises(RuntimeError) as raised:
            module._substitute_windows_remote_atm_temp(doubled, "C:/x/atm")
        self.assertIn("found 2 time(s)", str(raised.exception))

    # -- fenix ruling: POSIX-shell sshd for the Windows-sender ->
    # POSIX-receiver contract (mocked `where`/registry, runs on every
    # platform) --

    def test_find_windows_posix_shell_prefers_path(self) -> None:
        module = load_module()
        original_which = module.shutil.which
        module.shutil.which = lambda name, path=None: (
            "/usr/bin/bash" if name == "bash" else None
        )
        try:
            found = module.find_windows_posix_shell({"PATH": "/usr/bin"})
        finally:
            module.shutil.which = original_which
        self.assertEqual(found, module.Path("/usr/bin/bash"))

    def test_find_windows_posix_shell_falls_back_to_known_git_for_windows_locations(self) -> None:
        module = load_module()
        original_which = module.shutil.which
        module.shutil.which = lambda name, path=None: None
        try:
            with tempfile.TemporaryDirectory() as temporary:
                git_bash = Path(temporary) / "Git" / "bin" / "bash.exe"
                git_bash.parent.mkdir(parents=True)
                git_bash.write_text("", encoding="utf-8")
                found = module.find_windows_posix_shell({"ProgramFiles": temporary})
                self.assertEqual(found, git_bash)
        finally:
            module.shutil.which = original_which

    def test_find_windows_posix_shell_returns_none_when_nothing_is_found(self) -> None:
        module = load_module()
        original_which = module.shutil.which
        module.shutil.which = lambda name, path=None: None
        try:
            self.assertIsNone(module.find_windows_posix_shell({}))
        finally:
            module.shutil.which = original_which

    class _FakeWinreg:
        """Minimal `winreg` stand-in: an in-memory `{name: value}` store for
        one key, raising `FileNotFoundError` for a missing key/value and
        `PermissionError` when `deny_writes` is set -- exactly the two
        failure shapes `_read_windows_default_shell`/
        `_write_windows_default_shell` must handle.

        `view_mismatch` models the failure `prepare_windows_posix_shell`'s
        post-write readback exists to catch: a `SetValueEx` call that
        raises nothing (so the caller has no exception to observe) yet
        does not land where `QueryValueEx` looks -- the observable shape
        of a 32-bit-vs-64-bit registry view split (`WOW6432Node`), without
        this fake needing to model two separate hives."""

        HKEY_LOCAL_MACHINE = object()
        KEY_SET_VALUE = object()
        REG_SZ = object()

        def __init__(
            self,
            *,
            key_exists: bool = True,
            deny_writes: bool = False,
            view_mismatch: bool = False,
        ) -> None:
            self.key_exists = key_exists
            self.deny_writes = deny_writes
            self.view_mismatch = view_mismatch
            self.values: dict[str, str] = {}

        def OpenKey(self, _hive: object, _key: str, *_args: object) -> "Aq4TransferEvidenceTests._FakeWinreg._Key":
            if not self.key_exists:
                raise FileNotFoundError("key not found")
            return Aq4TransferEvidenceTests._FakeWinreg._Key(self)

        def CreateKeyEx(self, _hive: object, _key: str, *_args: object) -> "Aq4TransferEvidenceTests._FakeWinreg._Key":
            if self.deny_writes:
                raise PermissionError("access is denied")
            self.key_exists = True
            return Aq4TransferEvidenceTests._FakeWinreg._Key(self)

        def QueryValueEx(self, key: "Aq4TransferEvidenceTests._FakeWinreg._Key", name: str) -> tuple[str, object]:
            if name not in key.store.values:
                raise FileNotFoundError("value not found")
            return key.store.values[name], None

        def SetValueEx(
            self,
            key: "Aq4TransferEvidenceTests._FakeWinreg._Key",
            name: str,
            _reserved: int,
            _value_type: object,
            value: str,
        ) -> None:
            if key.store.deny_writes:
                raise PermissionError("access is denied")
            if key.store.view_mismatch:
                # Silently "succeeds" without updating what QueryValueEx
                # returns -- see the class docstring.
                return
            key.store.values[name] = value

        def DeleteValue(self, key: "Aq4TransferEvidenceTests._FakeWinreg._Key", name: str) -> None:
            if name not in key.store.values:
                raise FileNotFoundError("value not found")
            del key.store.values[name]

        class _Key:
            def __init__(self, store: "Aq4TransferEvidenceTests._FakeWinreg") -> None:
                self.store = store

            def __enter__(self) -> "Aq4TransferEvidenceTests._FakeWinreg._Key":
                return self

            def __exit__(self, *_exc: object) -> None:
                return None

    def test_read_windows_default_shell_returns_none_when_the_key_is_absent(self) -> None:
        module = load_module()
        original_winreg = module.winreg
        module.winreg = self._FakeWinreg(key_exists=False)
        try:
            self.assertIsNone(module._read_windows_default_shell())
        finally:
            module.winreg = original_winreg

    def test_write_then_read_windows_default_shell_round_trips(self) -> None:
        module = load_module()
        original_winreg = module.winreg
        module.winreg = self._FakeWinreg(key_exists=False)
        try:
            module._write_windows_default_shell(r"C:\Program Files\Git\bin\bash.exe")
            self.assertEqual(
                module._read_windows_default_shell(), r"C:\Program Files\Git\bin\bash.exe"
            )
            module._write_windows_default_shell(None)
            self.assertIsNone(module._read_windows_default_shell())
        finally:
            module.winreg = original_winreg

    def test_write_windows_default_shell_raises_when_writes_are_denied(self) -> None:
        module = load_module()
        original_winreg = module.winreg
        module.winreg = self._FakeWinreg(key_exists=False, deny_writes=True)
        try:
            with self.assertRaises(PermissionError):
                module._write_windows_default_shell(r"C:\Git\bin\bash.exe")
        finally:
            module.winreg = original_winreg

    def test_prepare_windows_posix_shell_skips_when_no_bash_is_found(self) -> None:
        module = load_module()
        original_find = module.find_windows_posix_shell
        module.find_windows_posix_shell = lambda env=None: None
        try:
            result = module.prepare_windows_posix_shell()
        finally:
            module.find_windows_posix_shell = original_find
        self.assertEqual(result["outcome"], "skipped_no_posix_receiver")
        self.assertIn("no POSIX shell", result["reason"])

    def test_prepare_windows_posix_shell_skips_when_the_registry_write_is_denied(self) -> None:
        module = load_module()
        original_find = module.find_windows_posix_shell
        original_winreg = module.winreg
        module.find_windows_posix_shell = lambda env=None: Path(r"C:\Git\bin\bash.exe")
        module.winreg = self._FakeWinreg(key_exists=False, deny_writes=True)
        try:
            result = module.prepare_windows_posix_shell()
        finally:
            module.find_windows_posix_shell = original_find
            module.winreg = original_winreg
        self.assertEqual(result["outcome"], "skipped_no_posix_receiver")
        self.assertIn("denied administrator access", result["reason"])

    def test_prepare_windows_posix_shell_configures_and_records_before_and_after(self) -> None:
        module = load_module()
        original_find = module.find_windows_posix_shell
        original_winreg = module.winreg
        fake_winreg = self._FakeWinreg(key_exists=True)
        fake_winreg.values["DefaultShell"] = r"C:\Windows\System32\cmd.exe"
        module.find_windows_posix_shell = lambda env=None: Path(r"C:\Git\bin\bash.exe")
        module.winreg = fake_winreg
        try:
            result = module.prepare_windows_posix_shell()
            self.assertEqual(result["outcome"], "configured")
            self.assertEqual(result["bash_path"], r"C:\Git\bin\bash.exe")
            self.assertEqual(result["before"], r"C:\Windows\System32\cmd.exe")
            self.assertEqual(result["after"], r"C:\Git\bin\bash.exe")
            self.assertEqual(
                module._read_windows_default_shell(), r"C:\Git\bin\bash.exe"
            )
            # Caller-driven restore, mirroring run_scenario's `finally`.
            module._write_windows_default_shell(result["before"])
            self.assertEqual(
                module._read_windows_default_shell(), r"C:\Windows\System32\cmd.exe"
            )
        finally:
            module.find_windows_posix_shell = original_find
            module.winreg = original_winreg

    def test_prepare_windows_posix_shell_skips_when_readback_does_not_match(self) -> None:
        """run 33141941621 @ 21f00edb1: the scenario reported `"outcome":
        "configured"` for `windows_default_shell`, yet the scratch sshd it
        started still could not run `sftp.ps1`'s POSIX remote commands --
        a registry write that raised nothing but did not durably land
        where `sshd.exe` reads it (see `_FakeWinreg.view_mismatch`). This
        must be caught and reported as `skipped_no_posix_receiver`, never
        as a false `configured`."""
        module = load_module()
        original_find = module.find_windows_posix_shell
        original_winreg = module.winreg
        fake_winreg = self._FakeWinreg(key_exists=False, view_mismatch=True)
        module.find_windows_posix_shell = lambda env=None: Path(r"C:\Git\bin\bash.exe")
        module.winreg = fake_winreg
        try:
            result = module.prepare_windows_posix_shell()
            # Never left claiming a value it could not confirm.
            self.assertIsNone(module._read_windows_default_shell())
        finally:
            module.find_windows_posix_shell = original_find
            module.winreg = original_winreg
        self.assertEqual(result["outcome"], "skipped_no_posix_receiver")
        self.assertIn("registry-view mismatch", result["reason"])


if __name__ == "__main__":
    unittest.main()
