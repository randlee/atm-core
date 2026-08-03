#!/usr/bin/env python3
"""Run an end-to-end smoke test for the Hermes PyO3 graft surface.

The test uses two registered Hermes identities: ``sender`` writes to the
receiver mailbox, while the receiver exercises read, acknowledge, and graft
nudging.  Run it from the active Hermes gateway environment after building
the binding with Maturin.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import threading
import time
import uuid

def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sender", default="hendrix")
    parser.add_argument("--agent", default=os.environ.get("ATM_IDENTITY", "skillrx"))
    parser.add_argument("--team", default=os.environ.get("ATM_TEAM", "hermes"))
    parser.add_argument("--chat-id", default=None)
    parser.add_argument(
        "--workspace-root",
        default=os.environ.get("ATM_WORKSPACE_ROOT", str(Path.cwd())),
    )
    parser.add_argument("--timeout", type=float, default=15.0)
    return parser.parse_args()


def expect_error(label: str, operation: object) -> None:
    try:
        operation()  # type: ignore[operator]
    except Exception as error:  # noqa: BLE001 - smoke test records the boundary error
        print(f"PASS {label}: {type(error).__name__}: {error}")
        return
    raise AssertionError(f"{label} unexpectedly succeeded")


def main() -> None:
    args = parse_args()
    try:
        import atm_graft
    except ImportError as error:
        raise SystemExit(
            "atm_graft is not installed; run maturin develop for "
            "crates/atm-graft-python first"
        ) from error
    if args.agent == args.sender:
        raise SystemExit("--agent and --sender must identify different registered agents")
    if args.timeout <= 0:
        raise SystemExit("--timeout must be positive")

    sender = atm_graft.PyAgentAddress(args.sender, args.team, None)
    receiver = atm_graft.PyAgentAddress(args.agent, args.team, args.chat_id)
    options = atm_graft.PyGraftSessionOptions(args.workspace_root, args.agent, args.team)
    print(f"PASS address: {sender} -> {receiver}")
    print(
        "PASS options: "
        f"workspace_root={options.workspace_root} agent={options.agent} team={options.team}"
    )

    expect_error(
        "blank workspace validation",
        lambda: atm_graft.PyGraftSessionOptions("   ", args.agent, args.team),
    )
    expect_error(
        "invalid nudge validation",
        lambda: atm_graft.PyNudge("not-a-message-id", receiver, "body"),
    )

    receiver_session = atm_graft.PyGraftSession(receiver)
    sender_session = atm_graft.PyGraftSession(sender)
    nudges: list[atm_graft.PyNudge] = []
    nudge_ready = threading.Event()

    def on_nudge(nudge: atm_graft.PyNudge) -> None:
        nudges.append(nudge)
        nudge_ready.set()

    try:
        expect_error("snapshot before activation", receiver_session.snapshot)
        receiver_session.activate_receiver(options, on_nudge)
        snapshot = receiver_session.snapshot()
        assert snapshot.agent == args.agent, snapshot.agent
        assert snapshot.team == args.team, snapshot.team
        assert snapshot.state == "listening", snapshot.state
        print(f"PASS receiver activation: state={snapshot.state}")
        expect_error(
            "duplicate receiver activation",
            lambda: receiver_session.activate_receiver(options, on_nudge),
        )

        marker = f"hermes-graft-smoke-{uuid.uuid4()}"
        sender_session.send(receiver, marker)
        print(f"PASS send: marker={marker}")

        if not nudge_ready.wait(args.timeout):
            raise TimeoutError(f"no graft nudge arrived within {args.timeout:.1f}s")
        assert len(nudges) == 1, f"expected one nudge, got {len(nudges)}"
        nudge = nudges[0]
        assert nudge.source.agent == args.sender, nudge.source.agent
        assert nudge.source.team == args.team, nudge.source.team
        assert marker in nudge.body, nudge.body
        print(
            "PASS nudge callback: "
            f"message_id={nudge.message_id} source={nudge.source} body={nudge.body!r}"
        )

        deadline = time.monotonic() + args.timeout
        messages: list[atm_graft.PyMessage] = []
        while time.monotonic() < deadline:
            messages = receiver_session.read()
            if any(message.body == marker for message in messages):
                break
        message = next((message for message in messages if message.body == marker), None)
        if message is None or message.message_id is None:
            raise AssertionError("read did not return the sent message with an ATM id")
        assert message.source.agent == args.sender, message.source.agent
        assert message.source.team == args.team, message.source.team
        print(
            "PASS read: "
            f"message_id={message.message_id} source={message.source} body={message.body!r}"
        )

        acknowledgement = f"ack-{marker}"
        receiver_session.acknowledge(message.message_id, acknowledgement)
        deadline = time.monotonic() + args.timeout
        while time.monotonic() < deadline:
            replies = sender_session.read()
            if any(
                reply.body == acknowledgement
                and reply.source.agent == args.agent
                and reply.source.team == args.team
                for reply in replies
            ):
                break
        else:
            raise AssertionError("acknowledgement reply was not delivered to the sender")
        print(f"PASS acknowledge round trip: message_id={message.message_id}")
    finally:
        sender_session.close()
        receiver_session.close()

    expect_error("snapshot after close", receiver_session.snapshot)
    print("PASS close: sender and receiver sessions closed")
    print("Hermes graft smoke test: PASS")


if __name__ == "__main__":
    main()
