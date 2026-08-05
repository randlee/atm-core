from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_runtime_observation_boundary import collect_runtime_observation_boundary_violations


TOKENS = ("ActivityObservation", "RuntimeMemberObservation", "RuntimeObservationSource")


class RuntimeObservationBoundaryTests(unittest.TestCase):
    def test_guard_accepts_phase_aj_sources(self) -> None:
        root = Path(__file__).resolve().parents[2]
        result = subprocess.run(
            [sys.executable, str(root / ".just/check_runtime_observation_boundary.py")],
            cwd=root,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_forbidden_consumer_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            source = root / "crates/atm-daemon/src/runtime_health/peer_authority.rs"
            source.parent.mkdir(parents=True)
            source.write_text("pub fn bypass() -> RuntimeObservationSource { todo!() }\n", encoding="utf-8")
            violations = collect_runtime_observation_boundary_violations(
                root, tokens=TOKENS, allowed_paths=frozenset(), required_positive=()
            )
            self.assertEqual(violations[0].kind, "source_use_not_allowed")

    def test_missing_required_positive_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            violations = collect_runtime_observation_boundary_violations(
                root,
                tokens=TOKENS,
                allowed_paths=frozenset(),
                required_positive=(("crates/atm-core/src/read/mod.rs", "ReadQuery"),),
            )
            self.assertEqual(violations[0].kind, "required_positive_missing")

    def test_cfg_test_source_is_not_scanned(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            source = root / "crates/example/src/lib.rs"
            source.parent.mkdir(parents=True)
            source.write_text(
                "#[cfg(test)]\nmod tests {\n fn fixture() { let _ = ActivityObservation; }\n}\n",
                encoding="utf-8",
            )
            violations = collect_runtime_observation_boundary_violations(
                root, tokens=TOKENS, allowed_paths=frozenset(), required_positive=()
            )
            self.assertEqual(violations, [])


if __name__ == "__main__":
    unittest.main()
