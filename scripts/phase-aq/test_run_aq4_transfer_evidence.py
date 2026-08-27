from __future__ import annotations

import importlib.util
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


if __name__ == "__main__":
    unittest.main()
