from __future__ import annotations

import io
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from run_lint import build_tasks
from run_lint import build_transcript
from run_lint import console_safe_text
from run_lint import extract_count
from run_lint import extract_error_fail_preview
from run_lint import failure_preview
from run_lint import LintResult
from run_lint import LintTask
from run_lint import partition_python_tasks
from run_lint import preview_lines_for_task
from run_lint import prioritize_error_lines
from run_lint import print_result
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
        self.assertIn("unix-gating", names)
        self.assertIn("runtime-waits", names)
        self.assertIn("manifests", names)
        self.assertIn("daemon-signing-coupling", names)
        self.assertIn("silent-emit", names)
        self.assertIn("function-length", names)
        self.assertIn("legacy-mailbox-paths", names)
        self.assertIn("nudge-taxonomy", names)
        self.assertIn("capability-degradation", names)
        self.assertIn("deny", names)
        self.assertIn("shear", names)
        self.assertIn("arch-gates", names)
        self.assertIn("fixed-sleep", names)
        self.assertIn("ttl-triage", names)
        self.assertIn("spell", names)
        self.assertIn("daemon-singleton", names)
        self.assertIn("hermes-adapter", names)
        self.assertIn("pytests", names)
        self.assertIn("sc-boundary", names)
        self.assertIn("sc-portability", names)
        self.assertIn("read-concurrency-gates", names)

    def test_resolve_task_names_rejects_unknown_target(self) -> None:
        with self.assertRaises(ValueError):
            resolve_task_names("unknown")

    def test_resolve_task_names_accepts_sc_boundary_manual_target(self) -> None:
        self.assertEqual(resolve_task_names("sc-boundary"), ["sc-boundary"])

    def test_resolve_task_names_accepts_sc_portability_manual_target(self) -> None:
        self.assertEqual(resolve_task_names("sc-portability"), ["sc-portability"])

    def test_extract_count_understands_total_violations(self) -> None:
        self.assertEqual(extract_count(["total violations: 58"]), 58)

    def test_console_safe_text_escapes_characters_missing_from_windows_code_pages(self) -> None:
        self.assertEqual(console_safe_text("replacement: \ufffd", "cp1252"), r"replacement: \ufffd")
        self.assertEqual(console_safe_text("replacement: \ufffd", "utf-8"), "replacement: \ufffd")

    def test_failed_lint_report_does_not_crash_on_windows_console_encoding(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            result = LintResult(
                task=LintTask("pytests", ["python", "tests.py"]),
                returncode=1,
                stdout="FAIL: replacement: \ufffd",
                stderr="",
                duration_seconds=0.1,
                log_path=repo_root / ".just/logs/pytests.log",
            )
            output = io.TextIOWrapper(io.BytesIO(), encoding="cp1252")
            with mock.patch("sys.stdout", output):
                print_result(result, repo_root)
            output.flush()
            self.assertIn(br"replacement: \ufffd", output.buffer.getvalue())

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

    def test_failure_preview_keeps_python_test_traceback_context(self) -> None:
        lines = [f"setup {index}" for index in range(45)]
        lines.extend(
            [
                "FAIL: test_expected_behavior",
                "Traceback (most recent call last):",
                "AssertionError: expected value",
                "FAILED (failures=1)",
            ]
        )

        preview = failure_preview("pytests", lines)

        self.assertEqual(
            preview,
            [
                "FAIL: test_expected_behavior",
                "Traceback (most recent call last):",
                "AssertionError: expected value",
                "FAILED (failures=1)",
            ],
        )
        self.assertIn("FAIL: test_expected_behavior", preview)

    def test_failure_preview_falls_back_to_tail_when_no_blocks_found(self) -> None:
        lines = [f"setup {index}" for index in range(45)]
        lines.append("Traceback (most recent call last):")
        lines.append("ImportError: could not import test module")

        preview = failure_preview("pytests", lines)

        self.assertEqual(preview, lines[-40:])

    def test_extract_error_fail_preview_surfaces_every_test_id(self) -> None:
        # A realistic unittest.TextTestRunner failure report: every ERROR
        # block prints before any FAIL block, and there are more blocks
        # than would fit in a tail-only window.
        lines = [
            "======================================================================",
            "ERROR: test_alpha (test_transfer_scripts.SftpShTests.test_alpha)",
            "----------------------------------------------------------------------",
            "Traceback (most recent call last):",
            '  File "test_transfer_scripts.py", line 10, in test_alpha',
            "    subprocess.run([str(self.SCRIPT), *args])",
            "OSError: [WinError 193] %1 is not a valid Win32 application",
            "======================================================================",
            "ERROR: test_beta (test_transfer_scripts.SftpShTests.test_beta)",
            "----------------------------------------------------------------------",
            "Traceback (most recent call last):",
            "OSError: [WinError 193] %1 is not a valid Win32 application",
            "======================================================================",
            "FAIL: test_gamma (test_transfer_scripts.SftpPs1Tests.test_gamma)",
            "----------------------------------------------------------------------",
            "Traceback (most recent call last):",
            "AssertionError: 1 != 0",
            "----------------------------------------------------------------------",
            "Ran 776 tests in 56.680s",
            "FAILED (failures=1, errors=2, skipped=14)",
        ]

        preview = extract_error_fail_preview(lines)

        self.assertIn("ERROR: test_alpha (test_transfer_scripts.SftpShTests.test_alpha)", preview)
        self.assertIn("ERROR: test_beta (test_transfer_scripts.SftpShTests.test_beta)", preview)
        self.assertIn("FAIL: test_gamma (test_transfer_scripts.SftpPs1Tests.test_gamma)", preview)
        # Traceback bodies stay attached to their own id, not bled into the
        # neighboring block.
        alpha_index = preview.index("ERROR: test_alpha (test_transfer_scripts.SftpShTests.test_alpha)")
        beta_index = preview.index("ERROR: test_beta (test_transfer_scripts.SftpShTests.test_beta)")
        self.assertLess(alpha_index, beta_index)
        self.assertNotIn("Ran 776 tests in 56.680s", preview)
        self.assertNotIn("FAILED (failures=1, errors=2, skipped=14)", preview)

    def test_extract_error_fail_preview_bounds_each_traceback_and_total_lines(self) -> None:
        long_traceback = [f"    frame {index}" for index in range(30)]
        lines = ["ERROR: test_one (module.Case.test_one)", *long_traceback]

        preview = extract_error_fail_preview(lines, block_lines=5, max_total_lines=400)

        self.assertEqual(preview, ["ERROR: test_one (module.Case.test_one)", *long_traceback[:4]])

    def test_extract_error_fail_preview_caps_total_output_across_many_blocks(self) -> None:
        lines = []
        for index in range(50):
            lines.append(f"ERROR: test_{index} (module.Case.test_{index})")
            lines.append("Traceback (most recent call last):")
            lines.append(f"RuntimeError: failure {index}")

        preview = extract_error_fail_preview(lines, block_lines=3, max_total_lines=30)

        self.assertLessEqual(len(preview), 31)
        self.assertTrue(any("truncated" in line for line in preview))

    def test_failure_preview_keeps_other_lints_concise(self) -> None:
        lines = ["progress", "error: one", "error: two", "error: three", "error: four", "error: five"]

        self.assertEqual(
            failure_preview("boundaries", lines),
            ["error: one", "error: two", "error: three", "error: four"],
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
            self.assertEqual(tasks["unix-gating"].command[-1], str(repo_root / ".just/lint_unix_gating.py"))
            self.assertEqual(tasks["runtime-waits"].command[-1], str(repo_root / ".just/lint_runtime_waits.py"))
            self.assertEqual(tasks["sc-boundary"].command[-1], str(repo_root / ".just/lint_sc_boundary.py"))
            self.assertEqual(tasks["sc-portability"].command[-1], str(repo_root / ".just/lint_sc_portability.py"))
            self.assertEqual(tasks["manifests"].command[-1], str(repo_root / ".just/lint_manifests.py"))
            self.assertEqual(
                tasks["daemon-signing-coupling"].command[-1],
                str(repo_root / ".just/lint_daemon_signing_coupling.py"),
            )
            self.assertEqual(
                tasks["silent-emit"].command[-1],
                str(repo_root / "scripts/check-silent-emit.py"),
            )
            self.assertEqual(
                tasks["function-length"].command[-1],
                str(repo_root / "scripts/check-function-length.py"),
            )
            self.assertEqual(
                tasks["legacy-mailbox-paths"].command[-1],
                str(repo_root / "scripts/check-legacy-mailbox-paths.py"),
            )
            self.assertEqual(
                tasks["nudge-taxonomy"].command[-1],
                str(repo_root / "scripts/check-nudge-taxonomy.py"),
            )
            self.assertEqual(
                tasks["capability-degradation"].command[-1],
                str(repo_root / "scripts/check-capability-degradation.py"),
            )
            self.assertEqual(tasks["deny"].command[-1], str(repo_root / ".just/lint_cargo_deny.py"))
            self.assertEqual(tasks["shear"].command[-1], str(repo_root / ".just/lint_cargo_shear.py"))
            self.assertEqual(tasks["arch-gates"].command, ["cargo", "test", "-p", "atm-architecture", "--quiet"])
            self.assertEqual(
                tasks["fixed-sleep"].command[-1],
                str(repo_root / ".just/check_fixed_sleep_hygiene.py"),
            )
            self.assertEqual(
                tasks["ttl-triage"].command[-1],
                str(repo_root / ".just/lint_ttl_triage_consistency.py"),
            )
            self.assertEqual(tasks["spell"].command[-1], str(repo_root / ".just/lint_codespell.py"))
            self.assertEqual(
                tasks["hermes-adapter"].command[-1],
                str(repo_root / ".just/lint_hermes_adapter.py"),
            )
            self.assertEqual(
                tasks["daemon-singleton"].command[-1],
                str(repo_root / "scripts/lint_daemon_singleton.py"),
            )
            self.assertEqual(tasks["pytests"].command[-1], str(repo_root / ".just/run_pytests.py"))

    def test_resolve_task_names_fast_is_low_latency_subset(self) -> None:
        self.assertEqual(
            resolve_task_names("fast"),
            [
                "fmt",
                "version",
                "boundaries",
                "manifests",
                "daemon-signing-coupling",
                "shear",
                "silent-emit",
                "function-length",
                "legacy-mailbox-paths",
                "nudge-taxonomy",
                "capability-degradation",
                "spell",
                "hermes-adapter",
                "pytests",
            ],
        )

    def test_partition_python_tasks_runs_pytests_after_independent_lints(self) -> None:
        version = LintTask("version", ["python3", "version.py"])
        pytests = LintTask("pytests", ["python3", "run_pytests.py"])
        boundaries = LintTask("boundaries", ["python3", "boundaries.py"])

        parallel, serial = partition_python_tasks([version, pytests, boundaries])

        self.assertEqual(parallel, [version, boundaries])
        self.assertEqual(serial, [pytests])

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
            for lint_name in ("fmt", "clippy", "boundaries", "sc-boundary", "manifests"):
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
            "RULE-008/RULE-009 violation: raw production literals found in Rust code.",
            "crates/atm/tests/ack.rs:28: let fixture = Fixture::new(&[\"arch-ctm\", \"team-lead\"]);",
            "total violations: 880",
        ]

        self.assertEqual(
            preview_lines_for_task("identities", lines),
            [
                "RULE-008/RULE-009 violation: raw production literals found in Rust code.",
                "crates/atm/tests/ack.rs:28: let fixture = Fixture::new(&[\"arch-ctm\", \"team-lead\"]);",
            ],
        )

    def test_preview_lines_for_sc_boundary_skips_wrapper_banner(self) -> None:
        self.assertEqual(
            preview_lines_for_task(
                "sc-boundary",
                [
                    "sc-boundary failed",
                    "architectural cycle across owners: A, B",
                    "full log: .just/logs/example.log",
                ],
            ),
            ["architectural cycle across owners: A, B"],
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
