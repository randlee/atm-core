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

    def test_sc_compose_uses_the_repository_authoritative_revision(self) -> None:
        manifest = bootstrap.load_manifest()
        command = bootstrap.sc_compose_install_command(manifest.sc_compose_rev, force=True)
        self.assertIn(manifest.sc_compose_rev, command)
        self.assertIn("--locked", command)

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
            self.assertFalse(bootstrap.registry_tool_matches("cargo-shear", "1.12.0", "1.94.1"))

    def test_manifest_uses_current_compatible_stable_releases(self) -> None:
        manifest = bootstrap.load_manifest()
        self.assertEqual(manifest.python, "3.14.7")
        self.assertEqual(manifest.just, "1.58.0")
        self.assertEqual(dict(manifest.cargo_tools), {
            "cargo-deny": "0.20.2",
            "cargo-audit": "0.22.2",
            "cargo-shear": "1.12.0",
            "cargo-modules": "0.26.0",
        })
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

    def test_sc_compose_receipt_requires_the_exact_git_revision(self) -> None:
        manifest = bootstrap.load_manifest()
        expected_key = (
            "sc-compose 1.4.0 "
            f"(git+{bootstrap.SC_COMPOSE_REPOSITORY}?rev={manifest.sc_compose_rev}#{manifest.sc_compose_rev})"
        )
        with mock.patch.object(bootstrap, "cargo_receipts", return_value={expected_key: {"rustc": "release: 1.94.1"}}):
            bootstrap.verify_sc_compose_receipt(manifest.sc_compose_rev)

    def test_sc_compose_receipt_rejects_a_different_revision(self) -> None:
        with mock.patch.object(bootstrap, "cargo_receipts", return_value={"sc-compose 1.4.0": {"rustc": "release: 1.94.1"}}):
            with self.assertRaisesRegex(bootstrap.BootstrapError, "exact source revision"):
                bootstrap.verify_sc_compose_receipt("expected-revision")

    def test_ci_uses_the_shared_bootstrap_recipe(self) -> None:
        workflow = (SCRIPT.parents[1] / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        self.assertIn('python-version: "3.14.7"', workflow)
        self.assertIn("tool: just@1.58.0", workflow)
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
        self.assertEqual(workflow.count("tools/bootstrap-requirements.txt"), 3)

    def test_seed_python_preserves_ci_python_precedence(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn('PATH=\\"$PATH:/opt/homebrew/bin\\" python3.14', justfile)
        self.assertNotIn("PATH=/opt/homebrew/bin:$PATH python3.14", justfile)


if __name__ == "__main__":
    unittest.main()
