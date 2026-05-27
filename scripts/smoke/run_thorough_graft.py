from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import time
from typing import Any


def enable_graft_config(workspace_dir: Path) -> None:
    config_path = workspace_dir / ".atm.toml"
    config_text = config_path.read_text(encoding="utf-8")
    if "[atm.graft]" in config_text:
        return
    graft_section = '\n[atm.graft]\nenabled = true\n'
    config_path.write_text(config_text.rstrip() + graft_section, encoding="utf-8")


def graft_smoke_example_path(root: Path) -> Path:
    example_name = "smoke_same_host.exe" if os.name == "nt" else "smoke_same_host"
    return root / "target" / "release" / "examples" / example_name


def graft_ready_timeout_secs() -> float:
    return float(os.environ.get("ATM_SMOKE_GRAFT_READY_TIMEOUT_SECS", "30"))


def graft_complete_timeout_secs() -> int:
    return int(os.environ.get("ATM_SMOKE_GRAFT_COMPLETE_TIMEOUT_SECS", "90"))


def run_graft_lane(
    runtime: Any,
    rows: dict[str, Any],
    fixture: Any,
    base_env: dict[str, str],
) -> bool:
    nudge_timeout = float(os.environ.get("ATM_SMOKE_GRAFT_NUDGE_TIMEOUT_SECS", "30"))
    graft_complete_timeout = graft_complete_timeout_secs()
    if nudge_timeout >= graft_complete_timeout:
        raise ValueError(
            "ATM_SMOKE_GRAFT_NUDGE_TIMEOUT_SECS "
            f"({nudge_timeout}s) must be < ATM_SMOKE_GRAFT_COMPLETE_TIMEOUT_SECS "
            f"({graft_complete_timeout}s)"
        )
    ready_path = fixture.root / "graft-ready"
    ready_path.unlink(missing_ok=True)
    graft_env = runtime.smoke_env(fixture, identity=runtime.recipient, root=runtime.root)
    graft_send_payload: dict[str, object] | None = None
    graft_stdout = ""
    graft_stderr = ""
    graft_error: str | None = None
    graft_payload: dict[str, object] | None = None
    graft_process = subprocess.Popen(
        [
            str(graft_smoke_example_path(runtime.root)),
            str(fixture.workspace_dir),
            runtime.team,
            runtime.recipient,
            f"{runtime.operator}@{runtime.team}",
            "thorough smoke graft requires ack",
            runtime.operator,
            str(ready_path),
        ],
        cwd=fixture.workspace_dir,
        env=graft_env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        ready_deadline = time.perf_counter() + graft_ready_timeout_secs()
        while not ready_path.exists():
            if graft_process.poll() is not None:
                graft_stdout, graft_stderr = graft_process.communicate()
                graft_error = "atm-graft smoke host exited before reporting ready"
                break
            if time.perf_counter() >= ready_deadline:
                graft_process.kill()
                graft_stdout, graft_stderr = graft_process.communicate()
                graft_error = "timed out waiting for atm-graft smoke host readiness"
                break
            time.sleep(0.05)

        if graft_error is None:
            try:
                graft_send_payload = runtime.parse_json_output(
                    runtime.run_atm(
                        runtime.root,
                        base_env,
                        fixture.workspace_dir,
                        "send",
                        runtime.recipient,
                        "thorough smoke graft requires ack",
                        "--requires-ack",
                        "--json",
                )
                )
                try:
                    graft_stdout, graft_stderr = graft_process.communicate(
                        timeout=graft_complete_timeout
                    )
                except subprocess.TimeoutExpired:
                    graft_process.kill()
                    graft_stdout, graft_stderr = graft_process.communicate()
                    graft_error = "atm-graft smoke host timed out before completing the ICD flow"
                if graft_error is None and graft_process.returncode == 0:
                    graft_payload = json.loads(graft_stdout)
            except Exception as exc:
                graft_error = str(exc)
    finally:
        if graft_process.poll() is None:
            graft_process.kill()
            graft_process.communicate()

    if graft_error is not None or graft_process.returncode != 0 or graft_payload is None:
        runtime.fail_row(
            rows["GRAFT-001"],
            observed=json.dumps(
                {
                    "send": graft_send_payload,
                    "graft_error": graft_error,
                    "stdout": graft_stdout,
                    "stderr": graft_stderr,
                    "returncode": graft_process.returncode,
                },
                indent=2,
            ),
            expected="the atm-graft host registers, receives one advisory nudge, reads and acknowledges the nudged message, and sends one unary follow-up back to the CLI operator",
            root_cause="the same-host atm-graft smoke host exited before completing the advisory plus unary ICD lane",
            artifact="atm-graft smoke host stdout/stderr",
            notes="same-host atm-graft advisory and unary ICD lane failed",
        )
        return False

    ack_reply_read = runtime.parse_json_output(
        runtime.run_atm(
            runtime.root,
            base_env,
            fixture.workspace_dir,
            "read",
            runtime.operator,
            "--team",
            runtime.team,
            "--all",
            "--message-id",
            str(graft_payload["ack_reply_message_id"]),
            "--json",
        )
    )
    follow_up_read = runtime.parse_json_output(
        runtime.run_atm(
            runtime.root,
            base_env,
            fixture.workspace_dir,
            "read",
            runtime.operator,
            "--team",
            runtime.team,
            "--all",
            "--message-id",
            str(graft_payload["follow_up_message_id"]),
            "--json",
        )
    )
    graft_ok = (
        graft_payload.get("status") == "passed"
        and graft_payload.get("nudge_count") == 1
        and graft_payload.get("nudge_from") == runtime.operator
        and graft_payload.get("nudge_message_id") == str(graft_send_payload["message_id"])
        and graft_payload.get("read_selected_message_id") == str(graft_send_payload["message_id"])
        and ack_reply_read.get("selected_message_id") == str(graft_payload["ack_reply_message_id"])
        and follow_up_read.get("selected_message_id") == str(graft_payload["follow_up_message_id"])
    )
    if graft_ok:
        runtime.pass_row(
            rows["GRAFT-001"],
            "a real atm-graft host registered, consumed the advisory nudge, read and acknowledged the nudged message, and sent a unary follow-up back to the CLI operator",
        )
        return True

    runtime.fail_row(
        rows["GRAFT-001"],
        observed=json.dumps(
            {
                "send": graft_send_payload,
                "graft_payload": graft_payload,
                "ack_reply_read": ack_reply_read,
                "follow_up_read": follow_up_read,
            },
            indent=2,
        ),
        expected="the atm-graft host registers, receives one advisory nudge, reads and acknowledges the nudged message, and sends one unary follow-up back to the CLI operator",
        root_cause="the same-host atm-graft advisory and unary ICD lane diverged before the smoke runner could prove the retained CLI and graft surfaces share the accepted daemon contract",
        artifact="atm-graft smoke host JSON plus operator-side read outputs",
        notes="same-host atm-graft advisory and unary ICD lane failed",
    )
    return False
