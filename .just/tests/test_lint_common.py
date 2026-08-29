from __future__ import annotations

from contextlib import redirect_stdout
from datetime import datetime, timezone
from io import StringIO
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_common import LintReport
from lint_common import build_report
from lint_common import classify_rust_test_scope
from lint_common import discover_repo_root
from lint_common import format_duration
from lint_common import is_code_line
from lint_common import is_comment_line
from lint_common import iter_string_literal_contents
from lint_common import lint_slug
from lint_common import load_lint_config
from lint_common import make_log_path
from lint_common import print_report
from lint_common import relative_log_path
from lint_common import render_workspace_crate_table
from lint_common import resolve_posix_shell
from lint_common import rust_file_test_scope
from lint_common import strip_negated_cfg_segments
from lint_common import workspace_crates
from lint_common import workspace_manifest_paths
from lint_common import workspace_target_args
from lint_common import _is_system32_wsl_stub


ROOT_MANIFEST = """\
[workspace]
members = ["crates/lib-crate", "crates/bin-crate", "excluded/*"]
exclude = ["excluded/skip-me"]
resolver = "2"
"""


class LintCommonTests(unittest.TestCase):
    def test_resolve_posix_shell_prefers_bash_outside_windows(self) -> None:
        with mock.patch(
            "lint_common.shutil.which",
            side_effect=lambda name: {"bash": "/usr/bin/bash", "sh": "/bin/sh"}.get(name),
        ):
            self.assertEqual(resolve_posix_shell(windows=False), "/usr/bin/bash")

    def test_resolve_posix_shell_rejects_system32_wsl_stub(self) -> None:
        git_result = mock.Mock(stdout="C:\\Program Files\\Git\\mingw64\\libexec\\git-core\n")

        def is_git_bash(candidate: Path) -> bool:
            return str(candidate).replace("/", "\\").casefold() == (
                r"c:\program files\git\bin\bash.exe"
            )

        with (
            mock.patch("lint_common.subprocess.run", return_value=git_result) as run,
            mock.patch(
                "lint_common.shutil.which",
                side_effect=lambda name: r"C:\Windows\System32\bash.exe" if name == "bash" else None,
            ),
            mock.patch("lint_common.Path.is_file", new=is_git_bash),
        ):
            self.assertEqual(
                resolve_posix_shell(windows=True, windir=r"C:\Windows"),
                r"C:\Program Files\Git\bin\bash.exe",
            )

        self.assertEqual(run.call_args.kwargs["timeout"], 5)

    def test_resolve_posix_shell_ignores_ambient_git_bash(self) -> None:
        with (
            mock.patch.dict("lint_common.os.environ", {"GIT_BASH": r"C:\unsafe"}, clear=False),
            mock.patch("lint_common.subprocess.run", side_effect=OSError),
            mock.patch("lint_common.shutil.which", return_value=None),
        ):
            self.assertIsNone(resolve_posix_shell(windows=True))

    def test_system32_wsl_stub_rejection_resolves_prefix_and_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            system32 = root / "System32"
            system32.mkdir()
            (system32 / "Sub").mkdir()
            literal = system32 / "bash.exe"
            nested = system32 / "Sub" / "bash.exe"
            literal.touch()
            nested.touch()
            linked_system32 = root / "linked-system32"
            try:
                linked_system32.symlink_to(system32, target_is_directory=True)
            except OSError as error:
                self.skipTest(f"symlink support unavailable: {error}")

            linked = linked_system32 / "bash.exe"
            outside = root / "Git" / "bin" / "bash.exe"
            outside.parent.mkdir(parents=True)
            outside.touch()

            self.assertTrue(_is_system32_wsl_stub(str(literal), str(system32)))
            self.assertTrue(_is_system32_wsl_stub(str(nested), str(system32)))
            self.assertTrue(_is_system32_wsl_stub(str(linked), str(system32)))
            self.assertFalse(_is_system32_wsl_stub(str(outside), str(system32)))

    def write_workspace(self, repo_root: Path) -> None:
        (repo_root / ".just").mkdir()
        (repo_root / ".just/lint-config.toml").write_text(
            "[identities]\nforbidden_literals = [\"team-lead\"]\n",
            encoding="utf-8",
        )
        (repo_root / "Cargo.toml").write_text(ROOT_MANIFEST, encoding="utf-8")

        lib_dir = repo_root / "crates/lib-crate"
        lib_dir.mkdir(parents=True)
        (lib_dir / "Cargo.toml").write_text(
            """\
[package]
name = "lib-crate"
version = "0.1.0"

[lib]
name = "lib_crate_custom"
""",
            encoding="utf-8",
        )
        (lib_dir / "src").mkdir()
        (lib_dir / "src/lib.rs").write_text("pub fn lib_fn() {}\n", encoding="utf-8")
        (lib_dir / "tests").mkdir()
        (lib_dir / "tests/lib_tests.rs").write_text("#[test]\nfn ok() {}\n", encoding="utf-8")

        bin_dir = repo_root / "crates/bin-crate"
        bin_dir.mkdir(parents=True)
        (bin_dir / "Cargo.toml").write_text(
            """\
[package]
name = "bin-crate"
version = "0.1.0"
""",
            encoding="utf-8",
        )
        (bin_dir / "src").mkdir()
        (bin_dir / "src/main.rs").write_text("fn main() {}\n", encoding="utf-8")

        excluded_dir = repo_root / "excluded/skip-me"
        excluded_dir.mkdir(parents=True)
        (excluded_dir / "Cargo.toml").write_text(
            """\
[package]
name = "skip-me"
version = "0.1.0"
""",
            encoding="utf-8",
        )

    def test_discover_repo_root_and_load_config_use_explicit_root(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)

            discovered = discover_repo_root(str(repo_root))
            loaded = load_lint_config(discovered)

            self.assertEqual(discovered, repo_root.resolve())
            self.assertEqual(loaded["identities"]["forbidden_literals"], ["team-lead"])

    def test_workspace_manifest_paths_respect_excludes(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)

            manifests = [path.relative_to(repo_root).as_posix() for path in workspace_manifest_paths(repo_root)]

            self.assertEqual(
                manifests,
                [
                    "crates/bin-crate/Cargo.toml",
                    "crates/lib-crate/Cargo.toml",
                ],
            )

    def test_workspace_crates_and_target_args_capture_lib_and_bin_shapes(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)

            crates = workspace_crates(repo_root)
            by_dir = {crate.crate_dir: crate for crate in crates}

            self.assertEqual(by_dir["lib-crate"].crate_path_name, "lib_crate_custom")
            self.assertEqual(by_dir["bin-crate"].crate_path_name, "bin_crate")

            self.assertEqual(
                workspace_target_args(repo_root / "crates/lib-crate/Cargo.toml"),
                ["--lib"],
            )
            self.assertEqual(
                workspace_target_args(repo_root / "crates/bin-crate/Cargo.toml"),
                ["--bin", "bin-crate"],
            )

    def test_render_workspace_crate_table_lists_workspace_inventory(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.write_workspace(repo_root)

            lines = render_workspace_crate_table(repo_root)
            rendered = "\n".join(lines)

            self.assertIn("crate", rendered)
            self.assertIn("package", rendered)
            self.assertIn("lib-crate", rendered)
            self.assertIn("bin-crate", rendered)

    def test_make_log_path_relative_display_and_duration_are_stable(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            started_at = datetime(2026, 7, 7, 18, 45, 0, tzinfo=timezone.utc)

            log_path = make_log_path(repo_root, "Lint Common!", started_at)

            self.assertEqual(log_path.name, "20260707184500-lint-common.log")
            self.assertEqual(relative_log_path(repo_root, log_path), ".just/logs/20260707184500-lint-common.log")
            self.assertEqual(lint_slug("Lint Common!"), "lint-common")
            self.assertEqual(format_duration(0.25), "0.25s")
            self.assertEqual(format_duration(2.0), "2.0s")

    def test_build_report_writes_log_and_print_report_uses_preview_rules(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            started_at = datetime(2026, 7, 7, 18, 45, 0, tzinfo=timezone.utc)

            report = build_report(
                lint_name="example",
                repo_root=repo_root,
                passed=False,
                summary="failed",
                findings=["first issue", "second issue", "third issue", "fourth issue"],
                transcript_lines=["command: demo"],
                started_at=started_at,
                duration_seconds=1.25,
            )

            self.assertTrue(report.log_path.exists())
            self.assertIn("lint: example", report.log_path.read_text(encoding="utf-8"))

            output = StringIO()
            with redirect_stdout(output):
                print_report(report, repo_root=repo_root, preview_limit=2, direct_threshold=3)

            rendered = output.getvalue()
            self.assertIn("example failed", rendered)
            self.assertIn("first issue", rendered)
            self.assertIn("second issue", rendered)
            self.assertIn("[4] errors in .just/logs/20260707184500-example.log", rendered)

    def test_print_report_passed_reports_single_line(self) -> None:
        report = LintReport(
            lint_name="example",
            passed=True,
            summary="ok",
            findings=[],
            transcript=[],
            duration_seconds=0.5,
            log_path=Path("/tmp/example.log"),
        )

        output = StringIO()
        with redirect_stdout(output):
            print_report(report, repo_root=Path("/tmp"))

        self.assertEqual(output.getvalue().strip(), "example passed [0.50s]")

    def test_comment_and_code_helpers_classify_expected_lines(self) -> None:
        self.assertTrue(is_comment_line("// hello"))
        self.assertTrue(is_comment_line("/* block start"))
        self.assertFalse(is_comment_line("let x = 1;"))
        self.assertFalse(is_code_line("   "))
        self.assertTrue(is_code_line("let x = 1;"))

    def test_classify_rust_test_scope_marks_cfg_test_blocks(self) -> None:
        lines = [
            "pub fn production() {",
            "    do_work();",
            "}",
            "#[cfg(test)]",
            "mod tests {",
            "    #[test]",
            "    fn example() {",
            "        assert!(true);",
            "    }",
            "}",
        ]

        scope = classify_rust_test_scope(lines)

        self.assertEqual(scope[:3], [False, False, False])
        self.assertEqual(scope[3:], [True, True, True, True, True, True, True])

    def test_classify_rust_test_scope_marks_cfg_any_test_utils_items(self) -> None:
        lines = [
            "pub fn production() {}",
            '#[cfg(any(test, feature = "test-utils"))]',
            'pub const TEST_ARCH_CTM: &str = "arch-ctm";',
            "pub fn still_production() {}",
        ]

        scope = classify_rust_test_scope(lines)

        self.assertEqual(scope, [False, True, True, False])

    def test_classify_rust_test_scope_marks_inner_cfg_any_test_file(self) -> None:
        lines = [
            '#![cfg(any(test, feature = "test-utils"))]',
            'pub const TEST_ARCH_CTM: &str = "arch-ctm";',
            "pub fn helper() {}",
        ]

        scope = classify_rust_test_scope(lines)

        self.assertEqual(scope, [True, True, True])

    def test_rust_file_test_scope_marks_tests_and_src_paths(self) -> None:
        src_lines = ["pub fn production() {}", "#[cfg(test)]", "mod tests {", "}"]
        tests_lines = ["#[test]", "fn ok() {}"]

        self.assertEqual(
            rust_file_test_scope(Path("crates/lib-crate/src/lib.rs"), src_lines),
            [False, True, True, True],
        )
        self.assertEqual(
            rust_file_test_scope(Path("crates/lib-crate/tests/lib_tests.rs"), tests_lines),
            [True, True],
        )
        self.assertEqual(
            rust_file_test_scope(Path("scripts/tool.py"), ["print('hi')"]),
            [False],
        )

    def test_strip_negated_cfg_segments_removes_negated_test_tokens(self) -> None:
        self.assertEqual(strip_negated_cfg_segments("not(test)"), " ")
        self.assertEqual(
            strip_negated_cfg_segments('any(not(test), feature = "test-utils")'),
            'any( , feature = "test-utils")',
        )
        self.assertEqual(
            strip_negated_cfg_segments('all(unix, not(any(test, feature = "test-utils")))'),
            "all(unix,  )",
        )

    def test_classify_rust_test_scope_ignores_cfg_not_test_after_cfg_test_field(self) -> None:
        lines = [
            "struct TailArgs {",
            "    poll_interval_ms: u64,",
            "    #[cfg(test)]",
            "    max_polls: Option<usize>,",
            "}",
            "",
            "impl TailArgs {",
            "    #[cfg(not(test))]",
            "    fn run(self) {",
            "        thread::sleep(std::time::Duration::from_millis(self.poll_interval_ms));",
            "    }",
            "}",
        ]

        scope = classify_rust_test_scope(lines)

        self.assertEqual(scope[:5], [False, False, True, True, False])
        self.assertEqual(scope[7:], [False, False, False, False, False])

    def test_iter_string_literal_contents_handles_escaped_and_raw_literals(self) -> None:
        line = 'let a = "hello"; let b = r#"raw value"#;'

        self.assertEqual(
            iter_string_literal_contents(line),
            ["hello", "raw value"],
        )

    def test_iter_string_literal_contents_preserves_unicode_while_scanning_escapes(self) -> None:
        line = r'let value = format!("case-{case_index}-🙂-漢字-\\\"-newline\\n");'

        self.assertEqual(
            iter_string_literal_contents(line),
            [r'case-{case_index}-🙂-漢字-\"-newline\n'],
        )


if __name__ == "__main__":
    unittest.main()
