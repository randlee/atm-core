"""Fixed-input tests of scripts/atm_send.py, the global `atm send` wrapper (~/.local/bin/atm_send).

main() runs in-process with subprocess.run replaced by a recorder, so no atm process, daemon or tmux pane
is involved; each case asserts the exact `atm send` argv and stdin the wrapper produces.
"""
from __future__ import annotations

import importlib.machinery
import importlib.util
import io
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "atm_send.py"
loader = importlib.machinery.SourceFileLoader("atm_send_wrapper", str(SCRIPT))
wrapper = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
loader.exec_module(wrapper)


class Recorder:
    def __init__(self, returncode=0):
        self.returncode, self.sends = returncode, []

    def __call__(self, argv, **kwargs):
        if argv[0] == "git":
            return SimpleNamespace(returncode=1, stdout="", stderr="")
        self.sends.append({"argv": argv, "input": kwargs.get("input"), "env": kwargs.get("env")})
        return SimpleNamespace(returncode=self.returncode)


class AtmSendWrapperTests(unittest.TestCase):
    def run_wrapper(self, *args, stdin=b"", returncode=0):
        recorder, stderr = Recorder(returncode), io.StringIO()
        stdin_stream = SimpleNamespace(buffer=io.BytesIO(stdin))
        with patch.object(wrapper.subprocess, "run", recorder), patch.object(wrapper.sys, "stdin", stdin_stream), \
             patch.object(wrapper.sys, "stderr", stderr), \
             patch.dict(wrapper.os.environ, {"ATM_IDENTITY": "fenix", "ATM_TEAM": "atm-dev"}):
            try:
                code = wrapper.main(["atm_send", *args])
            except SystemExit as exit_:
                code = exit_.code
        return code, recorder.sends, stderr.getvalue()

    def test_inline_message_injects_as_and_team(self):
        code, sends, _ = self.run_wrapper("solar", "hello")
        self.assertEqual(code, 0)
        self.assertEqual(sends[0]["argv"], ["atm", "send", "--as", "fenix", "--team", "atm-dev", "solar", "hello"])
        self.assertNotIn("--from", sends[0]["argv"])

    def test_stdin_body_passes_through_natively_byte_exact(self):
        body = b"line one\nline `two` $(not run)\n"
        code, sends, _ = self.run_wrapper("solar", "--stdin", stdin=body)
        self.assertEqual(code, 0)
        self.assertIn("--stdin", sends[0]["argv"])
        self.assertNotIn("--file", sends[0]["argv"])
        self.assertEqual(sends[0]["input"], body)

    def test_legacy_from_is_mapped_to_as(self):
        for spelling in (["--from", "lumen"], ["--from=lumen"]):
            with self.subTest(spelling=spelling):
                code, sends, _ = self.run_wrapper("solar", "hi", *spelling)
                self.assertEqual(code, 0)
                argv = sends[0]["argv"]
                self.assertTrue("--as=lumen" in argv or argv[argv.index("--as") + 1] == "lumen", argv)
                self.assertFalse(any(a == "--from" or a.startswith("--from=") for a in argv), argv)
                self.assertEqual(argv.count("--as") + sum(a.startswith("--as=") for a in argv), 1)
                self.assertEqual(sends[0]["env"]["ATM_IDENTITY"], "lumen")

    def test_file_pointer_mode_is_refused_before_any_send(self):
        code, sends, stderr = self.run_wrapper("solar", "--file", "/tmp/body.txt")
        self.assertEqual(code, 1)
        self.assertEqual(sends, [])
        self.assertIn("--stdin < <path>", stderr)

    def test_success_prints_no_nudge_warning_and_failure_returns_atm_exit(self):
        code, sends, stderr = self.run_wrapper("solar", "hello", "--no-nudge", "--nudge-delay", "0.5")
        self.assertEqual((code, stderr), (0, ""))
        self.assertEqual(len(sends), 1)
        self.assertNotIn("--no-nudge", sends[0]["argv"])
        code, _, stderr = self.run_wrapper("solar", "hello", returncode=3)
        self.assertEqual((code, stderr), (3, ""))

    def test_missing_identity_is_refused(self):
        with patch.dict(wrapper.os.environ, {"ATM_IDENTITY": ""}):
            recorder = Recorder()
            with patch.object(wrapper.subprocess, "run", recorder), patch.object(wrapper.sys, "stderr", io.StringIO()):
                with self.assertRaises(SystemExit) as exit_:
                    wrapper.main(["atm_send", "solar", "hi"])
        self.assertEqual(exit_.exception.code, 1)
        self.assertEqual(recorder.sends, [])


if __name__ == "__main__":
    unittest.main()
