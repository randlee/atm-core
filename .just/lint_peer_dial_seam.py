#!/usr/bin/env python3
"""peer-dial-seam: keep ADR-060 / REQ-CORE-TRANSPORT-002E from drifting.

Three checks, all on non-test Rust code:

1. Peer name resolution primitives (``lookup_host``, ``ToSocketAddrs``,
   ``dns_resolver``) appear only in the ADR-060 seam ``peer_dial.rs`` and the
   one ADR-040 CLI site (literal-IP-to-trusted-host check).
2. TCP dials (``TcpStream::connect``, ``TcpStream::connect_timeout``,
   ``TcpSocket::connect``) inside ``atm-http-runtime`` appear only in
   ``peer_dial.rs``; every peer dial goes through the seam.
3. The locked design lines exist verbatim: the dial constants *and the
   arithmetic that applies them*, the cache-key normalization, the default
   TTL, and the plaintext-test client installing ``OrderedPeerResolver``.
   Changing any of them requires a superseding ADR and an update to the
   expected lines below in the same change.
"""
from __future__ import annotations

import argparse
import re
import sys
from datetime import datetime, timezone
from pathlib import Path

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import is_code_line
from lint_common import iter_workspace_rust_files
from lint_common import monotonic_now
from lint_common import print_report
from lint_common import rust_file_test_scope
from lint_common import workspace_crate_section_lines

LINT_NAME = "peer-dial-seam"
SEAM = "crates/atm-http-runtime/src/peer_dial.rs"
RESOLUTION_RE = re.compile(r"\blookup_host\s*\(|\bto_socket_addrs\s*\(|\bToSocketAddrs\b|\.dns_resolver\s*\(")
RESOLUTION_ALLOWED = {
    SEAM,
    # ADR-040: CLI literal-IP input is authorized by a fresh lookup of the
    # registered hostnames. It is an authorization check, not a dial.
    "crates/atm/src/commands/send.rs",
}
DIAL_RE = re.compile(r"\b(?:TcpStream::connect(?:_timeout)?|TcpSocket::connect)\s*\(")
DIAL_SCOPE_PREFIX = "crates/atm-http-runtime/"
LOCKED_LINES: dict[str, tuple[str, ...]] = {
    SEAM: (
        "pub(crate) const MAX_DIAL_CANDIDATES: usize = 4;",
        "pub(crate) const DIAL_REPORT_GRACE: Duration = Duration::from_millis(250);",
        "pub(crate) const STALE_ADDRESS_DIAL_CAP: Duration = Duration::from_millis(500);",
        # Rule 5: cached dial bounded by min(half remaining, cap).
        "RequestDeadline::after((remaining / 2).min(STALE_ADDRESS_DIAL_CAP))",
        "let name = peer.as_str().to_ascii_lowercase();",
        '.strip_suffix(".local")',
        "address.ip().is_unicast_link_local() && address.scope_id() == 0",
        "impl reqwest::dns::Resolve for OrderedPeerResolver {",
    ),
    "crates/atm-http-runtime/src/peer_connection_pool.rs": (
        "address_cache_ttl: Duration::from_secs(5 * 60),",
        # Rule 7: the dial loop ends DIAL_REPORT_GRACE inside the budget.
        ".checked_sub(DIAL_REPORT_GRACE)",
        "self.shared.addresses.connect(",
    ),
    "crates/atm-http-runtime/src/client.rs": (
        ".dns_resolver(Arc::new(crate::peer_dial::OrderedPeerResolver))",
    ),
}


def collect_seam_findings(repo_root: Path) -> list[str]:
    """Resolution or dialing in non-test code outside the seam."""
    findings: list[str] = []
    for abs_path in iter_workspace_rust_files(repo_root):
        rel = abs_path.relative_to(repo_root).as_posix()
        lines = abs_path.read_text(encoding="utf-8").splitlines()
        scope = rust_file_test_scope(Path(rel), lines)
        for number, (line, in_test) in enumerate(zip(lines, scope, strict=True), start=1):
            if in_test or not is_code_line(line):
                continue
            # A line that hands resolution to the seam (`peer_dial::...`) is
            # the seam being used, not bypassed.
            if rel not in RESOLUTION_ALLOWED and RESOLUTION_RE.search(line) and "peer_dial::" not in line:
                findings.append(
                    f"{rel}:{number}: peer name resolution outside the ADR-060 seam ({SEAM}): {line.strip()}"
                )
            if rel.startswith(DIAL_SCOPE_PREFIX) and rel != SEAM and DIAL_RE.search(line):
                findings.append(
                    f"{rel}:{number}: peer TCP dial outside the ADR-060 seam ({SEAM}): {line.strip()}"
                )
    return findings


def collect_lock_findings(repo_root: Path) -> list[str]:
    """Locked ADR-060 lines missing from the files that must carry them."""
    findings: list[str] = []
    for rel, expected_lines in LOCKED_LINES.items():
        path = repo_root / rel
        if not path.is_file():
            findings.append(f"{rel}: missing; ADR-060 locked file must exist")
            continue
        text = path.read_text(encoding="utf-8")
        for expected in expected_lines:
            if expected not in text:
                findings.append(
                    f"{rel}: locked ADR-060 line not found (supersede ADR-060 and update this lint together): {expected}"
                )
    return findings


def collect_findings(repo_root: Path) -> list[str]:
    return [*collect_seam_findings(repo_root), *collect_lock_findings(repo_root)]


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Enforce the ADR-060 peer dial seam and locked design lines.")
    parser.add_argument("--root", help="Repo root to inspect.")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    repo_root = discover_repo_root(args.root)
    started_at = datetime.now(timezone.utc)
    start_time = monotonic_now()
    findings = collect_findings(repo_root)
    duration_seconds = monotonic_now() - start_time
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=not findings,
        summary=(
            "ADR-060 peer dial seam violated or locked design drifted"
            if findings
            else "peer dial seam intact and ADR-060 locked lines present"
        ),
        findings=findings,
        transcript_lines=[*workspace_crate_section_lines(repo_root), "findings:", *(findings or ["none"])],
        started_at=started_at,
        duration_seconds=duration_seconds,
    )
    print_report(report, repo_root=repo_root, preview_limit=5, direct_threshold=5)
    return 0 if report.passed else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
