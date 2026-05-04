from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from run_lint import build_tasks
from run_lint import build_transcript
from run_lint import extract_count
from run_lint import LintResult
from run_lint import LintTask
from run_lint import preview_lines_for_task
from run_lint import prioritize_error_lines
from run_lint import resolve_task_names
from run_lint import strip_ansi


class RunLintTests(unittest.TestCase):
    ROOT_MANIFEST = """\
[workspace]
members = ["crates/atm-core"]
resolver = "2"
"""

    def test_resolve_task_names_all_includes_new_targets(self) -> None:
        names = resolve_task_names("all")
        self.assertIn("boundaries", names)
        self.assertIn("manifests", names)
        self.assertIn("deny", names)
        self.assertIn("shear", names)
        self.assertIn("spell", names)
        self.assertIn("pytests", names)

    def test_resolve_task_names_rejects_unknown_target(self) -> None:
        with self.assertRaises(ValueError):
            resolve_task_names("unknown")

    def test_extract_count_understands_total_violations(self) -> None:
        self.assertEqual(extract_count(["total violations: 58"]), 58)

    def test_prioritize_error_lines_prefers_actual_failures(self) -> None:
        lines = [
            "Updating crates.io index",
            "Downloaded crate",
            "error[E0432]: unresolved import `uuid`",
            "could not compile `agent-team-mail`",
        ]

        self.assertEqual(
            prioritize_error_lines(lines),
            [
                "error[E0432]: unresolved import `uuid`",
                "could not compile `agent-team-mail`",
            ],
        )

    def test_strip_ansi_and_prioritize_error_lines_handles_colored_cargo_output(self) -> None:
        lines = [
            strip_ansi("\x1b[1m\x1b[92m  Downloaded\x1b[0m thiserror v2.0.18"),
            strip_ansi("\x1b[1m\x1b[91merror[E0308]: mismatched types\x1b[0m"),
            strip_ansi("\x1b[1m\x1b[91merror:\x1b[0m could not compile `agent-team-mail`"),
        ]

        self.assertEqual(
            prioritize_error_lines(lines),
            [
                "error[E0308]: mismatched types",
                "error: could not compile `agent-team-mail`",
            ],
        )

    def test_build_tasks_contains_expected_commands(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            tasks = build_tasks(repo_root)
            self.assertEqual(tasks["modules"].command[-1], str(repo_root / ".just/lint_cargo_modules.py"))
            self.assertEqual(tasks["boundaries"].command[-1], str(repo_root / ".just/lint_boundaries.py"))
            self.assertEqual(tasks["manifests"].command[-1], str(repo_root / ".just/lint_manifests.py"))
            self.assertEqual(tasks["deny"].command[-1], str(repo_root / ".just/lint_cargo_deny.py"))
            self.assertEqual(tasks["shear"].command[-1], str(repo_root / ".just/lint_cargo_shear.py"))
            self.assertEqual(tasks["spell"].command[-1], str(repo_root / ".just/lint_codespell.py"))
            self.assertEqual(tasks["pytests"].command[-1], str(repo_root / ".just/run_pytests.py"))

    def test_resolve_task_names_fast_is_low_latency_subset(self) -> None:
        self.assertEqual(
            resolve_task_names("fast"),
            ["fmt", "version", "boundaries", "manifests", "spell", "pytests"],
        )

    def test_build_transcript_adds_crate_inventory_for_crate_scoped_lints(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Cargo.toml").write_text(self.ROOT_MANIFEST, encoding="utf-8")
            (repo_root / ".just").mkdir()
            (repo_root / ".just/lint-config.toml").write_text(
                "[boundaries]\ndoc_glob = \"docs/*/boundaries.md\"\n",
                encoding="utf-8",
            )
            crate_dir = repo_root / "crates" / "atm-core"
            crate_dir.mkdir(parents=True)
            (crate_dir / "Cargo.toml").write_text(
                """\
[package]
name = "agent-team-mail-core"
version = "1.1.2"
""",
                encoding="utf-8",
            )
            for lint_name in ("fmt", "clippy", "boundaries", "manifests"):
                result = LintResult(
                    task=LintTask(lint_name, ["just", f"_lint-{lint_name}"]),
                    returncode=0,
                    stdout="",
                    stderr="",
                    duration_seconds=0.2,
                    log_path=repo_root / ".just/logs/example.log",
                )

                transcript = build_transcript(result.task, result, repo_root)

                self.assertIn("crates analyzed:", transcript)
                joined = "\n".join(transcript)
                self.assertIn("crate_path", joined)
                self.assertIn("atm-core", joined)

    def test_preview_lines_for_identities_skips_inventory_rows(self) -> None:
        lines = [
            "crates analyzed:",
            "crate     package               crate_path  manifest",
            "atm       agent-team-mail       atm         crates/atm/Cargo.toml",
            "atm-core  agent-team-mail-core  atm_core    crates/atm-core/Cargo.toml",
            "RULE-008/RULE-009 violation: raw production literals found in test/cfg(test) Rust code.",
            "crates/atm/tests/ack.rs:28: let fixture = Fixture::new(&[\"arch-ctm\", \"team-lead\"]);",
            "total violations: 880",
        ]

        self.assertEqual(
            preview_lines_for_task("identities", lines),
            [
                "RULE-008/RULE-009 violation: raw production literals found in test/cfg(test) Rust code.",
                "crates/atm/tests/ack.rs:28: let fixture = Fixture::new(&[\"arch-ctm\", \"team-lead\"]);",
            ],
        )

    def test_build_transcript_adds_boundary_doc_inventory_for_boundaries(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Cargo.toml").write_text(self.ROOT_MANIFEST, encoding="utf-8")
            (repo_root / ".just").mkdir()
            (repo_root / ".just/lint-config.toml").write_text(
                "[boundaries]\ndoc_glob = \"docs/*/boundaries.md\"\n",
                encoding="utf-8",
            )
            crate_dir = repo_root / "crates" / "atm-core"
            crate_dir.mkdir(parents=True)
            (crate_dir / "Cargo.toml").write_text(
                """\
[package]
name = "agent-team-mail-core"
version = "1.1.2"
""",
                encoding="utf-8",
            )
            docs_dir = repo_root / "docs" / "atm-core"
            docs_dir.mkdir(parents=True)
            (docs_dir / "boundaries.md").write_text(
                """\
# Boundaries

```yaml
boundary_id: BOUNDARY-Test
owner_package: atm-core
owner_crate_path: atm_core
name: TestBoundary

public:
  trait: TestTrait
  facade: null

implementation:
  type: null
  module: null
  visibility: trait_only
  constructor: none

composition:
  roots: []

ownership:
  io_owns:
    - sqlite
  io_forbidden:
    - sockets

dependencies:
  allowed_dependents:
    - atm
  allowed_dependencies:
    - atm-core
  forbidden_edges:
    - atm -> atm-rusqlite

references:
  scope: outside_owner_crate
  forbidden:
    - TestImpl

contracts:
  request_types: []
  response_types: []
  error_types:
    - AtmError

testing:
  allowed_test_double_paths:
    - atm_core::tests::TestDouble
  forbidden_test_bypasses:
    - rusqlite::Connection

enforcement:
  lint_rules:
    - LINT_BOUNDARY_TEST
  review_gates:
    - no_public_impl

status:
  state: planned
  notes: []
```
""",
                encoding="utf-8",
            )
            result = LintResult(
                task=LintTask("boundaries", ["just", "_lint-boundaries"]),
                returncode=0,
                stdout="boundaries passed [0.10s]\n",
                stderr="",
                duration_seconds=0.2,
                log_path=repo_root / ".just/logs/example.log",
            )

            transcript = build_transcript(result.task, result, repo_root)
            joined = "\n".join(transcript)

            self.assertIn("boundary docs analyzed:", joined)
            self.assertIn("docs/atm-core/boundaries.md", joined)
            self.assertIn("boundary records validated: 1", joined)


if __name__ == "__main__":
    unittest.main()
