"""Package-native unit tests for the vendored GitHub scripts."""

from __future__ import annotations

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


PACKAGE_ROOT = next(path for path in Path(__file__).resolve().parents if (path / "install.py").is_file())
SCRIPTS = PACKAGE_ROOT / ".github" / "scripts"
sys.path.insert(0, str(SCRIPTS))
import release_manifest  # noqa: E402

BOOTSTRAP_SPEC = importlib.util.spec_from_file_location(
    "bootstrap_sc_compose", SCRIPTS / "bootstrap_sc_compose.py"
)
assert BOOTSTRAP_SPEC is not None and BOOTSTRAP_SPEC.loader is not None
BOOTSTRAP = importlib.util.module_from_spec(BOOTSTRAP_SPEC)
BOOTSTRAP_SPEC.loader.exec_module(BOOTSTRAP)


class ReleaseManifestTests(unittest.TestCase):
    def test_channel_contracts_describe_all_six_workers(self) -> None:
        contracts = release_manifest.load_channel_contracts(
            PACKAGE_ROOT / "release" / "publish-channel-contracts.toml.j2"
        )
        self.assertEqual(
            {contract["agent"] for contract in contracts.values()},
            {
                "crates-io-publisher",
                "github-release-publisher",
                "pypi-publisher",
                "homebrew-publisher",
                "scoop-publisher",
                "winget-publisher",
            },
        )
        self.assertEqual(contracts["pypi"]["stage"], "post_release")
        self.assertEqual(contracts["crates_io"]["repository_secrets"], ["CARGO_REGISTRY_TOKEN"])

    def test_load_manifest_orders_crates_by_publish_order(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / "publish-artifacts.toml"
            manifest.write_text(
                """[[crates]]
artifact = "late"
package = "late"
publish_order = 2

[[crates]]
artifact = "first"
package = "first"
publish_order = 1

[[release_binaries]]
name = "example"
""",
                encoding="utf-8",
            )
            loaded = release_manifest.load_manifest(manifest)
        self.assertEqual([crate["artifact"] for crate in loaded["crates"]], ["first", "late"])
        self.assertEqual(loaded["release_binaries"], [{"name": "example"}])


class RegistryVersionStateTests(unittest.TestCase):
    def test_lookup_statuses_map_to_published_absent_or_fail_closed(self) -> None:
        import urllib.error
        from unittest.mock import patch

        class FakeResponse:
            def __init__(self, status: int) -> None:
                self.status = status

            def __enter__(self) -> "FakeResponse":
                return self

            def __exit__(self, *args: object) -> bool:
                return False

        def http_error(code: int) -> urllib.error.HTTPError:
            return urllib.error.HTTPError("https://registry.invalid", code, "", {}, None)

        with patch("urllib.request.urlopen", return_value=FakeResponse(200)):
            self.assertEqual(
                release_manifest.registry_version_state("https://registry.invalid"),
                "published",
            )
        with patch("urllib.request.urlopen", side_effect=http_error(404)):
            self.assertEqual(
                release_manifest.registry_version_state("https://registry.invalid"),
                "absent",
            )
        with patch("urllib.request.urlopen", side_effect=http_error(503)):
            with self.assertRaisesRegex(SystemExit, "indeterminate"):
                release_manifest.registry_version_state("https://registry.invalid")
        with patch(
            "urllib.request.urlopen",
            side_effect=urllib.error.URLError("network unreachable"),
        ):
            with self.assertRaisesRegex(SystemExit, "registry lookup failed"):
                release_manifest.registry_version_state("https://registry.invalid")


class ReleaseScriptTests(unittest.TestCase):
    def test_release_gate_has_valid_bash_syntax(self) -> None:
        result = subprocess.run(
            ["bash", "-n", str(SCRIPTS / "release_gate.sh")],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_release_gate_accepts_main_with_post_cut_develop_drift(self) -> None:
        """A release stays valid when new work lands on develop after the RC cut."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            remote = root / "remote.git"
            repo = root / "repo"
            self._git(root, "init", "--bare", str(remote))
            self._git(root, "init", str(repo))
            self._git(repo, "config", "user.email", "tests@example.invalid")
            self._git(repo, "config", "user.name", "Publish Kit Tests")
            self._git(repo, "remote", "add", "origin", str(remote))
            (repo / ".github" / "scripts").mkdir(parents=True)
            (repo / ".github" / "scripts" / "release_artifacts.py").write_text(
                "raise SystemExit(0)\n", encoding="utf-8"
            )
            (repo / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            (repo / "release").mkdir()
            (repo / "release" / "publish-artifacts.toml").write_text("", encoding="utf-8")
            (repo / "README.md").write_text("base\n", encoding="utf-8")
            self._git(repo, "add", ".")
            self._git(repo, "commit", "-m", "base")
            self._git(repo, "branch", "-M", "main")
            self._git(repo, "push", "-u", "origin", "main")
            self._git(repo, "checkout", "-b", "develop")
            (repo / "README.md").write_text("candidate\n", encoding="utf-8")
            self._git(repo, "commit", "-am", "candidate")
            self._git(repo, "push", "-u", "origin", "develop")
            self._git(repo, "tag", "-a", "release-candidate-v1.2.3", "-m", "candidate")
            self._git(repo, "push", "origin", "release-candidate-v1.2.3")
            self._git(repo, "checkout", "main")
            self._git(repo, "merge", "--ff-only", "develop")
            (repo / "CHANGELOG.md").write_text("release metadata\n", encoding="utf-8")
            self._git(repo, "add", "CHANGELOG.md")
            self._git(repo, "commit", "-m", "release metadata")
            self._git(repo, "push", "origin", "main")
            self._git(repo, "checkout", "develop")
            (repo / "post-cut.rs").write_text("// new develop work\n", encoding="utf-8")
            self._git(repo, "add", "post-cut.rs")
            self._git(repo, "commit", "-m", "post-cut develop work")
            self._git(repo, "push", "origin", "develop")
            self._git(repo, "checkout", "main")

            result = subprocess.run(
                [
                    "bash",
                    str(SCRIPTS / "release_gate.sh"),
                    "final",
                    "origin/main",
                    "release-candidate-v1.2.3",
                    "1.2.3",
                    "release/publish-artifacts.toml",
                    "Cargo.toml",
                ],
                cwd=repo,
                text=True,
                capture_output=True,
                check=False,
            )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PASS - release gate checks satisfied", result.stdout)

    def test_release_gate_rejects_candidate_outside_release_history(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            remote = root / "remote.git"
            repo = root / "repo"
            self._git(root, "init", "--bare", str(remote))
            self._git(root, "init", str(repo))
            self._git(repo, "config", "user.email", "tests@example.invalid")
            self._git(repo, "config", "user.name", "Publish Kit Tests")
            self._git(repo, "remote", "add", "origin", str(remote))
            (repo / ".github" / "scripts").mkdir(parents=True)
            (repo / ".github" / "scripts" / "release_artifacts.py").write_text(
                "raise SystemExit(0)\n", encoding="utf-8"
            )
            (repo / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            (repo / "release").mkdir()
            (repo / "release" / "publish-artifacts.toml").write_text("", encoding="utf-8")
            (repo / "README.md").write_text("main\n", encoding="utf-8")
            self._git(repo, "add", ".")
            self._git(repo, "commit", "-m", "main")
            self._git(repo, "branch", "-M", "main")
            self._git(repo, "push", "-u", "origin", "main")
            self._git(repo, "checkout", "--orphan", "unrelated")
            self._git(repo, "rm", "-rf", ".")
            (repo / "unrelated.txt").write_text("candidate\n", encoding="utf-8")
            self._git(repo, "add", "unrelated.txt")
            self._git(repo, "commit", "-m", "unrelated candidate")
            self._git(repo, "tag", "-a", "release-candidate-v1.2.3", "-m", "candidate")
            self._git(repo, "push", "origin", "release-candidate-v1.2.3")
            self._git(repo, "checkout", "main")

            result = subprocess.run(
                [
                    "bash",
                    str(SCRIPTS / "release_gate.sh"),
                    "final",
                    "origin/main",
                    "release-candidate-v1.2.3",
                    "1.2.3",
                    "release/publish-artifacts.toml",
                    "Cargo.toml",
                ],
                cwd=repo,
                text=True,
                capture_output=True,
                check=False,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("is not an ancestor of origin/main", result.stderr)

    @staticmethod
    def _git(cwd: Path, *args: str) -> None:
        result = subprocess.run(
            ["git", *args],
            cwd=cwd,
            text=True,
            capture_output=True,
            check=False,
        )
        if result.returncode:
            raise AssertionError(f"git {' '.join(args)} failed: {result.stderr}")

    def test_release_artifacts_cli_exposes_read_only_inquiry(self) -> None:
        result = subprocess.run(
            [sys.executable, str(SCRIPTS / "release_artifacts.py"), "--help"],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("public-registry-inquiry-plan", result.stdout)
        self.assertIn("preflight-secret-plan", result.stdout)

    def test_bootstrap_enforces_the_documented_renderer_version_floor(self) -> None:
        script = SCRIPTS / "bootstrap_sc_compose.py"
        text = script.read_text(encoding="utf-8")
        probe = text[text.index("def installed_version"):text.index("def version_components")]
        self.assertIn('SC_COMPOSE_VERSION = "1.4.1"', text)
        self.assertIn("historical compatibility floor", text)
        self.assertIn('"venv"', text)
        self.assertIn('f"sc-compose=={SC_COMPOSE_VERSION}"', text)
        self.assertIn("from importlib.metadata import version", probe)
        self.assertNotIn("import sc_compose", probe)
        self.assertNotIn("existing != SC_COMPOSE_VERSION", text)
        self.assertIn("require_version_floor(existing)", text)
        self.assertIn("managed environment has incompatible sc-compose wheel", text)

    def test_bootstrap_rejects_a_too_old_wheel(self) -> None:
        with self.assertRaisesRegex(
            SystemExit,
            r"stale version '1\.4\.0'; required >= 1\.4\.1",
        ):
            BOOTSTRAP.require_version_floor("1.4.0")

    def test_bootstrap_accepts_the_floor_and_newer_wheels(self) -> None:
        BOOTSTRAP.require_version_floor("1.4.1")
        BOOTSTRAP.require_version_floor("1.5.0")


if __name__ == "__main__":
    unittest.main()
