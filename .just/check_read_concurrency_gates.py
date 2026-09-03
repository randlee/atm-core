#!/usr/bin/env python3
"""AV.3 static gate for the post-cutover read-concurrency contract.

The AV.1b cutover removes the legacy writer-routed ``ListMessages`` operation
and introduces ``AsyncMailboxRuntime``.  AV.3 is intentionally stacked while
that implementation is in flight, so this checker is inert only until that
cutover marker exists.  From then on it is a normal ``just lint`` gate.
"""

from __future__ import annotations

from pathlib import Path
import argparse
import re
import sys

from lint_common import discover_repo_root
from rust_source import extract_fn_body


ROUTER = Path("crates/atm-http-runtime/src/storage_and_nudge_router.rs")
WRITE_OPS = Path("crates/atm-storage-rusqlite/src/writer/ops.rs")
ALLOWED_WRITE_OPS = {
    "UpsertMessage",
    "UpsertMessages",
    "Acknowledge",
    "RegisterTemplate",
    "AdmitDecomposedMessage",
    "AdmitTemplateMessage",
    "ApplyReadDisplayState",
}
HANDLER_LIST = Path(__file__).with_name("allowlists") / "read_concurrency_handlers.txt"
READ_HANDLERS = tuple(
    line for line in HANDLER_LIST.read_text(encoding="utf-8").splitlines() if line
)
PROHIBITED_READ_HANDLER_TERMS = (
    "BlockingCoreBridge",
    "ControlPathSyncBridge",
    "spawn_blocking",
)
REQUIRED_LIVENESS_TESTS = (
    "mailbox_and_doctor_fanout_stays_live_while_the_legacy_bridge_is_occupied",
    "doctor_projection_serves_parallel_control_requests_without_the_read_bridge",
    "doctor_projection_rejects_control_lane_overload_explicitly",
    "read_family_uses_only_the_supervised_recording_writer_handoff",
)


def write_op_variants(source: str) -> set[str]:
    enum_match = re.search(r"pub\(crate\)\s+enum\s+WriteOp\s*\{(?P<body>.*?)\n\}", source, re.DOTALL)
    if enum_match is None:
        raise ValueError("WriteOp enum is missing")
    return set(re.findall(r"^\s{4}([A-Z][A-Za-z0-9_]*)", enum_match.group("body"), re.MULTILINE))


def check(root: Path) -> list[str]:
    router = (root / ROUTER).read_text(encoding="utf-8")
    # AV.1b is the atomic activation point.  Before it lands, ListMessages is
    # an acknowledged pre-cutover baseline rather than an AV.3 regression.
    if "AsyncMailboxRuntime" not in router:
        return []

    findings: list[str] = []
    variants = write_op_variants((root / WRITE_OPS).read_text(encoding="utf-8"))
    unexpected = sorted(variants - ALLOWED_WRITE_OPS)
    missing = sorted(ALLOWED_WRITE_OPS - variants)
    if unexpected or missing:
        findings.append(
            "WriteOp must contain only AV.1b mutation operations; "
            f"unexpected={unexpected}, missing={missing}"
        )

    for handler in READ_HANDLERS:
        body = extract_fn_body(router, handler)
        for term in PROHIBITED_READ_HANDLER_TERMS:
            if term in body:
                findings.append(f"read handler `{handler}` references prohibited `{term}`")
    for test_name in REQUIRED_LIVENESS_TESTS:
        test_match = re.search(
            rf"(?P<attributes>(?:\s*#\[[^\]]+\]\s*)*)\s*(?:async\s+)?fn\s+{test_name}\s*\(",
            router,
        )
        if test_match is None:
            findings.append(f"required AV.1b liveness test `{test_name}` is missing")
        elif "ignore" in test_match.group("attributes"):
            findings.append(f"required AV.1b liveness test `{test_name}` must not be ignored")
    return findings


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", help="repository root (defaults to discovery from cwd)")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    root = discover_repo_root(parse_args(argv).root)
    try:
        findings = check(root)
    except (OSError, ValueError) as error:
        print(f"AV.3 read-concurrency gate could not inspect source: {error}")
        return 1
    if findings:
        print("AV.3 read-concurrency gate failed:")
        for finding in findings:
            print(f"- {finding}")
        return 1
    print("AV.3 read-concurrency gate passed (pre-cutover activation is expected until AV.1b lands).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
