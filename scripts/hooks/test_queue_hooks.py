#!/usr/bin/env python3
"""Deterministic unit tests for the ATM lifecycle hook contract.

Two test classes: ``QueueHookTests`` covers the shared Claude/Codex-neutral
contract and is run on all three CI matrix OSes (Claude Code runs on
Windows). ``CodexQueueHookTests`` covers only the Codex-specific ``--harness
codex`` behavior; it is gated to ubuntu/macOS both here (a class-level
``unittest.skipIf`` so a direct/local run on Windows also skips cleanly, per
AC10 — Codex/hermes are not used on Windows) and at the CI/Justfile lane
level (``just test-queue-hooks-python-codex`` is only invoked on non-Windows
matrix legs; see ``Justfile`` and ``.github/workflows/ci.yml``).
"""

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


class HookTestHelpers:
    """Shared fixtures for the Claude-neutral and Codex-specific test
    classes below. Deliberately not a `unittest.TestCase` subclass so
    neither test class inherits the other's `test_*` methods (that would
    double-run the shared contract tests once per class)."""

    def run_hook(
        self,
        event: str,
        fake: Path,
        state: Path,
        harness: str = "claude",
        debounce_seconds: str = "0.02",
    ) -> subprocess.CompletedProcess[str]:
        env = {
            **os.environ,
            "ATM_BIN": str(fake),
            "ATM_HOOK_STATE_DIR": str(state),
            "ATM_HOOK_DEBOUNCE_SECONDS": debounce_seconds,
            "ATM_HOOK_TIMEOUT_SECONDS": "1",
            "ATM_IDENTITY": "test-agent",
            "ATM_TEAM": "test-team",
            "ATM_HOME": str(state.parent),
            "ATM_CONFIG_HOME": str(state.parent),
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

    def stateful_fake_cli(self, root: Path, rows: list[dict[str, object]]) -> Path:
        """A fake `atm` whose `_internal-queue-get` drains exactly one item
        per call, oldest first, like the real bare-CLI FIFO — unlike
        `fake_cli`, which always replays the same static rows on every
        call. This lets a test exercise a literal, multi-`Stop` drain
        sequence end to end at the hook-script level (AC4)."""
        state_file = root / "queue-state.json"
        state_file.write_text(json.dumps(rows), encoding="utf-8")
        fake = root / "fake-atm-stateful.py"
        fake.write_text(
            "#!/usr/bin/env python3\n"
            "import json, pathlib, sys\n"
            f"state = pathlib.Path({str(state_file)!r})\n"
            "pathlib.Path(__file__).with_suffix('.calls').open('a').write(' '.join(sys.argv[1:])+'\\n')\n"
            "if sys.argv[1:2] == ['_internal-queue-get']:\n"
            "    remaining = json.loads(state.read_text())\n"
            "    if remaining:\n"
            "        print(json.dumps(remaining[0]))\n"
            "        state.write_text(json.dumps(remaining[1:]))\n",
            encoding="utf-8",
        )
        fake.chmod(0o755)
        return fake

    def calls_for(self, fake: Path) -> list[str]:
        calls_path = fake.with_suffix(".calls")
        if not calls_path.exists():
            return []
        return calls_path.read_text(encoding="utf-8").splitlines()


class QueueHookTests(HookTestHelpers, unittest.TestCase):
    """Shared, harness-neutral hook contract. Runs on all three CI matrix
    OSes, including Windows (Claude Code runs on Windows)."""

    def test_stop_pull_blocks_with_literal_json_and_completes_idle_inline(self):
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.fake_cli(root, [{"kind": "queue", "msg_id": "01TEST", "body": "read atm"}])
            # Avoid leaving a detached expiry child behind while
            # TemporaryDirectory is tearing down this fixture. Other tests
            # cover the delayed expiry path and the worker also tolerates a
            # vanished state directory.
            result = self.run_hook("stop", fake, state, debounce_seconds="0")
            self.assertEqual(result.returncode, 0)
            self.assertEqual(json.loads(result.stdout), {"decision": "block", "reason": "read atm"})

    def test_pre_tool_use_cancels_debounced_stop_timer(self):
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.fake_cli(root, [])
            self.run_hook("stop", fake, state)
            self.run_hook("pre-tool-use", fake, state)
            self.assertFalse((state / "pending-idle").exists())

    def test_idle_expiry_ignores_a_vanished_state_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.fake_cli(root, [])
            result = self.run_hook("idle-expired", fake, state)
            self.assertEqual(result.returncode, 0)

    def test_stop_debounce_expiry_sends_exactly_one_idle_heartbeat(self):
        """AC2: proves the debounced expiry actually calls
        `_internal-heartbeat --activity idle`, not merely that the debounce
        window elapses."""
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.fake_cli(root, [])
            result = self.run_hook("stop", fake, state)
            self.assertEqual(result.returncode, 0)
            deadline = time.monotonic() + 2.0
            heartbeat_calls: list[str] = []
            while time.monotonic() < deadline:
                heartbeat_calls = [
                    call for call in self.calls_for(fake)
                    if call.startswith("_internal-heartbeat")
                ]
                if heartbeat_calls:
                    break
                time.sleep(0.02)
            self.assertEqual(
                heartbeat_calls,
                ["_internal-heartbeat --activity idle --as test-agent"],
                "the debounced expiry must send exactly one idle heartbeat call",
            )

    def test_empty_stop_proceeds_without_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.fake_cli(root, [])
            result = self.run_hook("stop", fake, state)
            self.assertEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")

    def test_stop_fails_loudly_when_caller_context_is_missing(self):
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.fake_cli(root, [])
            env = {
                **os.environ,
                "ATM_BIN": str(fake),
                "ATM_HOOK_STATE_DIR": str(state),
                "ATM_HOOK_TIMEOUT_SECONDS": "1",
                "ATM_IDENTITY": "test-agent",
                "ATM_HOME": str(root),
            }
            env.pop("ATM_TEAM", None)
            result = subprocess.run(
                [sys.executable, str(HOOK), "--event", "stop"],
                capture_output=True,
                text=True,
                env=env,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("ATM_TEAM is required", result.stderr)

    def test_stop_fails_loudly_when_atm_cli_cannot_start(self):
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            result = self.run_hook("stop", root / "missing-atm", state)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("could not run", result.stderr)

    def test_literal_multi_stop_drain_sequence_terminates_on_empty(self):
        """AC4: a genuine-idle Stop drains the oldest queued message, the
        following Stop (the harness's own re-invocation while it keeps
        looping) drains the next, and the next Stop after that sees an
        empty FIFO, emits nothing, and proceeds -- never-block-on-empty is
        the loop terminator."""
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.stateful_fake_cli(
                root,
                [
                    {"kind": "queue", "msg_id": "01FIRST", "body": "first queued message"},
                    {"kind": "queue", "msg_id": "01SECOND", "body": "second queued message"},
                ],
            )
            first = self.run_hook("stop", fake, state)
            self.assertEqual(first.returncode, 0)
            self.assertEqual(
                json.loads(first.stdout),
                {"decision": "block", "reason": "first queued message"},
            )
            second = self.run_hook("stop", fake, state)
            self.assertEqual(second.returncode, 0)
            self.assertEqual(
                json.loads(second.stdout),
                {"decision": "block", "reason": "second queued message"},
            )
            third = self.run_hook("stop", fake, state)
            self.assertEqual(third.returncode, 0)
            self.assertEqual(third.stdout, "", "the third Stop must proceed with no output")


@unittest.skipIf(
    sys.platform.startswith("win"),
    "Codex/hermes lifecycle hooks are not used on Windows (ADR-054 AQ2.5 AC10); "
    "gated separately at the Justfile/CI lane level too.",
)
class CodexQueueHookTests(HookTestHelpers, unittest.TestCase):
    """Codex-specific (`--harness codex`) hook contract tests only.

    Composes `HookTestHelpers` rather than subclassing `QueueHookTests`, so
    this class carries exactly its own `test_*` methods -- running
    `QueueHookTests` and `CodexQueueHookTests` never double-executes the
    shared contract tests.
    """

    def test_codex_stop_consumes_queue_without_claude_block_json(self):
        with tempfile.TemporaryDirectory() as directory:
            root, state = Path(directory), Path(directory) / "state"
            fake = self.fake_cli(root, [{"kind": "steer", "msg_id": "01TEST", "body": "notice"}])
            result = self.run_hook("stop", fake, state, "codex")
            self.assertEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
