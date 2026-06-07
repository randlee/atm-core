from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
import json
from typing import Any


def run_shared_host_lane(runtime: Any, rows: dict[str, Any]) -> tuple[bool, Any, int | None]:
    shared_host_fixture_pair = runtime.create_shared_host_fixture_pair(
        prefix="z21s.",
        team_name_a="z21-shared-a",
        team_name_b="z21-shared-b",
        operator_a="z21-shared-operator-a",
        operator_b="z21-shared-operator-b",
        recipient_a="z21-shared-recipient-a",
        recipient_b="z21-shared-recipient-b",
    )
    shared_a = shared_host_fixture_pair.workspace_a
    shared_b = shared_host_fixture_pair.workspace_b
    shared_env_a = runtime.smoke_env(shared_a, identity=shared_a.operator, root=runtime.root)
    shared_env_b = runtime.smoke_env(shared_b, identity=shared_b.operator, root=runtime.root)
    shared_bootstrap_env = shared_env_a.copy()
    shared_bootstrap_env.pop("ATM_IDENTITY", None)
    shared_bootstrap_env.pop("ATM_TEAM", None)
    runtime.run_atm(
        runtime.root,
        shared_bootstrap_env,
        shared_a.workspace_dir,
        "doctor",
        "--json",
    )
    shared_doctor_a = runtime.parse_json_output(
        runtime.run_atm(runtime.root, shared_env_a, shared_a.workspace_dir, "doctor", "--json")
    )
    shared_doctor_b = runtime.parse_json_output(
        runtime.run_atm(runtime.root, shared_env_b, shared_b.workspace_dir, "doctor", "--json")
    )
    shared_pid_a = shared_doctor_a.get("runtime_status", {}).get("singleton_owner_pid")
    shared_pid_b = shared_doctor_b.get("runtime_status", {}).get("singleton_owner_pid")
    shared_daemon_pid = int(shared_pid_a) if shared_pid_a is not None else None
    for fixture_item, env_item in ((shared_a, shared_env_a), (shared_b, shared_env_b)):
        runtime.run_atm(
            runtime.root,
            env_item,
            fixture_item.workspace_dir,
            "teams",
            "add-member",
            fixture_item.team_name,
            fixture_item.operator,
            "--json",
        )
        runtime.run_atm(
            runtime.root,
            env_item,
            fixture_item.workspace_dir,
            "teams",
            "add-member",
            fixture_item.team_name,
            fixture_item.recipient,
            "--json",
        )

    def run_send(fixture_item: Any, env_item: dict[str, str], body: str) -> dict[str, object]:
        target = f"{fixture_item.recipient}@{fixture_item.team_name}"
        return runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                env_item,
                fixture_item.workspace_dir,
                "send",
                target,
                body,
                "--from",
                fixture_item.operator,
                "--requires-ack",
                "--json",
            )
        )

    with ThreadPoolExecutor(max_workers=2) as pool:
        send_future_a = pool.submit(
            run_send, shared_a, shared_env_a, "shared-host message from workspace A"
        )
        send_future_b = pool.submit(
            run_send, shared_b, shared_env_b, "shared-host message from workspace B"
        )
        shared_send_a = send_future_a.result()
        shared_send_b = send_future_b.result()

    shared_message_id_a = str(shared_send_a["message_id"])
    shared_message_id_b = str(shared_send_b["message_id"])

    def read_and_ack(
        fixture_item: Any,
        env_item: dict[str, str],
        message_id: str,
        ack_body: str,
    ) -> dict[str, object]:
        read_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                env_item,
                fixture_item.workspace_dir,
                "read",
                fixture_item.recipient,
                "--as",
                fixture_item.recipient,
                "--team",
                fixture_item.team_name,
                "--all",
                "--message-id",
                message_id,
                "--json",
            )
        )
        ack_payload = runtime.parse_json_output(
            runtime.run_atm(
                runtime.root,
                env_item,
                fixture_item.workspace_dir,
                "ack",
                message_id,
                ack_body,
                "--team",
                fixture_item.team_name,
                "--as",
                fixture_item.recipient,
                "--json",
            )
        )
        return {"read": read_payload, "ack": ack_payload}

    with ThreadPoolExecutor(max_workers=2) as pool:
        read_ack_future_a = pool.submit(
            read_and_ack, shared_a, shared_env_a, shared_message_id_a, "shared-host ack A"
        )
        read_ack_future_b = pool.submit(
            read_and_ack, shared_b, shared_env_b, shared_message_id_b, "shared-host ack B"
        )
        shared_read_ack_a = read_ack_future_a.result()
        shared_read_ack_b = read_ack_future_b.result()

    shared_list_a = runtime.parse_json_output(
        runtime.run_atm(
            runtime.root,
            shared_env_a,
            shared_a.workspace_dir,
            "list",
            "--as",
            shared_a.operator,
            "--team",
            shared_a.team_name,
            "--json",
        )
    )
    shared_list_b = runtime.parse_json_output(
        runtime.run_atm(
            runtime.root,
            shared_env_b,
            shared_b.workspace_dir,
            "list",
            "--as",
            shared_b.operator,
            "--team",
            shared_b.team_name,
            "--json",
        )
    )
    shared_log_snapshot_a = runtime.parse_json_output(
        runtime.run_atm(runtime.root, shared_env_a, shared_a.workspace_dir, "log", "snapshot", "--json")
    )
    shared_records_a = json.dumps(shared_list_a)
    shared_records_b = json.dumps(shared_list_b)
    shared_host_ok = (
        shared_doctor_a.get("summary", {}).get("status") == "healthy"
        and shared_doctor_b.get("summary", {}).get("status") == "healthy"
        and shared_pid_a is not None
        and shared_pid_a == shared_pid_b
        and shared_send_a.get("outcome") == "sent"
        and shared_send_b.get("outcome") == "sent"
        and shared_read_ack_a["read"].get("selected_message_id") == shared_message_id_a
        and shared_read_ack_b["read"].get("selected_message_id") == shared_message_id_b
        and shared_read_ack_a["ack"].get("message_id") == shared_message_id_a
        and shared_read_ack_b["ack"].get("message_id") == shared_message_id_b
        and shared_message_id_b not in shared_records_a
        and shared_message_id_a not in shared_records_b
        and isinstance(shared_log_snapshot_a.get("records"), list)
        and runtime.process_is_alive(int(shared_pid_a))
    )
    if shared_host_ok:
        runtime.pass_row(
            rows["PRR-001"],
            "two workspaces with one shared ATM_HOME daemon/database/log root handled concurrent send/read/ack traffic without cross-workspace leakage",
        )
        return True, shared_host_fixture_pair, shared_daemon_pid

    runtime.fail_row(
        rows["PRR-001"],
        observed=json.dumps(
            {
                "doctor_a": shared_doctor_a,
                "doctor_b": shared_doctor_b,
                "send_a": shared_send_a,
                "send_b": shared_send_b,
                "read_ack_a": shared_read_ack_a,
                "read_ack_b": shared_read_ack_b,
                "list_a": shared_list_a,
                "list_b": shared_list_b,
                "log_snapshot_a": shared_log_snapshot_a,
            },
            indent=2,
        ),
        expected="two or more workspaces share one host daemon/database/log root, concurrent send/read/ack succeeds, no cross-workspace message leakage occurs, and the shared daemon remains healthy",
        root_cause="the shared-host same-daemon smoke lane diverged before proving the accepted multi-workspace topology",
        artifact="shared-host doctor/send/read/ack/list/log snapshot outputs",
        notes="shared-host multi-workspace smoke coverage failed",
    )
    return False, shared_host_fixture_pair, shared_daemon_pid
