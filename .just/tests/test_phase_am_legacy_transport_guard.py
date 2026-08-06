from __future__ import annotations

from pathlib import Path
import importlib.util
import subprocess
import sys
import tempfile
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts/phase-am/check_legacy_transport_removal.py"
SPEC = importlib.util.spec_from_file_location("phase_am_legacy_transport_guard", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise ImportError(f"unable to import {SCRIPT_PATH}")
GUARD = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = GUARD
SPEC.loader.exec_module(GUARD)


class PhaseAmLegacyTransportGuardTests(unittest.TestCase):
    def assert_reintroduced_symbol_fails(self, relative_path: str, source: str, category: str) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            target = root / relative_path
            target.parent.mkdir(parents=True)
            target.write_text(source, encoding="utf-8")
            violations = GUARD.find_violations(root)
        self.assertTrue(any(violation.category == category for violation in violations), violations)

    def test_raw_framing_mutation_fails(self) -> None:
        self.assert_reintroduced_symbol_fails(
            "crates/atm-core/src/api.rs", "let _ = HttpFrameReader::new();", "raw-framing"
        )

    def test_peer_only_ingress_mutation_fails(self) -> None:
        self.assert_reintroduced_symbol_fails(
            "crates/atm-core/src/api.rs", "let _ = PEER_SOURCE_HOST_HEADER;", "peer-ingress"
        )

    def test_resend_replay_mutation_fails(self) -> None:
        self.assert_reintroduced_symbol_fails(
            "crates/atm-daemon/src/lib.rs", "let _ = PeerDrainCoordinator;", "resend-replay"
        )

    def test_direct_sqlite_mutation_fails(self) -> None:
        self.assert_reintroduced_symbol_fails(
            "crates/atm-daemon/src/lib.rs", "use rusqlite::Connection;", "direct-sqlite"
        )

    def test_daemon_harness_mutation_fails(self) -> None:
        self.assert_reintroduced_symbol_fails(
            "crates/atm-daemon/src/lib.rs", "use atm_graft::GraftClient;", "daemon-harness"
        )

    def test_selected_category_ignores_other_category(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            target = root / "crates/atm-core/src/api.rs"
            target.parent.mkdir(parents=True)
            target.write_text("let _ = HttpFrameReader::new();", encoding="utf-8")
            violations = GUARD.find_violations(root, GUARD.rules_for_categories(("peer-ingress",)))
        self.assertEqual(violations, ())

    def test_cli_mutation_exits_nonzero(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            target = root / "crates/atm-daemon/src/lib.rs"
            target.parent.mkdir(parents=True)
            target.write_text("use rusqlite::Connection;", encoding="utf-8")
            result = subprocess.run(
                [sys.executable, str(SCRIPT_PATH), "--repo-root", str(root), "--category", "direct-sqlite"],
                check=False,
                capture_output=True,
                text=True,
            )
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_accepted_tmux_receiver_is_excluded(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            target = root / "crates/atm-daemon/src/message_received_emitter.rs"
            target.parent.mkdir(parents=True)
            target.write_text("fn receiver() { run_tmux_command(); }", encoding="utf-8")
            violations = GUARD.find_violations(root, GUARD.rules_for_categories(("daemon-harness",)))
        self.assertEqual(violations, ())

    def test_hyphenated_test_support_crate_is_excluded(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            target = root / "crates/atm-runtime-test-support/src/lib.rs"
            target.parent.mkdir(parents=True)
            target.write_text("let _ = PeerDrainCoordinator;", encoding="utf-8")
            sources = GUARD.iter_production_sources(root)
        self.assertNotIn(target, sources)

    def test_peer_observability_module_mutation_fails(self) -> None:
        self.assert_reintroduced_symbol_fails(
            "crates/atm-daemon/src/lib.rs", "mod peer_delivery_observability;", "resend-replay"
        )


if __name__ == "__main__":
    unittest.main()
