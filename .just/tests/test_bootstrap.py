from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[2] / "tools" / "bootstrap.py"
SPEC = importlib.util.spec_from_file_location("bootstrap", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
bootstrap = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = bootstrap
SPEC.loader.exec_module(bootstrap)


class BootstrapTests(unittest.TestCase):
    def test_manifest_has_only_exact_tool_versions(self) -> None:
        manifest = bootstrap.load_manifest()
        versions = [manifest.rust, manifest.python, manifest.just]
        versions.extend(version for _, version in manifest.cargo_tools)
        versions.extend(version for _, version in manifest.python_packages)
        self.assertTrue(all("*" not in version and ">" not in version and "<" not in version for version in versions))

    def test_sc_compose_uses_the_exact_prebuilt_release_asset(self) -> None:
        manifest = bootstrap.load_manifest()
        asset, url = bootstrap.sc_compose_install_command(manifest.sc_compose, "aarch64-apple-darwin")
        self.assertEqual(asset, "sc-compose_1.5.0_aarch64-apple-darwin.tar.gz")
        self.assertEqual(url, "https://github.com/randlee/sc-compose/releases/download/v1.5.0/" + asset)
        self.assertEqual(dict(manifest.sc_compose_checksums)["aarch64-apple-darwin"], "7751631cd86e6644e88cfcf3dd80f352779350f9f24f891f52983c8da0ed4620")

    def test_sc_compose_install_never_uses_cargo(self) -> None:
        source = (SCRIPT.parents[1] / "tools" / "bootstrap.py").read_text(encoding="utf-8")
        self.assertNotIn('cargo", "install", "--locked", "--version", version, "sc-compose"', source)

    def test_wyvern_uses_the_pinned_release_asset(self) -> None:
        manifest = bootstrap.load_manifest()
        asset, url = bootstrap.wyvern_install_command(manifest.wyvern, "aarch64-apple-darwin")
        self.assertEqual(asset, "wyvern-macos-aarch64.tar.gz")
        self.assertEqual(
            url,
            "https://github.com/randlee/wyvern/releases/download/v0.5.0/wyvern-macos-aarch64.tar.gz",
        )

    def test_sc_compose_checksum_mismatch_is_a_hard_failure(self) -> None:
        manifest = bootstrap.load_manifest()
        with (
            mock.patch.object(bootstrap, "sc_compose_target", return_value="aarch64-apple-darwin"),
            mock.patch.object(bootstrap, "_download_release", return_value=b"tampered"),
        ):
            with self.assertRaisesRegex(bootstrap.BootstrapError, "checksum mismatch"):
                bootstrap.install_sc_compose_release(manifest, dry_run=False)

    def test_binstall_uses_prebuilt_only_and_exact_version(self) -> None:
        command = bootstrap.cargo_binstall_command("cargo-audit", "0.22.2", force=True)
        self.assertEqual(command, [
            "cargo", "binstall", "--no-confirm", "--disable-telemetry",
            "--disable-strategies", "quick-install,compile", "--force", "cargo-audit@0.22.2",
        ])

    def test_cargo_modules_uses_quick_install_without_compile(self) -> None:
        command = bootstrap.cargo_binstall_command(
            "cargo-modules", "0.26.0", force=True, allowed_strategies=("quick-install",)
        )
        self.assertEqual(command, [
            "cargo", "binstall", "--no-confirm", "--disable-telemetry",
            "--disable-strategies", "compile", "--force", "cargo-modules@0.26.0",
        ])

    def test_registry_tools_are_exact_and_locked(self) -> None:
        command = bootstrap.cargo_install_command("cargo-audit", "0.22.2", force=True)
        self.assertEqual(command, ["cargo", "install", "--locked", "--force", "--version", "0.22.2", "cargo-audit"])

    def test_matching_registry_receipt_avoids_a_rebuild(self) -> None:
        receipt = {"cargo-audit 0.22.2 (registry+https://example.test/index)": {"rustc": "release: 1.94.1"}}
        with mock.patch.object(bootstrap, "cargo_receipts", return_value=receipt):
            self.assertTrue(bootstrap.registry_tool_matches("cargo-audit", "0.22.2", "1.94.1"))

    def test_registry_receipt_rejects_an_unpinned_release(self) -> None:
        receipt = {"cargo-shear 1.13.2 (registry+https://example.test/index)": {"rustc": "release: 1.94.1"}}
        with mock.patch.object(bootstrap, "cargo_receipts", return_value=receipt):
            self.assertFalse(bootstrap.registry_tool_matches("cargo-shear", "1.13.3", "1.94.1"))

    def test_manifest_uses_current_compatible_stable_releases(self) -> None:
        manifest = bootstrap.load_manifest()
        self.assertEqual(manifest.python, "3.14.7")
        self.assertEqual(manifest.just, "1.58.0")
        self.assertEqual(dict(manifest.cargo_tools), {
            "cargo-deny": "0.20.2",
            "cargo-audit": "0.22.2",
            "cargo-shear": "1.13.3",
            "cargo-modules": "0.26.0",
        })
        self.assertEqual(dict(manifest.cargo_allowed_strategies), {
            "cargo-deny": (),
            "cargo-audit": ("quick-install",),
            "cargo-shear": (),
            "cargo-modules": ("quick-install",),
        })
        self.assertEqual(manifest.sc_compose, "1.5.0")
        self.assertEqual(manifest.wyvern, "0.5.0")
        self.assertEqual(dict(manifest.python_packages)["maturin"], "1.14.1")

    def test_macos_homebrew_seed_formula_is_derived_from_the_exact_python_pin(self) -> None:
        manifest = bootstrap.load_manifest()
        self.assertEqual(bootstrap.homebrew_python_formula(manifest), "python@3.14")

    @unittest.skipUnless(sys.platform == "darwin", "Homebrew seed paths are macOS-specific")
    def test_macos_homebrew_seed_commands_update_only_declared_seed_packages(self) -> None:
        manifest = bootstrap.load_manifest()
        commands = bootstrap.homebrew_seed_commands(manifest, Path("/opt/homebrew/bin/brew"))
        self.assertEqual(commands, (
            ("/opt/homebrew/bin/brew", "install", "python@3.14", "just"),
            ("/opt/homebrew/bin/brew", "upgrade", "python@3.14", "just"),
        ))

    def test_dry_run_never_executes_installers(self) -> None:
        manifest = bootstrap.load_manifest()
        with (
            mock.patch.object(bootstrap, "synchronize_macos_seed_tools"),
            mock.patch.object(bootstrap, "verify_seed_tools"),
            mock.patch.object(bootstrap, "ensure_bootstrap_venv", return_value=Path("/tmp/bootstrap-python")),
            mock.patch.object(bootstrap, "verify_installed_tools") as verify,
            mock.patch.object(bootstrap, "registry_tool_matches", return_value=False),
            mock.patch.object(bootstrap, "sc_compose_matches", return_value=False),
            mock.patch.object(bootstrap, "wyvern_matches", return_value=False),
            mock.patch.object(bootstrap.subprocess, "run") as run,
        ):
            bootstrap.bootstrap(manifest, dry_run=True)
        run.assert_not_called()
        verify.assert_not_called()

    def test_pip_installs_inside_the_repository_venv(self) -> None:
        python = (
            Path(r"C:\repo\.bootstrap-venv\Scripts\python.exe")
            if sys.platform == "win32"
            else Path("/repo/.bootstrap-venv/bin/python")
        )
        command = bootstrap.pip_install_command(python)
        self.assertEqual(Path(command[0]), python)
        self.assertIn("--no-deps", command)

    def test_seed_version_mismatch_refuses_before_any_installs(self) -> None:
        manifest = bootstrap.load_manifest()
        with mock.patch.object(bootstrap, "platform") as platform_module:
            platform_module.python_version.return_value = "3.11.9"
            with self.assertRaisesRegex(bootstrap.BootstrapError, "Python must be exactly"):
                bootstrap.verify_seed_tools(manifest)

    def test_binstall_receipt_proves_exact_prebuilt_version(self) -> None:
        # Binstall's crates-v1.json flattens CrateInfo fields into each record.
        receipt = [{"name": "cargo-audit", "current_version": "0.22.2"}]
        with mock.patch.object(bootstrap, "binstall_receipts", return_value=receipt):
            self.assertTrue(bootstrap.binstall_tool_matches("cargo-audit", "0.22.2"))
            self.assertFalse(bootstrap.binstall_tool_matches("cargo-audit", "0.22.3"))

    def test_ci_rejects_registry_compile_fallback(self) -> None:
        manifest = bootstrap.load_manifest()
        calls: list[tuple[list[str], bool]] = []

        def failed_binstall(command: list[str], *, dry_run: bool, allow_failure: bool = False) -> bool:
            calls.append((command, allow_failure))
            return False

        with (
            mock.patch.dict("os.environ", {"CI": "true"}),
            mock.patch.object(bootstrap, "synchronize_macos_seed_tools"),
            mock.patch.object(bootstrap, "verify_seed_tools"),
            mock.patch.object(bootstrap, "ensure_bootstrap_venv", return_value=Path("/tmp/bootstrap-python")),
            mock.patch.object(bootstrap, "registry_tool_matches", return_value=False),
            mock.patch.object(bootstrap, "binstall_tool_matches", return_value=False),
            mock.patch.object(bootstrap, "cargo_binstall_available", return_value=True),
            mock.patch.object(bootstrap, "run", side_effect=failed_binstall),
        ):
            with self.assertRaisesRegex(bootstrap.BootstrapError, "could not install the exact prebuilt cargo-deny"):
                bootstrap.bootstrap(manifest, dry_run=False)
        self.assertEqual(len(calls), 1)
        self.assertFalse(calls[0][1])

    def test_local_bootstrap_keeps_registry_fallback(self) -> None:
        manifest = bootstrap.load_manifest()
        calls: list[tuple[list[str], bool]] = []

        def failed_binstall(command: list[str], *, dry_run: bool, allow_failure: bool = False) -> bool:
            calls.append((command, allow_failure))
            return False

        with (
            mock.patch.dict("os.environ", {"CI": ""}),
            mock.patch.object(bootstrap, "synchronize_macos_seed_tools"),
            mock.patch.object(bootstrap, "verify_seed_tools"),
            mock.patch.object(bootstrap, "ensure_bootstrap_venv", return_value=Path("/tmp/bootstrap-python")),
            mock.patch.object(bootstrap, "registry_tool_matches", return_value=False),
            mock.patch.object(bootstrap, "binstall_tool_matches", return_value=False),
            mock.patch.object(bootstrap, "cargo_binstall_available", return_value=True),
            mock.patch.object(bootstrap, "sc_compose_matches", return_value=True),
            mock.patch.object(bootstrap, "wyvern_matches", return_value=True),
            mock.patch.object(bootstrap, "verify_installed_tools"),
            mock.patch.object(bootstrap, "run", side_effect=failed_binstall),
        ):
            bootstrap.bootstrap(manifest, dry_run=False)
        self.assertEqual(calls[0][0][1], "binstall")
        self.assertTrue(calls[0][1])
        self.assertEqual(calls[1][0][:2], ["cargo", "install"])

    def test_ci_uses_the_shared_bootstrap_recipe(self) -> None:
        workflow = (SCRIPT.parents[1] / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        self.assertIn('python-version: "3.14.7"', workflow)
        self.assertIn("tool: just@1.58.0", workflow)
        self.assertIn("cargo-bins/cargo-binstall@75b4bfae1b2c753a6806bbce6e6cb89b602de33c", workflow)
        self.assertGreaterEqual(workflow.count("run: just bootstrap"), 2)

    def test_just_recipes_propagate_the_bootstrap_python_to_children(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn("$PWD/.bootstrap-venv/bin:$PATH", justfile)
        self.assertIn("PYO3_PYTHON", justfile)
        self.assertIn(
            "test-admission-capacity:\n"
            "    {{python_cmd}} -m unittest scripts/smoke/test_run_admission_capacity.py",
            justfile,
        )

    def test_ci_uses_bootstrap_python_and_requirements_for_all_pydantic_consumers(self) -> None:
        workflow = (SCRIPT.parents[1] / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        self.assertIn("run: just test-admission-capacity", workflow)
        self.assertNotIn("run: python -m unittest scripts/smoke/test_run_admission_capacity.py", workflow)
        self.assertNotIn("pydantic>=2,<3", workflow)
        self.assertGreaterEqual(workflow.count("tools/bootstrap-requirements.txt"), 3)

    def test_seed_python_preserves_ci_python_precedence(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn('PATH=\\"$PATH:/opt/homebrew/bin\\" python3.14', justfile)
        self.assertNotIn("PATH=/opt/homebrew/bin:$PATH python3.14", justfile)


if __name__ == "__main__":
    unittest.main()
