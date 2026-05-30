from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_cargo_deny import build_command as build_cargo_deny_command
from lint_cargo_deny import build_runtime_config
from lint_cargo_shear import annotate_sections
from lint_cargo_shear import build_command as build_cargo_shear_command
from lint_cargo_shear import evaluate_policy
from lint_cargo_shear import load_policy_config
from lint_cargo_shear import parse_sections
from lint_codespell import build_command as build_codespell_command


class ExternalLintToolTests(unittest.TestCase):
    def test_build_cargo_deny_command_targets_workspace_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            config_path = repo_root / "deny.runtime.toml"
            self.assertEqual(
                build_cargo_deny_command(repo_root, config_path),
                [
                    "cargo-deny",
                    "check",
                    "--config",
                    str(config_path),
                    "advisories",
                    "bans",
                    "licenses",
                    "sources",
                ],
            )

    def test_build_runtime_config_strips_deprecated_keys(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "deny.toml").write_text(
                """\
[advisories]
vulnerability = "deny"
yanked = "deny"

[licenses]
unlicensed = "deny"
allow = ["MIT"]
""",
                encoding="utf-8",
            )

            runtime_path = build_runtime_config(repo_root)
            text = runtime_path.read_text(encoding="utf-8")

            self.assertNotIn('vulnerability = "deny"', text)
            self.assertNotIn('unlicensed = "deny"', text)
            self.assertIn('yanked = "deny"', text)
            self.assertIn('allow = ["MIT"]', text)

    def test_build_cargo_shear_command_targets_workspace_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            self.assertEqual(
                build_cargo_shear_command(repo_root),
                ["cargo-shear"],
            )

    def test_parse_sections_extracts_warning_files(self) -> None:
        stdout = """\
shear/unlinked_files

  ⚠ 1 unlinked file in `agent-team-mail`
  │ tests/support/mod.rs
  help: delete this file

shear/empty_files

  ⚠ 2 empty files in `agent-team-mail-core`
  │ src/model_registry.rs
  │ src/schema/settings.rs
"""
        sections = parse_sections(stdout)
        self.assertEqual([section.name for section in sections], ["unlinked_files", "empty_files"])
        self.assertEqual(sections[0].file_paths, ("tests/support/mod.rs",))
        self.assertEqual(
            sections[1].file_paths,
            ("src/model_registry.rs", "src/schema/settings.rs"),
        )

    def test_evaluate_policy_promotes_unapproved_warning_files_to_errors(self) -> None:
        stdout = """\
shear/unlinked_files

  ⚠ 1 unlinked file in `agent-team-mail`
  │ tests/support/mod.rs
"""
        sections = parse_sections(stdout)
        findings, downgraded = evaluate_policy(
            sections,
            {"allowed_empty_files": {}, "allowed_unlinked_files": {}},
        )
        self.assertEqual(downgraded, [])
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].section_name, "unlinked_files")
        self.assertEqual(findings[0].file_path, "tests/support/mod.rs")

    def test_evaluate_policy_downgrades_allowlisted_files(self) -> None:
        stdout = """\
shear/empty_files

  ⚠ 1 empty file in `agent-team-mail-core`
  │ src/model_registry.rs
"""
        sections = parse_sections(stdout)
        findings, downgraded = evaluate_policy(
            sections,
            {
                "allowed_empty_files": {"src/model_registry.rs": "planned stub"},
                "allowed_unlinked_files": {},
            },
        )
        self.assertEqual(findings, [])
        self.assertEqual(
            downgraded,
            ["empty_files: downgraded src/model_registry.rs (planned stub)"],
        )

    def test_load_policy_config_normalizes_windows_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            just_dir = repo_root / ".just"
            just_dir.mkdir()
            (just_dir / "lint-config.toml").write_text(
                """\
[cargo_shear.allowed_empty_files]
"src\\\\model_registry.rs" = "planned stub"

[cargo_shear.allowed_unlinked_files]
"tests\\\\support\\\\mod.rs" = "legacy pending"
""",
                encoding="utf-8",
            )

            policy = load_policy_config(repo_root)
            self.assertEqual(
                policy["allowed_empty_files"]["src/model_registry.rs"],
                "planned stub",
            )
            self.assertEqual(
                policy["allowed_unlinked_files"]["tests/support/mod.rs"],
                "legacy pending",
            )

    def test_annotate_sections_uses_crate_paths(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Cargo.toml").write_text(
                """\
[workspace]
members = ["crates/atm", "crates/atm-core"]
resolver = "2"
""",
                encoding="utf-8",
            )
            for crate_name, package_name in (
                ("atm", "agent-team-mail"),
                ("atm-core", "agent-team-mail-core"),
            ):
                crate_dir = repo_root / "crates" / crate_name
                crate_dir.mkdir(parents=True)
                (crate_dir / "Cargo.toml").write_text(
                    f"""\
[package]
name = "{package_name}"
version = "1.1.2"
""",
                    encoding="utf-8",
                )

            sections = parse_sections(
                """\
shear/unlinked_files

  ⚠ 1 unlinked file in `agent-team-mail`
  │ tests/support/mod.rs
"""
            )
            self.assertEqual(
                annotate_sections(sections, repo_root),
                ["shear note: crates/atm/tests/support/mod.rs [unlinked_files]"],
            )

    def test_build_codespell_command_uses_repo_config(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            command = build_codespell_command(repo_root)
            self.assertEqual(command[:2], [sys.executable, "-c"])
            self.assertIn("codespell_lib", command[2])
            self.assertEqual(len(command), 3)


if __name__ == "__main__":
    unittest.main()
