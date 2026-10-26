from __future__ import annotations

from collections.abc import Callable
from concurrent.futures import Future
import io
import importlib.util
import json
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
from run_lint import main
from run_lint import preview_lines_for_task
from run_lint import plan_lanes
from run_lint import prioritize_error_lines
from run_lint import print_result
from run_lint import resolve_task_names
from run_lint import run_parallel
from run_lint import strip_ansi
from run_tests import load_test_manifest
from run_tests import selected_test_commands


class RunLintTests(unittest.TestCase):
    ROOT_MANIFEST = """\
[workspace]
members = ["crates/atm-core"]
resolver = "2"
"""

    def test_resolve_task_names_default_includes_new_targets(self) -> None:
        names = resolve_task_names("default")
        self.assertIn("boundaries", names)
        self.assertIn("sc-runtime", names)
        self.assertIn("manifests", names)
        self.assertIn("daemon-signing-coupling", names)
        self.assertIn("silent-emit", names)
        self.assertIn("function-length", names)
        self.assertIn("kit-drift", names)
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

    def test_resolve_task_names_ci_host_scope_has_no_cross_target_step(self) -> None:
        for host_platform in ("linux", "darwin", "win32"):
            with self.subTest(host_platform=host_platform):
                names = resolve_task_names("ci", "host", host_platform)
                self.assertNotIn("clippy-windows", names)
                self.assertEqual(names, resolve_task_names("default", host_platform=host_platform))

    def test_resolve_task_names_ci_all_adds_clippy_windows_off_windows(self) -> None:
        for host_platform in ("linux", "darwin"):
            with self.subTest(host_platform=host_platform):
                names = resolve_task_names("ci", "all", host_platform)
                self.assertEqual(names[:3], ["fmt", "clippy", "clippy-windows"])
                self.assertEqual(names.count("clippy-windows"), 1)

    def test_resolve_task_names_accepts_clippy_windows_manual_target(self) -> None:
        self.assertEqual(resolve_task_names("clippy-windows"), ["clippy-windows"])

    def test_resolve_task_names_ci_all_on_windows_uses_native_clippy(self) -> None:
        names = resolve_task_names("ci", "all", "win32")
        self.assertIn("clippy", names)
        self.assertNotIn("clippy-windows", names)

    def test_main_runs_all_selected_lints_through_one_scheduler(self) -> None:
        names = resolve_task_names("ci", "all", "linux")
        tasks = {name: LintTask(name, ["true"]) for name in names}
        results = [LintResult(tasks[name], 0, "", "", 0.0, Path("log")) for name in names]

        with tempfile.TemporaryDirectory() as tempdir, \
            mock.patch("run_lint.resolve_task_names", lambda target, scope: resolve_task_names(target, scope, "linux")), \
            mock.patch("run_lint.build_tasks", return_value=tasks), \
            mock.patch("run_lint.run_parallel", return_value=results) as run_parallel, \
            mock.patch("run_lint.print_result"), \
            mock.patch("sys.stdout", new_callable=io.StringIO):
            exit_code = main(["run_lint.py", "ci", "all", "--root", tempdir])

        self.assertEqual(exit_code, 0)
        self.assertEqual([task.name for task in run_parallel.call_args.args[0]], names)

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

    @mock.patch("run_lint.lint_argv")
    def test_build_tasks_contains_expected_commands(self, lint_argv: mock.Mock) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            lint_argv.side_effect = lambda _root, analyzer, _args: (
                repo_root / "sc-lint",
                [str(repo_root / "sc-lint"), "lint", analyzer],
            )
            tasks = build_tasks(repo_root)
            self.assertEqual(
                tasks["clippy-windows"].command,
                ["just", "_sc-lint-clippy", "xwin", str(repo_root / "sc-lint")],
            )
            self.assertEqual(tasks["boundaries"].command[-1], str(repo_root / ".just/lint_boundaries.py"))
            self.assertEqual(tasks["sc-boundary"].command[-1], "sc-boundary")
            self.assertEqual(tasks["sc-portability"].command[-1], "sc-portability")
            self.assertEqual(tasks["sc-runtime"].command[-1], "sc-runtime")
            self.assertEqual(tasks["identities"].command[-1], "identity-literals")
            self.assertEqual(tasks["manifests"].command[1:3], ["-m", "sc_lint.lint_manifests"])
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
                "function-length",
            )
            self.assertEqual(
                tasks["kit-drift"].command,
                [
                    sys.executable,
                    "-B",
                    str(repo_root / "plugins/sc-lint/install.py"),
                    str(repo_root),
                    "--dry-run",
                ],
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
            self.assertEqual(tasks["shear"].command[1:3], ["-m", "sc_lint.lint_cargo_shear"])
            self.assertEqual(tasks["arch-gates"].command, ["cargo", "test", "-p", "atm-architecture", "--quiet"])
            self.assertEqual(
                tasks["fixed-sleep"].command[-1],
                str(repo_root / ".just/check_fixed_sleep_hygiene.py"),
            )
            self.assertEqual(
                tasks["ttl-triage"].command[-1],
                str(repo_root / ".just/lint_ttl_triage_consistency.py"),
            )
            self.assertEqual(tasks["spell"].command[1:3], ["-m", "sc_lint.lint_codespell"])
            self.assertEqual(
                tasks["hermes-adapter"].command[-1],
                str(repo_root / ".just/lint_hermes_adapter.py"),
            )
            self.assertEqual(
                tasks["daemon-singleton"].command[-1],
                str(repo_root / "scripts/lint_daemon_singleton.py"),
            )
            self.assertEqual(tasks["pytests"].command[-1], str(repo_root / ".just/run_pytests.py"))

    def test_resolve_task_names_rejects_removed_all_and_fast_modes(self) -> None:
        for removed in ("all", "fast"):
            with self.subTest(removed=removed), self.assertRaises(ValueError):
                resolve_task_names(removed)

    def test_test_manifest_default_excludes_coverage_and_selectors_are_named(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            manifest_dir = root / ".sc-lint"
            manifest_dir.mkdir()
            manifest = {
                "test": {
                    "default": ["just", "test"],
                    "python": ["python", "tests.py"],
                    "coverage": ["python", "coverage.py"],
                }
            }
            (manifest_dir / "install.json").write_text(json.dumps(manifest), encoding="utf-8")

            loaded = load_test_manifest(root)

        self.assertEqual(selected_test_commands(loaded, []), [("python", ["python", "tests.py"])])
        self.assertEqual(selected_test_commands(loaded, ["coverage"]), [("coverage", ["python", "coverage.py"])])
        with self.assertRaisesRegex(ValueError, "unknown test selector"):
            selected_test_commands(loaded, ["missing"])

    def test_coverage_reports_clear_linux_error_before_running_tools(self) -> None:
        coverage_path = Path(__file__).resolve().parents[2] / "scripts/coverage/run.py"
        spec = importlib.util.spec_from_file_location("atm_coverage_run", coverage_path)
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        coverage_run = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = coverage_run
        spec.loader.exec_module(coverage_run)
        error_output = io.StringIO()
        with mock.patch.object(coverage_run.host_platform_module, "system", return_value="Linux"), \
            mock.patch.object(coverage_run.sys, "argv", [str(coverage_path), "--write-artifacts"]), \
            mock.patch.object(coverage_run.sys, "stderr", error_output), \
            mock.patch.object(coverage_run, "run_coverage_export") as run_export:
            status = coverage_run.main()

        self.assertEqual(status, 2)
        self.assertIn("coverage reporting currently supports only macOS and Windows", error_output.getvalue())
        run_export.assert_not_called()

    def test_failure_summary_names_every_failed_lint(self) -> None:
        selected = [LintTask(name, [name]) for name in ("version", "spell", "pytests")]
        results = [
            LintResult(selected[0], 1, "error: version", "", 0.1, Path("version.log")),
            LintResult(selected[1], 0, "", "", 0.1, Path("spell.log")),
            LintResult(selected[2], 1, "FAIL: test_sample", "", 0.1, Path("pytests.log")),
        ]
        output = io.StringIO()
        with tempfile.TemporaryDirectory() as tempdir, \
            mock.patch("run_lint.resolve_task_names", return_value=[task.name for task in selected]), \
            mock.patch("run_lint.build_tasks", return_value={task.name: task for task in selected}), \
            mock.patch("run_lint.run_parallel", return_value=results), \
            mock.patch("sys.stdout", output):
            status = main(["run_lint.py", "default", "--root", tempdir])

        self.assertEqual(status, 1)
        self.assertIn("lint failed: 2 check(s) failed: version, pytests", output.getvalue())

    def test_parallel_scheduler_plans_serial_cargo_and_independent_lanes(self) -> None:
        tasks = [LintTask(name, [name]) for name in ("clippy", "version", "pytests", "spell", "arch-gates")]
        expected_cargo = [tasks[0], tasks[2], tasks[4]]
        expected_independent = [[tasks[1]], [tasks[3]]]

        lanes = plan_lanes(tasks)

        self.assertEqual(lanes, [expected_cargo, *expected_independent])
        flattened = [task for lane in lanes for task in lane]
        self.assertCountEqual(flattened, tasks)
        self.assertEqual(len(lanes), 3)

        worker_counts: list[int] = []
        submitted: list[
            tuple[Callable[..., list[LintResult]], tuple[object, ...], Future[list[LintResult]]]
        ] = []

        class DeferredExecutor:
            def __init__(self, max_workers: int) -> None:
                worker_counts.append(max_workers)

            def __enter__(self) -> DeferredExecutor:
                return self

            def __exit__(self, *_exc_info: object) -> None:
                return None

            def submit(
                self, function: Callable[..., list[LintResult]], *args: object
            ) -> Future[list[LintResult]]:
                future: Future[list[LintResult]] = Future()
                submitted.append((function, args, future))
                return future

        call_order: list[str] = []

        def fake_run_task(task: LintTask, _root: Path) -> LintResult:
            call_order.append(task.name)
            return LintResult(task, 0, "", "", 0.0, Path("log"))

        def fake_as_completed(
            futures: list[Future[list[LintResult]]],
        ) -> list[Future[list[LintResult]]]:
            self.assertEqual(len(submitted), len(lanes))
            self.assertEqual(futures, [future for _function, _args, future in submitted])
            self.assertEqual(worker_counts, [len(lanes)])

            cargo_function, cargo_args, cargo_future = submitted[0]
            independent_futures = []
            for function, args, future in submitted[1:]:
                future.set_result(function(*args))
                independent_futures.append(future)

            cargo_names = [task.name for task in expected_cargo]
            self.assertFalse(cargo_future.done())
            self.assertFalse(any(name in cargo_names for name in call_order))

            cargo_future.set_result(cargo_function(*cargo_args))
            cargo_positions = [index for index, name in enumerate(call_order) if name in cargo_names]
            self.assertEqual(cargo_positions, list(range(cargo_positions[0], cargo_positions[0] + len(cargo_names))))
            self.assertEqual(call_order[cargo_positions[0] : cargo_positions[-1] + 1], cargo_names)
            return [*independent_futures, cargo_future]

        run_task_mock = mock.Mock(side_effect=fake_run_task)
        with tempfile.TemporaryDirectory() as tempdir, \
            mock.patch("run_lint.ThreadPoolExecutor", DeferredExecutor), \
            mock.patch("run_lint.as_completed", side_effect=fake_as_completed), \
            mock.patch("run_lint.run_task", run_task_mock):
            results = run_parallel(tasks, Path(tempdir))

        self.assertEqual(worker_counts, [len(lanes)])
        self.assertEqual([result.task for result in results], tasks)
        self.assertEqual(call_order, ["version", "spell", "clippy", "pytests", "arch-gates"])
        self.assertEqual(run_task_mock.call_count, len(tasks))

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
