#!/usr/bin/env python3
"""Deterministic unit tests for the ATM lifecycle hook contract."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]
HOOK = ROOT / "scripts" / "hooks" / "atm_queue_hook.py"


class QueueHookTests(unittest.TestCase):
    def run_hook(self, event: str, fake: Path, state: Path, harness: str = "claude") -> subprocess.CompletedProcess[str]:
        env = {
            **os.environ,
            "ATM_BIN": str(fake),
            "ATM_HOOK_STATE_DIR": str(state),
            "ATM_HOOK_DEBOUNCE_SECONDS": "0.02",
            "ATM_HOOK_TIMEOUT_SECONDS": "1",
            "ATM_IDENTITY": "test-agent",
            "ATM_TEAM": "test-team",
        }
        return subprocess.run(
            [sys.executable, str(HOOK), "--event", event, "--harness", harness],
            capture_output=True,
            text=True,
            env=env,
            check=False,
        )

    def fake_cli(self, root: Path, rows: list[dict[str, object]]) -> Path:
        fake = root / "fake-atm.py"
        fake.write_text(
            "#!/usr/bin/env python3\n"
            "import json, pathlib, sys\n"
            f"rows = {rows!r}\n"
            "pathlib.Path(__file__).with_suffix('.calls').open('a').write(' '.join(sys.argv[1:])+'\\n')\n"
            "if sys.argv[1:2] == ['_internal-queue-get']:\n"
            "    print('\\n'.join(json.dumps(row) for row in rows))\n",
            encoding="utf-8",
        )
        fake.chmod(0o755)
        return fake

    def test_stop_pull_blocks_with_literal_json_and_schedules_idle(self):
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.fake_cli(root, [{"kind": "queue", "msg_id": "01TEST", "body": "read atm"}])
            result = self.run_hook("stop", fake, state)
            self.assertEqual(result.returncode, 0)
            self.assertEqual(json.loads(result.stdout), {"decision": "block", "reason": "read atm"})
            time.sleep(0.05)

    def test_pre_tool_use_cancels_debounced_stop_timer(self):
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.fake_cli(root, [])
            self.run_hook("stop", fake, state)
            self.run_hook("pre-tool-use", fake, state)
            self.assertFalse((state / "pending-idle").exists())

    def test_empty_stop_proceeds_without_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.fake_cli(root, [])
            result = self.run_hook("stop", fake, state)
            self.assertEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")

    def test_codex_stop_consumes_queue_without_claude_block_json(self):
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.fake_cli(root, [{"kind": "steer", "msg_id": "01TEST", "body": "notice"}])
            result = self.run_hook("stop", fake, state, "codex")
            self.assertEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
