from __future__ import annotations

import collections
import importlib.util
import os
import subprocess
import sys
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace


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
            ("fail", 1),
            ("blocked_sshd_start_failed", 1),
            ("blocked_daemon_start_failed", 1),
        ):
            with self.subTest(status=status):
                self.assertEqual(0 if status in ("pass", "blocked_ambient_daemon", "skipped_no_sshd") else 1, expected)

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
                info = module.install_transfer_script(home)
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
            module.install_transfer_script(home)
            (home / ".atm" / "transfer").chmod(0o755)
            info = module.install_transfer_script(home)
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

    def test_write_scratch_ssh_client_config_uses_dev_null_known_hosts_on_unix(self) -> None:
        module = load_module()
        self.assertFalse(module.IS_WINDOWS, "this test asserts the non-Windows branch")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            config_path = module.write_scratch_ssh_client_config(root, 1, root / "identity")
            self.assertIn("UserKnownHostsFile /dev/null", config_path.read_text(encoding="utf-8"))

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
                info = module.install_transfer_script(home)

                installed = home / ".atm" / "transfer" / f"{module.TRANSFER_HOST}.ps1"
                self.assertEqual(info["installed_at"], str(installed))
                self.assertTrue(installed.is_file())
                self.assertEqual(
                    installed.read_bytes(),
                    (module.ROOT / "scripts" / "transfer" / "sftp.ps1").read_bytes(),
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


if __name__ == "__main__":
    unittest.main()
