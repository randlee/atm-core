#!/usr/bin/env python3
"""Default-deny source-use guard for Phase AJ runtime observations."""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
ALLOWED = {
    "crates/atm-core/src/caller_context.rs",
    "crates/atm-core/src/send/mod.rs",
    "crates/atm-core/src/send/request.rs",
    "crates/atm-core/src/read/mod.rs",
    "crates/atm-core/src/read/request.rs",
    "crates/atm-core/src/ack/mod.rs",
    "crates/atm-core/src/protocol.rs",
    "crates/atm-daemon/src/https_transport.rs",
    "crates/atm-daemon/src/runtime_health.rs",
    "crates/atm-daemon/src/runtime_health/dispatch.rs",
    "crates/atm-daemon/src/runtime_status_cache.rs",
    "crates/atm-core/src/api.rs",
    "crates/atm-core/src/send/post_write_tests.rs",
    "crates/atm-daemon/src/tests_post_send_graft_warning.rs",
    "crates/atm/src/composition.rs",
    "crates/atm/src/commands/ack.rs",
    "crates/atm/src/commands/send.rs",
    "crates/atm/src/commands/teams.rs",
    "crates/atm/src/commands/read.rs",
    "crates/atm-graft-python/src/lib.rs",
    "crates/atm-graft/examples/smoke_same_host.rs",
    "crates/atm-graft/src/lib.rs",
}
REQUIRED = {
    "crates/atm-core/src/caller_context.rs": "ActivityObservation",
    "crates/atm-core/src/send/mod.rs": "activity_observation",
    "crates/atm-core/src/read/request.rs": "activity_observation",
    "crates/atm-core/src/ack/mod.rs": "activity_observation",
    "crates/atm-daemon/src/https_transport.rs": "activity_observation",
    "crates/atm-daemon/src/runtime_health/dispatch.rs": "touch_member",
    "crates/atm-daemon/src/runtime_status_cache.rs": "merge_observation",
}
TOKENS = ("ActivityObservation", "RuntimeMemberObservation", "RuntimeObservationSource", "activity_observation")

def main() -> int:
    failures = []
    for path, symbol in REQUIRED.items():
        if symbol not in (ROOT / path).read_text():
            failures.append(f"required positive missing: {path} ({symbol})")
    for source in (ROOT / "crates").rglob("*.rs"):
        relative = source.relative_to(ROOT).as_posix()
        if "/tests/" in relative or relative.endswith("/tests.rs") or "test_" in source.name:
            continue
        if relative in ALLOWED:
            continue
        for line_no, line in enumerate(source.read_text().splitlines(), 1):
            if any(token in line for token in TOKENS):
                failures.append(f"{relative}:{line_no}: runtime observation source use is not allowed")
    if failures:
        print("\n".join(failures))
        return 1
    print("runtime-observation-boundary passed")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
