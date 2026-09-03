from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_peer_dial_seam import LOCKED_LINES
from lint_peer_dial_seam import SEAM
from lint_peer_dial_seam import collect_findings
from lint_peer_dial_seam import collect_lock_findings
from lint_peer_dial_seam import collect_seam_findings

REPO_ROOT = JUST_DIR.parent

ROOT_MANIFEST = """\
[workspace]
members = ["crates/atm-http-runtime", "crates/atm"]
resolver = "2"
"""


def crate_manifest(name: str) -> str:
    return f"""\
[package]
name = "{name}"
version = "0.1.0"

[lib]
name = "{name.replace('-', '_')}"
"""


class LintPeerDialSeamTests(unittest.TestCase):
    def write_repo(self, repo_root: Path) -> None:
        (repo_root / "Cargo.toml").write_text(ROOT_MANIFEST, encoding="utf-8")
        for crate in ("atm-http-runtime", "atm"):
            (repo_root / "crates" / crate / "src").mkdir(parents=True)
            (repo_root / "crates" / crate / "Cargo.toml").write_text(crate_manifest(crate), encoding="utf-8")
        # Every locked file carries all of its locked lines, so the lock
        # check starts green and each test breaks exactly one thing.
        for rel, expected_lines in LOCKED_LINES.items():
            (repo_root / rel).write_text("\n".join(expected_lines) + "\n", encoding="utf-8")

    def write_runtime_file(self, repo_root: Path, name: str, body: str) -> str:
        rel = f"crates/atm-http-runtime/src/{name}"
        (repo_root / rel).write_text(body, encoding="utf-8")
        return rel

    def test_real_repository_is_clean(self) -> None:
        self.assertEqual(collect_findings(REPO_ROOT), [])

    def test_clean_fixture_has_no_findings(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.assertEqual(collect_findings(repo_root), [])

    def test_resolution_outside_the_seam_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            rel = self.write_runtime_file(
                repo_root,
                "sneaky.rs",
                "pub async fn find() {\n    let _ = tokio::net::lookup_host((\"peer\", 1)).await;\n}\n",
            )
            findings = collect_seam_findings(repo_root)
            self.assertEqual(len(findings), 1)
            self.assertTrue(findings[0].startswith(f"{rel}:2: peer name resolution outside"))

    def test_every_dial_primitive_outside_the_seam_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_runtime_file(
                repo_root,
                "dialer.rs",
                "fn a() { TcpStream::connect(x); }\n"
                "fn b() { TcpStream::connect_timeout(x, t); }\n"
                "fn c() { TcpSocket::connect(s, x); }\n",
            )
            findings = collect_seam_findings(repo_root)
            self.assertEqual(len(findings), 3)
            self.assertTrue(all("peer TCP dial outside" in finding for finding in findings))

    def test_test_scope_and_seam_delegation_are_allowed(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            self.write_runtime_file(
                repo_root,
                "client_like.rs",
                "fn build() { builder.dns_resolver(Arc::new(crate::peer_dial::OrderedPeerResolver)); }\n"
                "#[cfg(test)]\nmod tests {\n    fn t() { let _ = TcpStream::connect(x); let _ = lookup_host(y); }\n}\n",
            )
            self.assertEqual(collect_seam_findings(repo_root), [])

    def test_adr_040_literal_ip_check_is_allowlisted(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / "crates/atm/src/commands").mkdir(parents=True)
            (repo_root / "crates/atm/src/commands/send.rs").write_text(
                "fn check() { let _ = (host, port).to_socket_addrs(); }\n", encoding="utf-8"
            )
            (repo_root / "crates/atm/src/other.rs").write_text(
                "fn leak() { let _ = (host, port).to_socket_addrs(); }\n", encoding="utf-8"
            )
            findings = collect_seam_findings(repo_root)
            self.assertEqual(len(findings), 1)
            self.assertTrue(findings[0].startswith("crates/atm/src/other.rs:1:"))

    def test_each_locked_line_is_individually_enforced(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            for rel, expected_lines in LOCKED_LINES.items():
                for dropped in expected_lines:
                    kept = [line for line in expected_lines if line != dropped]
                    (repo_root / rel).write_text("\n".join(kept) + "\n", encoding="utf-8")
                    findings = collect_lock_findings(repo_root)
                    self.assertEqual(len(findings), 1, dropped)
                    self.assertIn(dropped, findings[0])
                    self.assertTrue(findings[0].startswith(f"{rel}: locked ADR-060 line not found"))
                (repo_root / rel).write_text("\n".join(expected_lines) + "\n", encoding="utf-8")

    def test_drifted_arithmetic_is_caught_even_when_constants_survive(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            seam = repo_root / SEAM
            text = seam.read_text(encoding="utf-8").replace(
                "RequestDeadline::after((remaining / 2).min(STALE_ADDRESS_DIAL_CAP))",
                "RequestDeadline::after(remaining / 2)",
            )
            seam.write_text(text, encoding="utf-8")
            findings = collect_lock_findings(repo_root)
            self.assertEqual(len(findings), 1)
            self.assertIn(".min(STALE_ADDRESS_DIAL_CAP)", findings[0])

    def test_missing_locked_file_is_reported(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_repo(repo_root)
            (repo_root / SEAM).unlink()
            findings = collect_lock_findings(repo_root)
            self.assertEqual(findings, [f"{SEAM}: missing; ADR-060 locked file must exist"])


if __name__ == "__main__":
    unittest.main()
