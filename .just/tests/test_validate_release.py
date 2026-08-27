from __future__ import annotations

import importlib.util
import shutil
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


class ValidateReleaseProofTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tempdir = tempfile.TemporaryDirectory()
        self.root = Path(self.tempdir.name)
        (self.root / "release").mkdir(parents=True, exist_ok=True)
        (self.root / "scripts").mkdir(parents=True, exist_ok=True)
        (self.root / "docs" / "user-documents").mkdir(parents=True, exist_ok=True)
        (self.root / "target").mkdir(parents=True, exist_ok=True)
        shutil.copy2(REPO_ROOT / "scripts" / "verify_user_docs.py", self.root / "scripts" / "verify_user_docs.py")

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

                [installed_docs]
                source_root = "docs/user-documents"
                install_root = "share/doc/atm"
                entrypoint = "share/doc/atm/README.md"
                """
            ).strip()
            + "\n",
            encoding="utf-8",
        )
        (self.root / "docs" / "user-documents" / "README.md").write_text(
            "---\nreviewed_for_release: 1.3.0\n---\n# ATM Docs\n",
            encoding="utf-8",
        )
        (self.root / "docs" / "user-documents" / "hooks.md").write_text(
            "---\nreviewed_for_release: 1.3.0\n---\n# Hooks\n",
            encoding="utf-8",
        )
        (self.root / "release" / "release-notes.md").write_text(
            "Installed docs live under share/doc/atm/ with share/doc/atm/README.md as the entrypoint.\n",
            encoding="utf-8",
        )

    def tearDown(self) -> None:
        self.tempdir.cleanup()

    def test_ensure_staged_install_docs_copies_user_doc_tree(self) -> None:
        staged_root = VALIDATE_RELEASE.ensure_staged_install_docs(
            self.root,
            manifest_path=self.root / "release" / "publish-artifacts.toml",
            staged_install_root=None,
        )

        self.assertTrue((staged_root / "share/doc/atm/README.md").is_file())
        self.assertTrue((staged_root / "share/doc/atm/hooks.md").is_file())

    def test_write_phase_ae_installed_docs_proof_records_required_fields(self) -> None:
        proof_path = self.root / "reports" / "smoke" / "phase-AE-installed-docs-proof.md"
        findings: list[object] = []

        VALIDATE_RELEASE.write_phase_ae_installed_docs_proof(
            self.root,
            version="1.3.0",
            proof_output=proof_path,
            staged_install_root=None,
            findings=findings,
        )

        text = proof_path.read_text(encoding="utf-8")
        self.assertIn("reviewed release version: 1.3.0", text)
        self.assertIn("share/doc/atm/README.md", text)
        self.assertIn("release/release-notes.md", text)
        self.assertIn("docs/user-documents/README.md", text)
        self.assertIn("- status: `passed`", text)
        self.assertIn("- installed-doc verifier: `passed`", text)
        self.assertFalse(findings)

    def test_write_phase_ae_installed_docs_proof_ignores_non_doc_blockers_in_status(self) -> None:
        proof_path = self.root / "reports" / "smoke" / "phase-AE-installed-docs-proof.md"
        findings = [
            VALIDATE_RELEASE.Finding(
                check="publish-version-unpublished",
                severity="error",
                summary="release version already published",
            )
        ]

        VALIDATE_RELEASE.write_phase_ae_installed_docs_proof(
            self.root,
            version="1.3.0",
            proof_output=proof_path,
            staged_install_root=None,
            findings=findings,
        )

        text = proof_path.read_text(encoding="utf-8")
        self.assertIn("- status: `passed`", text)
        self.assertIn("- installed-doc verifier: `passed`", text)

    def test_validate_staged_install_docs_fails_closed_on_stale_reviewed_release(self) -> None:
        staged_root = self.root / "target" / "phase-ae" / "staged-install-root"
        VALIDATE_RELEASE.ensure_staged_install_docs(
            self.root,
            manifest_path=self.root / "release" / "publish-artifacts.toml",
            staged_install_root=staged_root,
        )
        (self.root / "docs" / "user-documents" / "README.md").write_text(
            "---\nreviewed_for_release: 0.0.0\n---\n# ATM Docs\n",
            encoding="utf-8",
        )
        findings: list[VALIDATE_RELEASE.Finding] = []

        VALIDATE_RELEASE.validate_staged_install_docs(
            self.root,
            findings,
            manifest_path=self.root / "release" / "publish-artifacts.toml",
            staged_install_root=staged_root,
            release_version="1.3.0",
        )

        self.assertTrue(
            any(
                finding.check == "installed-docs-verifier"
                and finding.blocks
                and "reviewed_for_release is 0.0.0" in finding.detail
                for finding in findings
            )
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
            staged_install_root=None,
            release_version="1.3.0",
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
    def test_default_publish_surface_skips_candidate_only_package_checks(
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
        run_capture.return_value = subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr="")
        findings: list[VALIDATE_RELEASE.Finding] = []

        VALIDATE_RELEASE.validate_publish_surface(
            self.root,
            "1.3.0",
            findings,
            enforce_release_version=False,
        )

        commands = [call.args[0] for call in run_capture.call_args_list]
        self.assertNotIn(["cargo", "package", "-p", "published-crate", "--locked", "--no-verify"], commands)
        self.assertNotIn(["cargo", "publish", "--dry-run", "-p", "published-crate", "--locked", "--no-verify"], commands)
        self.assertIn(["cargo", "check", "-p", "internal-crate", "--locked"], commands)
        self.assertTrue(any(finding.check == "publishable-crate-dry-runs" for finding in findings))

    @mock.patch.object(VALIDATE_RELEASE, "load_release_contract")
    @mock.patch.object(VALIDATE_RELEASE, "run_capture")
    def test_release_candidate_publish_surface_keeps_package_checks(
        self,
        run_capture: mock.Mock,
        load_release_contract: mock.Mock,
    ) -> None:
        load_release_contract.return_value = {"crates": [{"package": "published-crate", "publish": True}]}
        run_capture.return_value = subprocess.CompletedProcess(args=[], returncode=0, stdout="", stderr="")

        VALIDATE_RELEASE.validate_publish_surface(
            self.root,
            "1.3.0",
            [],
            enforce_release_version=True,
        )

        commands = [call.args[0] for call in run_capture.call_args_list]
        self.assertIn(["cargo", "package", "-p", "published-crate", "--locked", "--no-verify"], commands)
        self.assertIn(["cargo", "publish", "--dry-run", "-p", "published-crate", "--locked", "--no-verify"], commands)


if __name__ == "__main__":
    unittest.main()
