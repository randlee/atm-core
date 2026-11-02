from __future__ import annotations

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[2] / "tools" / "bootstrap.py"
SPEC = importlib.util.spec_from_file_location("bootstrap", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
bootstrap = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = bootstrap
SPEC.loader.exec_module(bootstrap)


class IsolatedHomeTests(unittest.TestCase):
    def setUp(self) -> None:
        prepatch_home = tempfile.TemporaryDirectory()
        prepatch_cargo_home = tempfile.TemporaryDirectory()
        self._prepatch_cargo_home = Path(prepatch_cargo_home.name)
        self._prepatch_environment = mock.patch.dict(
            os.environ,
            {"HOME": prepatch_home.name, "CARGO_HOME": prepatch_cargo_home.name},
        )
        self._prepatch_environment.start()
        self.addCleanup(self._prepatch_environment.stop)
        self.addCleanup(prepatch_cargo_home.cleanup)
        self.addCleanup(prepatch_home.cleanup)
        self._test_home = tempfile.TemporaryDirectory()
        self._test_cargo_home = tempfile.TemporaryDirectory()
        self._test_venv = tempfile.TemporaryDirectory()
        self._test_python = Path(self._test_venv.name) / "bin" / "python"
        self._environment = mock.patch.dict(
            os.environ,
            {"HOME": self._test_home.name, "CARGO_HOME": self._test_cargo_home.name},
        )
        self._environment.start()
        self.addCleanup(self._environment.stop)
        self.addCleanup(self._test_cargo_home.cleanup)
        self.addCleanup(self._test_venv.cleanup)
        self.addCleanup(self._test_home.cleanup)


class BootstrapTests(IsolatedHomeTests):
    def test_manifest_has_only_exact_tool_versions(self) -> None:
        manifest = bootstrap.load_manifest()
        versions = [manifest.rust, manifest.python, manifest.windows_python, manifest.just]
        versions.extend(version for _, version in manifest.cargo_tools)
        versions.extend(version for _, version in manifest.python_packages)
        self.assertTrue(all("*" not in version and ">" not in version and "<" not in version for version in versions))

    def test_sc_compose_uses_the_exact_prebuilt_release_asset(self) -> None:
        manifest = bootstrap.load_manifest()
        asset, url = bootstrap.sc_compose_install_command(manifest.sc_compose, "aarch64-apple-darwin")
        self.assertEqual(asset, "sc-compose_1.7.0_aarch64-apple-darwin.tar.gz")
        self.assertEqual(url, "https://github.com/randlee/sc-compose/releases/download/v1.7.0/" + asset)
        self.assertEqual(dict(manifest.sc_compose_checksums)["aarch64-apple-darwin"], "d7590a41921bef5a428bcbd33dda48006c6f6df5eafa83217cb820240fe6544b")
        self.assertEqual(
            dict(manifest.sc_compose_checksums)["x86_64-pc-windows-msvc"],
            "80027b651ef86f2abe9ca88a16635fd53994fc0e9ec3c6c14d56ab151cbb3111",
        )

    def test_sc_compose_install_never_uses_cargo(self) -> None:
        source = (SCRIPT.parents[1] / "tools" / "bootstrap.py").read_text(encoding="utf-8")
        self.assertNotIn('cargo", "install", "--locked", "--version", version, "sc-compose"', source)

    def test_cargo_binstall_uses_pinned_release_assets(self) -> None:
        manifest = bootstrap.load_manifest()
        self.assertEqual(manifest.cargo_binstall, "1.22.0")
        asset, url = bootstrap.cargo_binstall_install_command(
            manifest.cargo_binstall, "x86_64-unknown-linux-gnu"
        )
        self.assertEqual(asset, "cargo-binstall-x86_64-unknown-linux-gnu.tgz")
        self.assertEqual(
            url,
            "https://github.com/cargo-bins/cargo-binstall/releases/download/v1.22.0/" + asset,
        )
        self.assertEqual(
            dict(manifest.cargo_binstall_checksums)["aarch64-apple-darwin"],
            "c364fd10e494b4fc0cc88a89995446364379a914930f0b0182b1151ad27690a1",
        )

    def test_cargo_binstall_windows_asset_is_zip_with_exe(self) -> None:
        asset = bootstrap.cargo_binstall_asset_name("1.22.0", "x86_64-pc-windows-msvc")
        self.assertEqual(asset, "cargo-binstall-x86_64-pc-windows-msvc.zip")
        archive_buffer = io.BytesIO()
        with bootstrap.zipfile.ZipFile(archive_buffer, "w") as package:
            package.writestr("cargo-binstall.exe", b"windows binary")
        with tempfile.TemporaryDirectory() as temp_dir:
            destination = Path(temp_dir) / "cargo-binstall.exe"
            bootstrap._extract_cargo_binstall(archive_buffer.getvalue(), asset, destination)
            self.assertEqual(destination.read_bytes(), b"windows binary")

    def test_cargo_binstall_checksum_mismatch_is_a_hard_failure(self) -> None:
        manifest = bootstrap.load_manifest()
        with (
            mock.patch.object(bootstrap, "sc_compose_target", return_value="aarch64-apple-darwin"),
            mock.patch.object(bootstrap, "_download_release", return_value=b"tampered"),
        ):
            with self.assertRaisesRegex(bootstrap.BootstrapError, "cargo-binstall release checksum mismatch"):
                bootstrap.install_cargo_binstall_release(manifest, dry_run=False)

    def test_cargo_binstall_verified_archive_warns_about_pinned_hash_only(self) -> None:
        manifest = bootstrap.load_manifest()
        archive = b"verified archive"
        with (
            mock.patch.object(bootstrap, "sc_compose_target", return_value="aarch64-apple-darwin"),
            mock.patch.object(
                bootstrap,
                "_cargo_binstall_release_checksum",
                return_value=hashlib.sha256(archive).hexdigest(),
            ),
            mock.patch.object(bootstrap, "_download_release", return_value=archive),
            mock.patch.object(bootstrap, "_extract_cargo_binstall") as extract,
        ):
            stderr = StringIO()
            with redirect_stderr(stderr):
                bootstrap.install_cargo_binstall_release(manifest, dry_run=False)
        self.assertIn("verified cargo-binstall against pinned SHA256 only", stderr.getvalue())
        extract.assert_called_once_with(
            archive,
            "cargo-binstall-aarch64-apple-darwin.zip",
            bootstrap.cargo_bin_path("cargo-binstall"),
        )

    def test_wyvern_uses_the_pinned_release_asset(self) -> None:
        manifest = bootstrap.load_manifest()
        asset, url = bootstrap.wyvern_install_command(manifest.wyvern, "aarch64-apple-darwin")
        self.assertEqual(asset, "wyvern_0.6.0_aarch64-apple-darwin.tar.gz")
        self.assertEqual(
            url,
            "https://github.com/randlee/wyvern/releases/download/v0.6.0/wyvern_0.6.0_aarch64-apple-darwin.tar.gz",
        )
        self.assertEqual(
            dict(manifest.wyvern_checksums)[asset],
            "b5f5b986868d65b37d39966d7e9fa0c2bb6fd35fd0675397cbe3b4f77dc6b9dc",
        )

    def test_wyvern_checksum_mismatch_is_a_hard_failure(self) -> None:
        manifest = bootstrap.load_manifest()
        with (
            mock.patch.object(bootstrap, "sc_compose_target", return_value="aarch64-apple-darwin"),
            mock.patch.object(bootstrap, "_download_release", return_value=b"tampered"),
        ):
            with self.assertRaisesRegex(bootstrap.BootstrapError, "Wyvern release checksum mismatch"):
                bootstrap.install_wyvern_release(manifest, dry_run=False)

    def test_wyvern_missing_checksums_file_warns_and_uses_pinned_hash(self) -> None:
        manifest = bootstrap.load_manifest()
        archive = b"verified archive"
        with (
            mock.patch.object(bootstrap, "sc_compose_target", return_value="aarch64-apple-darwin"),
            mock.patch.object(bootstrap, "_wyvern_release_checksum", return_value=hashlib.sha256(archive).hexdigest()),
            mock.patch.object(
                bootstrap,
                "_download_release",
                side_effect=[archive, bootstrap.BootstrapError("404 Not Found")],
            ),
            mock.patch.object(bootstrap, "_extract_wyvern") as extract,
        ):
            stderr = StringIO()
            with redirect_stderr(stderr):
                bootstrap.install_wyvern_release(manifest, dry_run=False)
        self.assertIn("upstream checksums.txt missing; verified against pinned SHA256 only", stderr.getvalue())
        extract.assert_called_once_with(archive, "wyvern_0.6.0_aarch64-apple-darwin.tar.gz", bootstrap.cargo_bin_path("wyvern"))

    def test_wyvern_present_checksums_file_disagreement_is_a_hard_failure(self) -> None:
        manifest = bootstrap.load_manifest()
        archive = b"verified archive"
        with (
            mock.patch.object(bootstrap, "sc_compose_target", return_value="aarch64-apple-darwin"),
            mock.patch.object(bootstrap, "_wyvern_release_checksum", return_value=hashlib.sha256(archive).hexdigest()),
                mock.patch.object(bootstrap, "_download_release", side_effect=[archive, b"bad-hash  wyvern_0.6.0_aarch64-apple-darwin.tar.gz\n"]),
        ):
            with self.assertRaisesRegex(bootstrap.BootstrapError, "checksums.txt does not confirm"):
                bootstrap.install_wyvern_release(manifest, dry_run=False)

    def test_sc_compose_checksum_mismatch_is_a_hard_failure(self) -> None:
        manifest = bootstrap.load_manifest()
        with (
            mock.patch.object(bootstrap, "sc_compose_target", return_value="aarch64-apple-darwin"),
            mock.patch.object(bootstrap, "_download_release", return_value=b"tampered"),
        ):
            with self.assertRaisesRegex(bootstrap.BootstrapError, "checksum mismatch"):
                bootstrap.install_sc_compose_release(manifest, dry_run=False)

    def test_sc_compose_install_leaves_ambient_cargo_sentinel_untouched(self) -> None:
        """Bootstrap test installs stay in the test CARGO_HOME, never the caller's."""
        ambient_binary = self._prepatch_cargo_home / "bin" / "sc-compose"
        ambient_binary.parent.mkdir(parents=True)
        ambient_binary.write_bytes(b"ambient-sentinel")

        binary = b"isolated-sc-compose"
        archive_buffer = io.BytesIO()
        with tarfile.open(fileobj=archive_buffer, mode="w:gz") as archive:
            member = tarfile.TarInfo("sc-compose")
            member.size = len(binary)
            archive.addfile(member, io.BytesIO(binary))
        archive = archive_buffer.getvalue()
        checksum = hashlib.sha256(archive).hexdigest()
        manifest = bootstrap.load_manifest()

        with (
            mock.patch.object(bootstrap, "sc_compose_target", return_value="aarch64-apple-darwin"),
            mock.patch.object(bootstrap, "_release_checksum", return_value=checksum),
            mock.patch.object(
                bootstrap,
                "_download_release",
                side_effect=[archive, f"{checksum}  sc-compose_1.7.0_aarch64-apple-darwin.tar.gz\n".encode()],
            ),
        ):
            bootstrap.install_sc_compose_release(manifest, dry_run=False)

        self.assertEqual(ambient_binary.read_bytes(), b"ambient-sentinel")
        self.assertEqual(
            bootstrap.cargo_bin_path("sc-compose"),
            Path(self._test_cargo_home.name) / "bin" / "sc-compose",
        )
        self.assertEqual(bootstrap.cargo_bin_path("sc-compose").read_bytes(), binary)

    def test_binstall_uses_prebuilt_only_and_exact_version(self) -> None:
        command = bootstrap.cargo_binstall_command("cargo-audit", "0.22.2", force=True)
        self.assertEqual(command, [
            str(bootstrap.cargo_bin_path("cargo-binstall")), "--no-confirm", "--disable-telemetry",
            "--disable-strategies", "quick-install,compile", "--force", "cargo-audit@0.22.2",
        ])

    def test_cargo_modules_uses_quick_install_without_compile(self) -> None:
        command = bootstrap.cargo_binstall_command(
            "cargo-modules", "0.26.0", force=True, allowed_strategies=("quick-install",)
        )
        self.assertEqual(command, [
            str(bootstrap.cargo_bin_path("cargo-binstall")), "--no-confirm", "--disable-telemetry",
            "--disable-strategies", "compile", "--force", "cargo-modules@0.26.0",
        ])

    def test_manifest_uses_current_compatible_stable_releases(self) -> None:
        manifest = bootstrap.load_manifest()
        self.assertEqual(manifest.python, "3.14.7")
        self.assertEqual(manifest.windows_python, "3.12.10")
        self.assertEqual(manifest.just, "1.58.0")
        self.assertEqual(dict(manifest.cargo_tools), {
            "cargo-deny": "0.20.2",
            "cargo-audit": "0.22.2",
            "cargo-shear": "1.13.3",
            "cargo-modules": "0.26.0",
            "cargo-xwin": "0.18.4",
        })
        self.assertEqual(dict(manifest.cargo_allowed_strategies), {
            "cargo-deny": (),
            "cargo-audit": ("quick-install",),
            "cargo-shear": (),
            "cargo-modules": ("quick-install",),
            "cargo-xwin": (),
        })
        self.assertEqual(manifest.cargo_binstall, "1.22.0")
        self.assertEqual(
            dict(manifest.cargo_binstall_checksums)["x86_64-pc-windows-msvc"],
            "0950f5a9f3422f74dea181d11f8b4d4447d1e9d1a9fdadcb72ff5b0b63c95207",
        )
        self.assertEqual(manifest.sc_compose, "1.7.0")
        self.assertEqual(manifest.wyvern, "0.6.0")
        self.assertEqual(dict(manifest.python_packages)["maturin"], "1.14.1")
        self.assertNotIn("sc-compose", dict(manifest.python_packages))
        requirements = (SCRIPT.parents[1] / "tools" / "bootstrap-requirements.txt").read_text(encoding="utf-8")
        self.assertNotIn("sc-compose==", requirements)

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
            mock.patch.object(bootstrap, "cargo_binstall_matches", return_value=False),
            mock.patch.object(bootstrap, "binstall_tool_matches", return_value=False),
            mock.patch.object(bootstrap, "sc_compose_matches", return_value=False),
            mock.patch.object(bootstrap, "wyvern_matches", return_value=False),
            mock.patch.object(bootstrap.subprocess, "run") as run,
        ):
            bootstrap.bootstrap(manifest, dry_run=True)
        run.assert_not_called()
        verify.assert_not_called()

    def test_windows_target_is_installed_only_off_windows(self) -> None:
        with mock.patch.object(bootstrap, "run") as run:
            with mock.patch.object(bootstrap.sys, "platform", "linux"):
                bootstrap.install_windows_msvc_target(dry_run=False)
        run.assert_called_once_with(
            ["rustup", "target", "add", bootstrap.WINDOWS_MSVC_TARGET], dry_run=False
        )
        with mock.patch.object(bootstrap, "run") as run:
            with mock.patch.object(bootstrap.sys, "platform", "win32"):
                bootstrap.install_windows_msvc_target(dry_run=False)
        run.assert_not_called()

    def test_cargo_xwin_is_required_only_off_windows(self) -> None:
        manifest = bootstrap.load_manifest()
        with mock.patch.object(bootstrap.sys, "platform", "win32"):
            self.assertNotIn(bootstrap.CARGO_XWIN, dict(bootstrap.cargo_tools_for_host(manifest)))
        with mock.patch.object(bootstrap.sys, "platform", "linux"):
            self.assertIn(bootstrap.CARGO_XWIN, dict(bootstrap.cargo_tools_for_host(manifest)))

    def test_bootstrap_installs_pinned_binstall_before_cargo_tools(self) -> None:
        manifest = bootstrap.load_manifest()
        with (
            mock.patch.object(bootstrap, "synchronize_macos_seed_tools"),
            mock.patch.object(bootstrap, "verify_seed_tools"),
            mock.patch.object(bootstrap, "ensure_bootstrap_venv", return_value=Path("/tmp/bootstrap-python")),
            mock.patch.object(bootstrap, "cargo_binstall_matches", return_value=False),
            mock.patch.object(bootstrap, "install_cargo_binstall_release") as install_binstall,
            mock.patch.object(bootstrap, "binstall_tool_matches", return_value=True),
            mock.patch.object(bootstrap, "sc_compose_matches", return_value=True),
            mock.patch.object(bootstrap, "wyvern_matches", return_value=True),
            mock.patch.object(bootstrap, "sc_compose_wheel_matches", return_value=True),
            mock.patch.object(bootstrap, "verify_installed_tools"),
            mock.patch.object(bootstrap, "run"),
        ):
            bootstrap.bootstrap(manifest, dry_run=False)
        install_binstall.assert_called_once_with(manifest, dry_run=False)

    def test_bootstrap_keeps_newer_sc_compose_cli_in_the_temporary_cargo_home(self) -> None:
        manifest = bootstrap.load_manifest()
        installed = bootstrap.cargo_bin_path("sc-compose")
        installed.parent.mkdir(parents=True)
        installed.write_bytes(b"newer-fake-cli")

        output = StringIO()
        with (
            mock.patch.object(bootstrap, "synchronize_macos_seed_tools"),
            mock.patch.object(bootstrap, "verify_seed_tools"),
            mock.patch.object(bootstrap, "ensure_bootstrap_venv", return_value=self._test_python),
            mock.patch.object(bootstrap, "cargo_binstall_matches", return_value=True),
            mock.patch.object(bootstrap, "binstall_tool_matches", return_value=True),
            mock.patch.object(bootstrap, "command_output", return_value="sc-compose 1.8.0"),
            mock.patch.object(bootstrap, "install_sc_compose_release") as install_cli,
            mock.patch.object(bootstrap, "wyvern_matches", return_value=True),
            mock.patch.object(bootstrap, "sc_compose_wheel_matches", return_value=True),
            mock.patch.object(bootstrap, "verify_installed_tools"),
            mock.patch.object(bootstrap, "run"),
            redirect_stdout(output),
        ):
            bootstrap.bootstrap(manifest, dry_run=False)

        install_cli.assert_not_called()
        self.assertEqual(installed.read_bytes(), b"newer-fake-cli")
        self.assertIn("keeping installed sc-compose CLI", output.getvalue())

    def test_bootstrap_replaces_old_sc_compose_cli_in_the_temporary_cargo_home(self) -> None:
        manifest = bootstrap.load_manifest()
        installed = bootstrap.cargo_bin_path("sc-compose")
        installed.parent.mkdir(parents=True)
        installed.write_bytes(b"older-fake-cli")

        def replace_cli(_: bootstrap.BootstrapManifest, *, dry_run: bool) -> None:
            self.assertFalse(dry_run)
            installed.write_bytes(b"pinned-replacement")

        with (
            mock.patch.object(bootstrap, "synchronize_macos_seed_tools"),
            mock.patch.object(bootstrap, "verify_seed_tools"),
            mock.patch.object(bootstrap, "ensure_bootstrap_venv", return_value=self._test_python),
            mock.patch.object(bootstrap, "cargo_binstall_matches", return_value=True),
            mock.patch.object(bootstrap, "binstall_tool_matches", return_value=True),
            mock.patch.object(bootstrap, "command_output", return_value="sc-compose 1.6.9"),
            mock.patch.object(bootstrap, "install_sc_compose_release", side_effect=replace_cli) as install_cli,
            mock.patch.object(bootstrap, "wyvern_matches", return_value=True),
            mock.patch.object(bootstrap, "sc_compose_wheel_matches", return_value=True),
            mock.patch.object(bootstrap, "verify_installed_tools"),
            mock.patch.object(bootstrap, "run"),
        ):
            bootstrap.bootstrap(manifest, dry_run=False)

        install_cli.assert_called_once_with(manifest, dry_run=False)
        self.assertEqual(installed.read_bytes(), b"pinned-replacement")

    def test_bootstrap_keeps_newer_sc_compose_wheel_in_the_temporary_venv(self) -> None:
        manifest = bootstrap.load_manifest()
        output = StringIO()
        with (
            mock.patch.object(bootstrap, "synchronize_macos_seed_tools"),
            mock.patch.object(bootstrap, "verify_seed_tools"),
            mock.patch.object(bootstrap, "ensure_bootstrap_venv", return_value=self._test_python),
            mock.patch.object(bootstrap, "cargo_binstall_matches", return_value=True),
            mock.patch.object(bootstrap, "binstall_tool_matches", return_value=True),
            mock.patch.object(bootstrap, "sc_compose_matches", return_value=True),
            mock.patch.object(bootstrap, "wyvern_matches", return_value=True),
            mock.patch.object(bootstrap, "sc_compose_wheel_matches", return_value=True),
            mock.patch.object(bootstrap, "verify_installed_tools"),
            mock.patch.object(bootstrap, "install_windows_msvc_target"),
            mock.patch.object(bootstrap, "run") as run,
            redirect_stdout(output),
        ):
            bootstrap.bootstrap(manifest, dry_run=False)

        self.assertEqual(run.call_args_list, [mock.call(bootstrap.pip_install_command(self._test_python), dry_run=False)])
        self.assertIn("keeping installed sc-compose wheel", output.getvalue())

    def test_bootstrap_replaces_old_sc_compose_wheel_in_the_temporary_venv(self) -> None:
        manifest = bootstrap.load_manifest()
        with (
            mock.patch.object(bootstrap, "synchronize_macos_seed_tools"),
            mock.patch.object(bootstrap, "verify_seed_tools"),
            mock.patch.object(bootstrap, "ensure_bootstrap_venv", return_value=self._test_python),
            mock.patch.object(bootstrap, "cargo_binstall_matches", return_value=True),
            mock.patch.object(bootstrap, "binstall_tool_matches", return_value=True),
            mock.patch.object(bootstrap, "sc_compose_matches", return_value=True),
            mock.patch.object(bootstrap, "wyvern_matches", return_value=True),
            mock.patch.object(bootstrap, "sc_compose_wheel_matches", return_value=False),
            mock.patch.object(bootstrap, "verify_installed_tools"),
            mock.patch.object(bootstrap, "install_windows_msvc_target"),
            mock.patch.object(bootstrap, "run") as run,
        ):
            bootstrap.bootstrap(manifest, dry_run=False)

        self.assertEqual(
            run.call_args_list,
            [
                mock.call(bootstrap.pip_install_command(self._test_python), dry_run=False),
                mock.call(bootstrap.sc_compose_wheel_install_command(self._test_python, manifest.sc_compose), dry_run=False),
            ],
        )

    def test_pip_installs_inside_the_repository_venv(self) -> None:
        python = (
            Path(r"C:\repo\.bootstrap-venv\Scripts\python.exe")
            if sys.platform == "win32"
            else Path("/repo/.bootstrap-venv/bin/python")
        )
        command = bootstrap.pip_install_command(python)
        self.assertEqual(Path(command[0]), python)
        self.assertIn("--no-deps", command)

    def test_sc_compose_wheel_uses_the_manifest_floor(self) -> None:
        python = Path("/repo/.bootstrap-venv/bin/python")
        command = bootstrap.sc_compose_wheel_install_command(python, "1.7.0")
        self.assertEqual(Path(command[0]), python)
        self.assertIn("sc-compose>=1.7.0,<2.0.0", command)
        self.assertNotIn("sc-compose==1.7.0", command)

    def test_sc_compose_cli_accepts_the_supported_release_line(self) -> None:
        manifest = bootstrap.load_manifest()
        with mock.patch.object(bootstrap, "command_output", side_effect=["sc-compose 1.7.0"]):
            self.assertTrue(bootstrap.sc_compose_matches(manifest))
        with mock.patch.object(bootstrap, "command_output", side_effect=["sc-compose 1.8.0"]):
            self.assertTrue(bootstrap.sc_compose_matches(manifest))
        with mock.patch.object(bootstrap, "command_output", side_effect=["sc-compose 1.6.9"]):
            self.assertFalse(bootstrap.sc_compose_matches(manifest))
        with mock.patch.object(bootstrap, "command_output", side_effect=["sc-compose 2.0.0"]):
            self.assertFalse(bootstrap.sc_compose_matches(manifest))

    def test_sc_compose_wheel_accepts_the_supported_release_line(self) -> None:
        manifest = bootstrap.load_manifest()
        python = Path("/repo/.bootstrap-venv/bin/python")
        for version, expected in (("1.7.0", True), ("1.8.0", True), ("1.6.9", False), ("2.0.0", False)):
            with self.subTest(version=version), mock.patch.object(bootstrap, "python_package_version", return_value=version):
                self.assertEqual(bootstrap.sc_compose_wheel_matches(manifest, python), expected)

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

    def test_binstall_failure_is_hard_and_has_no_registry_fallback(self) -> None:
        manifest = bootstrap.load_manifest()
        calls: list[list[str]] = []

        def failed_binstall(command: list[str], *, dry_run: bool) -> None:
            calls.append(command)
            raise bootstrap.BootstrapError("prebuilt install failed")

        with (
            mock.patch.object(bootstrap, "synchronize_macos_seed_tools"),
            mock.patch.object(bootstrap, "verify_seed_tools"),
            mock.patch.object(bootstrap, "ensure_bootstrap_venv", return_value=Path("/tmp/bootstrap-python")),
            mock.patch.object(bootstrap, "cargo_binstall_matches", return_value=True),
            mock.patch.object(bootstrap, "binstall_tool_matches", return_value=False),
            mock.patch.object(bootstrap, "run", side_effect=failed_binstall),
        ):
            with self.assertRaisesRegex(bootstrap.BootstrapError, "prebuilt install failed"):
                bootstrap.bootstrap(manifest, dry_run=False)
        self.assertEqual(len(calls), 1)
        self.assertEqual(calls[0][0], str(bootstrap.cargo_bin_path("cargo-binstall")))
        source = (SCRIPT.parents[1] / "tools" / "bootstrap.py").read_text(encoding="utf-8")
        self.assertNotIn('"cargo", "install"', source)

    def test_ci_uses_the_shared_bootstrap_recipe(self) -> None:
        workflow = (SCRIPT.parents[1] / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        self.assertIn('python: "3.14.7"', workflow)
        self.assertIn('python: "3.12.10"', workflow)
        self.assertIn("python-version: ${{ matrix.python }}", workflow)
        self.assertIn("tool: just@1.58.0", workflow)
        self.assertIn("cargo-bins/cargo-binstall@75b4bfae1b2c753a6806bbce6e6cb89b602de33c", workflow)
        self.assertEqual(workflow.count("Pre-seed pinned cargo-binstall (bootstrap also verifies)"), 2)
        self.assertGreaterEqual(workflow.count("run: just bootstrap"), 2)
        just_lint = workflow.split("\n  fmt:", 1)[0]
        sc_lint_setup = "- name: Set up sc-lint\n        run: just setup"
        self.assertIn(sc_lint_setup, just_lint)
        self.assertLess(just_lint.index("SC_LINT_INSTALL_DIR"), just_lint.index(sc_lint_setup))
        self.assertLess(just_lint.index(sc_lint_setup), just_lint.index("- name: Run just lint"))
        self.assertLess(just_lint.index("run: just bootstrap"), just_lint.index("sc-compose --version"))

    def test_ci_caches_xwin_sdk_for_non_windows_lint_before_clippy(self) -> None:
        workflow = (SCRIPT.parents[1] / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")
        self.assertNotIn("SC_LINT_INSTALL_DIR: ${{ runner.temp }}", workflow)
        self.assertEqual(workflow.count("- name: Configure platform cache directories"), 1)
        self.assertEqual(workflow.count("os.environ['RUNNER_TEMP']"), 1)
        self.assertEqual(workflow.count("os.environ['GITHUB_ENV']"), 1)
        self.assertEqual(workflow.count("${{ env.SC_LINT_INSTALL_DIR }}"), 1)
        just_lint = workflow.split("\n  fmt:", 1)[0]
        self.assertIn("xwin = cache_root / 'xwin'", just_lint)
        self.assertIn("'XWIN_CACHE_DIR=' + str(xwin)", just_lint)
        cache_step = "- name: Cache xwin SDK\n        if: runner.os != 'Windows'\n        uses: actions/cache@v4"
        self.assertIn(cache_step, just_lint)
        self.assertIn("path: ${{ env.XWIN_CACHE_DIR }}", just_lint)
        self.assertLess(just_lint.index("XWIN_CACHE_DIR=' + str(xwin)"), just_lint.index(cache_step))
        self.assertLess(just_lint.index(cache_step), just_lint.index("run: just lint ci all"))

        test_job = workflow.split("\n  test:", 1)[1].split("\n  hermes-atm-release-wheels:", 1)[0]
        self.assertNotIn("SC_LINT_INSTALL_DIR", test_job)
        self.assertNotIn("Configure platform cache directories", test_job)

    def test_just_recipes_propagate_the_bootstrap_python_to_children(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn("$PWD/.bootstrap-venv/bin:$PATH", justfile)
        self.assertIn(
            "test-admission-capacity:\n"
            "    {{python_cmd}} -m unittest scripts/smoke/test_run_admission_capacity.py",
            justfile,
        )

    def test_sc_lint_clippy_profiles_keep_pyo3_on_the_bootstrap_interpreter(self) -> None:
        root = SCRIPT.parents[1]
        install = json.loads((root / ".sc-lint" / "install.json").read_text(encoding="utf-8"))
        profiles = install["profiles"]
        command = ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]
        for name, platform, python in (
            ("clippy-linux", "linux", ".bootstrap-venv/bin/python"),
            ("clippy-macos", "macos", ".bootstrap-venv/bin/python"),
            ("clippy-native-windows", "windows", ".bootstrap-venv/Scripts/python.exe"),
        ):
            self.assertEqual(profiles[name], {"command": command, "platform": platform, "env_paths": {"PYO3_PYTHON": python}})
        self.assertEqual(profiles["clippy-windows"], {"command": command, "platform": "windows", "target": "x86_64-pc-windows-msvc", "host": "linux", "env_paths": {"PYO3_PYTHON": ".bootstrap-venv/bin/python"}})
        self.assertEqual(install["lint_profiles"], {"ci": list(profiles)})

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

    def test_windows_seed_python_selects_the_stable_pinned_major_minor(self) -> None:
        justfile = (SCRIPT.parents[1] / "Justfile").read_text(encoding="utf-8")
        self.assertIn('if os_family() == "windows" { "py -3.12" }', justfile)

    def test_seed_python_version_preserves_unix_pin_and_selects_windows_pin(self) -> None:
        manifest = bootstrap.load_manifest()
        with mock.patch.object(bootstrap.sys, "platform", "win32"):
            self.assertEqual(bootstrap.seed_python_version(manifest), "3.12.10")
        with mock.patch.object(bootstrap.sys, "platform", "linux"):
            self.assertEqual(bootstrap.seed_python_version(manifest), "3.14.7")


    def test_cli_reference_source_assets_are_pinned_to_lf(self) -> None:
        attributes = SCRIPT.parents[1] / ".gitattributes"
        self.assertIn(
            "crates/atm/assets/cli-reference/* text eol=lf",
            attributes.read_text(encoding="utf-8"),
        )


if __name__ == "__main__":
    unittest.main()
