from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest


SCRIPT_PATH = Path(__file__).resolve().parents[2] / "scripts" / "check-nudge-taxonomy.py"
SPEC = importlib.util.spec_from_file_location("check_nudge_taxonomy", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
CHECK = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = CHECK
SPEC.loader.exec_module(CHECK)


class CheckNudgeTaxonomyTests(unittest.TestCase):
    def test_test_only_sources_and_inline_modules_are_exempt(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            (root / "crates/example/tests").mkdir(parents=True)
            (root / "crates/example/src").mkdir(parents=True)
            (root / "crates/example/tests/integration.rs").write_text(
                "fn integration_nudge_identifier() {}\n", encoding="utf-8"
            )
            (root / "crates/example/src/fixture_tests.rs").write_text(
                "fn fixture_nudge_identifier() {}\n", encoding="utf-8"
            )
            (root / "crates/example/src/lib.rs").write_text(
                "#[cfg(test)]\n"
                "mod nudge_mode_tests {\n"
                "    fn inline_nudge_identifier() {}\n"
                "}\n"
                "fn production_nudge_identifier() {}\n"
                "fn another_nudge_tests() {}\n",
                encoding="utf-8",
            )

            violations = CHECK.find_violations(root)

        self.assertEqual(
            [(violation.path.as_posix(), violation.line_number) for violation in violations],
            [("crates/example/src/lib.rs", 5)],
        )
        self.assertIn("production_nudge_identifier", violations[0].line)

    def test_computed_backend_type_literal_outside_owners_is_flagged(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            core_dir = root / "crates/atm-core/src/team_admin"
            core_dir.mkdir(parents=True)
            (root / "crates/atm-core/src").mkdir(parents=True, exist_ok=True)
            (core_dir / "member_mutation.rs").write_text(
                'extra.insert("backendType".to_string(), json!("tmux"));\n',
                encoding="utf-8",
            )
            (root / "crates/atm-core/src/delivery_channel.rs").write_text(
                'pub(crate) const BACKEND_TYPE_METADATA_KEY: &str = "backendType";\n',
                encoding="utf-8",
            )
            other_dir = root / "crates/atm-daemon-bootstrap/src"
            other_dir.mkdir(parents=True)
            (other_dir / "queue_drain.rs").write_text(
                "#[cfg(test)]\n"
                "mod tests {\n"
                "    fn entry() {\n"
                '        metadata.insert(["backend", "Type"].concat(), json!("tmux"));\n'
                "    }\n"
                "}\n",
                encoding="utf-8",
            )
            other_dir2 = root / "crates/atm-http-runtime/src"
            other_dir2.mkdir(parents=True)
            (other_dir2 / "herdr_queue_wake.rs").write_text(
                'metadata.insert("backend" + "Type", json!("herdr"));\n',
                encoding="utf-8",
            )

            violations = CHECK.find_violations(root)

        computed_violations = [
            violation
            for violation in violations
            if "backend_type_containment_gate" in violation.label
        ]
        self.assertEqual(
            sorted(v.path.as_posix() for v in computed_violations),
            [
                "crates/atm-daemon-bootstrap/src/queue_drain.rs",
                "crates/atm-http-runtime/src/herdr_queue_wake.rs",
            ],
        )

    def test_canonical_owners_with_real_literal_still_pass(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            core_dir = root / "crates/atm-core/src/team_admin"
            core_dir.mkdir(parents=True)
            (core_dir / "member_mutation.rs").write_text(
                'extra.insert("backendType".to_string(), json!("tmux"));\n',
                encoding="utf-8",
            )
            (root / "crates/atm-core/src/delivery_channel.rs").write_text(
                'pub(crate) const BACKEND_TYPE_METADATA_KEY: &str = "backendType";\n'
                "pub fn test_backend_type_metadata(backend: &str) -> serde_json::Map<String, Value> {\n"
                "    let mut metadata = serde_json::Map::new();\n"
                "    metadata.insert(BACKEND_TYPE_METADATA_KEY.to_owned(), Value::String(backend.to_owned()));\n"
                "    metadata\n"
                "}\n",
                encoding="utf-8",
            )
            other_dir = root / "crates/atm-daemon-bootstrap/src"
            other_dir.mkdir(parents=True)
            (other_dir / "queue_drain.rs").write_text(
                "fn entry(backend: Option<&str>) {\n"
                "    let metadata_json = backend.map_or_else("
                "Default::default, atm_core::delivery_channel::test_backend_type_metadata);\n"
                "}\n",
                encoding="utf-8",
            )

            violations = CHECK.find_violations(root)

        self.assertEqual(violations, ())


if __name__ == "__main__":
    unittest.main()
