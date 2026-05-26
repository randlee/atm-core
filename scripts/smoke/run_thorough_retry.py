from __future__ import annotations

import json
from typing import Any


def verify_retry_evidence(
    runtime: Any,
    rows: dict[str, Any],
    copied_log_snapshot: dict[str, object],
) -> bool:
    retry_outcomes = {
        "initial_miss",
        "retry_attempt",
        "acquired",
        "spawn_requested",
        "publish_wait_started",
        "publish_wait_continuing",
        "connected",
    }
    observed_retry_outcomes = set()
    for record in copied_log_snapshot.get("records", []):
        if not isinstance(record, dict):
            continue
        message = str(record.get("message", ""))
        marker = "with outcome "
        if marker in message:
            observed_retry_outcomes.add(message.split(marker, 1)[1].strip())
    if retry_outcomes.issubset(observed_retry_outcomes):
        runtime.pass_row(
            rows["Z1-009"],
            "copied-state log snapshot retained the expected retry-visible daemon lifecycle outcomes while the durable send/read path succeeded",
        )
        return True

    runtime.fail_row(
        rows["Z1-009"],
        observed=json.dumps(
            {
                "observed_outcomes": sorted(observed_retry_outcomes),
                "records": copied_log_snapshot.get("records", []),
            },
            indent=2,
        ),
        expected="log snapshot includes initial_miss, retry_attempt, acquired, spawn_requested, publish_wait_started, publish_wait_continuing, and connected while the durable copied-state lane succeeds",
        root_cause="retry-visible daemon lifecycle evidence was not preserved in the retained copied-state log snapshot",
        artifact="copied-state log snapshot --json",
        notes="retry-visible daemon/runtime evidence was incomplete",
    )
    return False
