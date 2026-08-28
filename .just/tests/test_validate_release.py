"""ATM-owned consumer-contract tests retained until the first kit-release receipt."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

SPEC = importlib.util.spec_from_file_location(
    "validate_release_module",
    SCRIPTS_DIR / "validate_release.py",
)
assert SPEC is not None
assert SPEC.loader is not None
VALIDATE_RELEASE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = VALIDATE_RELEASE
SPEC.loader.exec_module(VALIDATE_RELEASE)


class ValidateReleaseContractTests(unittest.TestCase):
    """Validate ATM-owned release decisions around the installed kit contract."""

    def setUp(self) -> None:
        self.tempdir = tempfile.TemporaryDirectory()
        self.root = Path(self.tempdir.name)
        (self.root / "release").mkdir(parents=True, exist_ok=True)

        (self.root / "Cargo.toml").write_text(
            textwrap.dedent(
                """
                [workspace]
                members = []
                resolver = "2"

                [workspace.package]
                version = "1.3.0"
                """
            ).strip()
            + "\n",
            encoding="utf-8",
        )
        (self.root / "release" / "publish-artifacts.toml").write_text(
            textwrap.dedent(
                """
                schema_version = 1

                [[crates]]
                artifact = "atm"
                package = "agent-team-mail"
                cargo_toml = "crates/atm/Cargo.toml"
                required = true
                publish = false
                publish_order = 1
                preflight_check = "locked"
                wait_after_publish_seconds = 0
                verify_install = false

                [[release_binaries]]
                name = "atm"
                bundled_paths = [{ source = "docs/user-documents", destination = "share/doc/atm" }]
                """
            ).strip()
            + "\n",
            encoding="utf-8",
        )
    def tearDown(self) -> None:
        self.tempdir.cleanup()

    def test_default_release_targets_exclude_closed_phase_ad_diagnostic(self) -> None:
        self.assertNotIn("phase-ad-readiness", VALIDATE_RELEASE.DEFAULT_RELEASE_TARGETS)
        self.assertEqual(
            VALIDATE_RELEASE.build_parser().parse_args(["phase-ad-readiness"]).target,
            "phase-ad-readiness",
        )

    @mock.patch.object(VALIDATE_RELEASE, "run_capture")
    def test_validate_cli_surface_uses_the_feature_gated_contract(self, run_capture: mock.Mock) -> None:
        run_capture.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout="",
            stderr="",
        )
        findings: list[VALIDATE_RELEASE.Finding] = []

        VALIDATE_RELEASE.validate_cli_surface(self.root, findings)

        self.assertFalse(findings)
        self.assertEqual(
            run_capture.call_args.args[0],
            [
                "cargo",
                "test",
                "-p",
                "agent-team-mail",
                "--features",
                "cli-surface-dump",
                "--test",
                "cli_surface",
            ],
        )

    @mock.patch.object(VALIDATE_RELEASE, "run_capture")
    def test_manifest_validation_uses_installed_kit_contract(self, run_capture: mock.Mock) -> None:
        run_capture.side_effect = [
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.CompletedProcess(
                args=[],
                returncode=0,
                stdout='{"repository_secrets": [], "environment_secrets": [], "github_environments": []}',
                stderr="",
            ),
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
        ]
        findings: list[VALIDATE_RELEASE.Finding] = []

        VALIDATE_RELEASE.validate_manifest(
            self.root,
            findings,
        )

        self.assertFalse(findings)
        commands = [call.args[0] for call in run_capture.call_args_list]
        self.assertTrue(all("scripts/release_artifacts.py" not in command for command in commands))
        self.assertEqual(
            commands[:3],
            [
                [
                    "python3",
                    ".github/scripts/release_artifacts.py",
                    "validate-manifest",
                    "--manifest",
                    "release/publish-artifacts.toml",
                    "--workspace-toml",
                    "Cargo.toml",
                ],
                [
                    "python3",
                    ".github/scripts/release_artifacts.py",
                    "validate-publish-order",
                    "--manifest",
                    "release/publish-artifacts.toml",
                    "--workspace-toml",
                    "Cargo.toml",
                ],
                [
                    "python3",
                    ".github/scripts/release_artifacts.py",
                    "preflight-secret-plan",
                    "--manifest",
                    "release/publish-artifacts.toml",
                ],
            ],
        )

    @mock.patch.object(VALIDATE_RELEASE, "run_capture")
    def test_manifest_validation_does_not_restore_legacy_document_branch(self, run_capture: mock.Mock) -> None:
        manifest_path = self.root / "release" / "publish-artifacts.toml"
        legacy_section = "_".join(("installed", "docs"))
        manifest_path.write_text(
            manifest_path.read_text(encoding="utf-8")
            + f'\n[{legacy_section}]\nsource_root = "docs/user-documents"\n',
            encoding="utf-8",
        )
        run_capture.side_effect = [
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.CompletedProcess(
                args=[],
                returncode=0,
                stdout='{"repository_secrets": [], "environment_secrets": [], "github_environments": []}',
                stderr="",
            ),
        ]

        VALIDATE_RELEASE.validate_manifest(self.root, [])

        self.assertEqual(run_capture.call_count, 3)
        self.assertFalse(
            any("verify_user" + "_docs.py" in command for call in run_capture.call_args_list for command in call.args[0])
        )

    @mock.patch.object(VALIDATE_RELEASE, "run_capture")
    def test_release_binary_validation_fails_closed_when_kit_omits_a_binary(self, run_capture: mock.Mock) -> None:
        run_capture.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="--bin atm\n", stderr=""
        )
        findings: list[VALIDATE_RELEASE.Finding] = []

        VALIDATE_RELEASE.validate_release_binaries(self.root, findings)

        self.assertEqual(run_capture.call_args.args[0][1], ".github/scripts/release_artifacts.py")
        self.assertTrue(any(finding.check == "release-binaries" and finding.blocks for finding in findings))

    @mock.patch.object(VALIDATE_RELEASE, "load_release_contract")
    @mock.patch.object(VALIDATE_RELEASE, "run_capture")
    def test_publish_surface_warns_only_for_unpublished_workspace_resolution(
        self,
        run_capture: mock.Mock,
        load_release_contract: mock.Mock,
    ) -> None:
        load_release_contract.return_value = {
            "crates": [
                {"package": "published-crate", "publish": True},
                {"package": "internal-crate", "publish": False},
            ]
        }
        unpublished = subprocess.CompletedProcess(
            args=["cargo", "publish", "--dry-run"],
            returncode=101,
            stdout="",
            stderr=(
                'failed to select a version for the requirement `atm-error = "^1.3.0"`\n'
                'candidate versions found which didn\'t match: 1.2.9\n'
                'location searched: crates.io index\n'
            ),
        )
        run_capture.side_effect = [
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            unpublished,
            unpublished,
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
        ]
        findings: list[VALIDATE_RELEASE.Finding] = []

        with mock.patch.object(VALIDATE_RELEASE, "workspace_package_names", return_value={"atm-error"}):
            VALIDATE_RELEASE.validate_publish_surface(
                self.root,
                "1.3.0",
                findings,
                enforce_release_version=False,
            )

        commands = [call.args[0] for call in run_capture.call_args_list]
        self.assertIn(["cargo", "package", "-p", "published-crate", "--locked", "--no-verify"], commands)
        self.assertIn(["cargo", "publish", "--dry-run", "-p", "published-crate", "--locked", "--no-verify"], commands)
        self.assertIn(["cargo", "check", "-p", "internal-crate", "--locked"], commands)
        self.assertFalse(any(finding.blocks for finding in findings))
        self.assertEqual(
            [finding.severity for finding in findings if finding.check.startswith("cargo-")],
            ["warning", "warning"],
        )

    @mock.patch.object(VALIDATE_RELEASE, "load_release_contract")
    @mock.patch.object(VALIDATE_RELEASE, "run_capture")
    def test_publish_surface_keeps_other_dry_run_failures_blocking(
        self,
        run_capture: mock.Mock,
        load_release_contract: mock.Mock,
    ) -> None:
        load_release_contract.return_value = {"crates": [{"package": "published-crate", "publish": True}]}
        run_capture.side_effect = [
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
            subprocess.CompletedProcess(
                args=["cargo", "package"],
                returncode=101,
                stdout="",
                stderr=(
                    'failed to select a version for the requirement `third-party = "^1.3.0"`\n'
                    'candidate versions found which didn\'t match: 1.2.9\n'
                    'location searched: crates.io index\n'
                ),
            ),
            subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr=""),
        ]
        findings: list[VALIDATE_RELEASE.Finding] = []

        VALIDATE_RELEASE.validate_publish_surface(
            self.root,
            "1.3.0",
            findings,
            enforce_release_version=True,
        )

        commands = [call.args[0] for call in run_capture.call_args_list]
        self.assertIn(["cargo", "package", "-p", "published-crate", "--locked", "--no-verify"], commands)
        self.assertIn(["cargo", "publish", "--dry-run", "-p", "published-crate", "--locked", "--no-verify"], commands)
        self.assertTrue(any(finding.check == "cargo-package-published-crate" and finding.blocks for finding in findings))


if __name__ == "__main__":
    unittest.main()
