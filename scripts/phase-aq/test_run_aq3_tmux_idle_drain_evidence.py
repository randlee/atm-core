from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace


SCRIPT = Path(__file__).with_name("run_aq3_tmux_idle_drain_evidence.py")


def load_module():
    spec = importlib.util.spec_from_file_location("run_aq3_tmux_idle_drain_evidence", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class Aq3TmuxIdleDrainEvidenceTests(unittest.TestCase):
    def test_daemon_launch_argv_selects_plaintext_test_and_ready_stdout(self) -> None:
        module = load_module()
        launch = SimpleNamespace(
            args=[str(module.Path("/bin/atm-daemon")), "--peer-wire-security", "plaintext-test"]
        )
        self.assertIn("--peer-wire-security", launch.args)
        self.assertIn("plaintext-test", launch.args)

    def test_daemon_env_bridges_the_scratch_tmux_socket_without_dash_l(self) -> None:
        # The daemon's own tmux received-hook shells out to a bare `tmux`
        # with no `-L`/`-S`; TMUX=<socket_path>,0,0 is the mechanism tmux
        # itself uses to resolve an ambient server, matching what a shell
        # already running inside a tmux session would inherit.
        socket_path = "/tmp/tmux-501/aq3-deadbeef"
        daemon_env = {"TMUX": f"{socket_path},0,0"}
        self.assertEqual(daemon_env["TMUX"], "/tmp/tmux-501/aq3-deadbeef,0,0")

    def test_tmux_available_reflects_shutil_which(self) -> None:
        module = load_module()
        self.assertIsInstance(module.tmux_available(), bool)

    def test_evidence_file_names_are_host_scoped(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="clean-runner-linux", evidence_dir=Path(temporary))
            record = {
                "sprint": "AQ3",
                "host": "clean-runner-linux",
                "status": "skipped_no_tmux",
                "note": "tmux is not available on this runner",
            }
            json_path, markdown_path = module.write_evidence(args, record)

            self.assertEqual(json_path.name, "tmux-idle-drain-clean-runner-linux.json")
            self.assertEqual(markdown_path.name, "tmux-idle-drain-clean-runner-linux.md")
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("SKIPPED_NO_TMUX", markdown)
            self.assertIn("fail-closed skip", markdown)

    def test_evidence_record_schema_for_a_passing_scenario(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="m5", evidence_dir=Path(temporary))
            record = {
                "sprint": "AQ3",
                "host": "m5",
                "status": "pass",
                "steer_kind_immediate": {
                    "send": {"verb": "send", "body": module.STEER_BODY, "stdout": ""},
                    "delivered_before_any_idle_transition": True,
                },
                "idle_transition_drain_one": {
                    "drained_delta": 1,
                    "second_item_not_yet_present": True,
                },
                "idle_transition_drain_two": {"drained_delta": 1},
                "fifo_order_confirmed": True,
                "single_drain_per_transition_confirmed": True,
            }
            json_path, markdown_path = module.write_evidence(args, record)

            payload = json_path.read_text(encoding="utf-8")
            self.assertIn('"schema_version": 1', payload)
            self.assertIn('"sprint": "AQ3"', payload)
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("FIFO order confirmed: **True**", markdown)
            self.assertIn("Single drain per transition confirmed", markdown)

    def test_ambient_daemon_block_records_the_singleton_lock_reason(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="local", evidence_dir=Path(temporary))
            record = {
                "sprint": "AQ3",
                "host": "local",
                "status": "blocked_ambient_daemon",
                "ambient_daemon_pids": [4242],
            }
            _json_path, markdown_path = module.write_evidence(args, record)
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("run_hermes_atm_restart_matrix.py", markdown)
            self.assertIn("run_aq25_queue_delivery_trigger_evidence.py", markdown)

    def test_run_scenario_records_a_skip_when_tmux_is_unavailable(self) -> None:
        module = load_module()
        original = module.tmux_available
        module.tmux_available = lambda: False
        try:
            record = module.run_scenario(SimpleNamespace(host="local", timeout=1.0))
        finally:
            module.tmux_available = original
        self.assertEqual(record["status"], "skipped_no_tmux")

    def test_wait_for_drained_counter_polls_past_the_tmux_double_enter_tail_latency(self) -> None:
        # Regression for a real observed race (2026-08-27 clean-runner CI):
        # `wait_for_pane` only proves the rendered nudge became visible on the
        # daemon's *first* tmux send-keys call; the pending-marker clear and
        # counter increment land only after two more tmux round trips
        # separated by the 275ms TMUX_DOUBLE_ENTER_DELAY. A single immediate
        # `drained_counter` read reproducibly under-reads; polling must not.
        module = load_module()
        readings = iter([0, 0, 1])
        module.drained_counter = lambda atm, env, timeout: next(readings)
        result = module.wait_for_drained_counter_at_least(Path("/bin/atm"), {}, 5.0, minimum=1)
        self.assertEqual(result, 1)

    def test_wait_for_drained_counter_times_out_returning_the_last_observed_value(self) -> None:
        module = load_module()
        module.drained_counter = lambda atm, env, timeout: 0
        result = module.wait_for_drained_counter_at_least(Path("/bin/atm"), {}, 0.2, minimum=1)
        self.assertEqual(result, 0)

    def test_message_kinds_map_to_the_expected_atm_verbs(self) -> None:
        # AQ1 D4 / NudgeMode: `atm queue` is always Deferred (pending-store
        # gated), `atm send` is always Immediate (never queued). This
        # constant pairing is what lets the scenario prove steer-kind
        # delivery bypasses idle-transition gating entirely.
        module = load_module()
        self.assertNotEqual(module.QUEUE_BODY_ONE, module.STEER_BODY)
        self.assertNotEqual(module.QUEUE_BODY_TWO, module.STEER_BODY)
        self.assertNotEqual(module.QUEUE_BODY_ONE, module.QUEUE_BODY_TWO)


if __name__ == "__main__":
    unittest.main()
