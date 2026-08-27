from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "validate_release.py"
if str(SCRIPT.parent) not in sys.path:
    sys.path.insert(0, str(SCRIPT.parent))
SPEC = importlib.util.spec_from_file_location("validate_release_ecosystem", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
VALIDATE_RELEASE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = VALIDATE_RELEASE
SPEC.loader.exec_module(VALIDATE_RELEASE)


class EcosystemPinTests(unittest.TestCase):
    def test_shared_picker_fixtures_enforce_version_one_shapes(self) -> None:
        picker_input = json.loads(
            (REPO_ROOT / "docs/plans/phase-aq/fixtures/picker-input-v1.json").read_text()
        )
        picker_output = json.loads(
            (REPO_ROOT / "docs/plans/phase-aq/fixtures/picker-output-v1.json").read_text()
        )
        unknown_output = json.loads(
            (REPO_ROOT / "docs/plans/phase-aq/fixtures/picker-output-unknown-schema.json").read_text()
        )

        self.assertEqual(picker_input["schema_version"], 1)
        self.assertEqual(set(picker_output), {"schema_version", "recipients", "note"})
        self.assertEqual(picker_output["schema_version"], 1)
        self.assertNotEqual(unknown_output["schema_version"], 1)

    def test_exact_cargo_versions_compare_with_bare_registry_versions(self) -> None:
        self.assertEqual(VALIDATE_RELEASE.normalized_dependency_version("=1.2.0"), "1.2.0")
        self.assertEqual(VALIDATE_RELEASE.normalized_dependency_version("  1.2.0  "), "1.2.0")

    def test_workspace_dependencies_are_visible_to_currency_inventory(self) -> None:
        dependencies = VALIDATE_RELEASE.direct_registry_dependencies(REPO_ROOT)
        self.assertEqual(dependencies["sc-observability"], "=1.2.0")
        self.assertEqual(dependencies["sc-observability-types"], "=1.2.0")

    @mock.patch.object(VALIDATE_RELEASE, "latest_wyvern_version", return_value="0.5.0")
    @mock.patch.object(VALIDATE_RELEASE, "latest_registry_version")
    @mock.patch.object(VALIDATE_RELEASE.shutil, "which", return_value=None)
    def test_missing_wyvern_is_a_distinct_actionable_blocker(
        self,
        _which: mock.Mock,
        latest_registry: mock.Mock,
        _latest_wyvern: mock.Mock,
    ) -> None:
        latest_registry.side_effect = lambda _root, dependency: {
            "sc-composer": "1.5.0",
            "sc-observability": "1.2.0",
            "sc-observability-types": "1.2.0",
        }[dependency]
        findings: list[VALIDATE_RELEASE.Finding] = []

        VALIDATE_RELEASE.validate_ecosystem_currency(REPO_ROOT, findings, dry_run=True)

        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].check, "sc-ecosystem-preflight")
        self.assertIn("required on PATH", findings[0].summary)
        self.assertIn("install wyvern before running preflight", findings[0].detail)

    @mock.patch.object(VALIDATE_RELEASE, "maybe_file_dep_currency_issue", return_value="https://example.test/issue/1")
    @mock.patch.object(VALIDATE_RELEASE, "latest_wyvern_version", return_value="0.5.0")
    @mock.patch.object(VALIDATE_RELEASE, "latest_registry_version", return_value="1.5.0")
    @mock.patch.object(VALIDATE_RELEASE.shutil, "which", return_value="/usr/bin/wyvern")
    def test_upstream_regression_reuses_issue_escape_hatch(
        self,
        _which: mock.Mock,
        _latest_registry: mock.Mock,
        _latest_wyvern: mock.Mock,
        file_issue: mock.Mock,
    ) -> None:
        findings: list[VALIDATE_RELEASE.Finding] = []

        VALIDATE_RELEASE.validate_ecosystem_currency(REPO_ROOT, findings, dry_run=True)

        self.assertTrue(any("pin is not the latest" in finding.summary for finding in findings))
        file_issue.assert_any_call(
            REPO_ROOT,
            [
                ("sc-observability", "=1.2.0", "1.5.0"),
                ("sc-observability-types", "=1.2.0", "1.5.0"),
            ],
        )

    @mock.patch.object(VALIDATE_RELEASE, "run_ecosystem_command", return_value=False)
    @mock.patch.object(VALIDATE_RELEASE, "maybe_file_dep_currency_issue", return_value="https://example.test/issue/2")
    @mock.patch.object(VALIDATE_RELEASE, "latest_wyvern_version", return_value="0.6.0")
    @mock.patch.object(VALIDATE_RELEASE, "latest_registry_version")
    @mock.patch.object(VALIDATE_RELEASE.shutil, "which", return_value="/usr/bin/tool")
    def test_failed_latest_wyvern_probe_pins_back_and_records_evidence(
        self,
        _which: mock.Mock,
        latest_registry: mock.Mock,
        _latest_wyvern: mock.Mock,
        _file_issue: mock.Mock,
        _run_command: mock.Mock,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for relative_path in (
                Path("Cargo.toml"),
                Path("crates/atm-template-sc-compose/Cargo.toml"),
                *VALIDATE_RELEASE.WYVERN_PIN_FILES,
            ):
                destination = root / relative_path
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(REPO_ROOT / relative_path, destination)
            for path in root.joinpath("scripts/send-to").glob("atm-send-to.*"):
                path.write_text(path.read_text().replace("0.5.0", "0.6.0"), encoding="utf-8")
            evidence = root / "evidence.md"
            latest_registry.side_effect = lambda _root, dependency: {
                "sc-composer": "1.5.0",
                "sc-observability": "1.2.0",
                "sc-observability-types": "1.2.0",
            }[dependency]
            findings: list[VALIDATE_RELEASE.Finding] = []
            with mock.patch.dict(
                VALIDATE_RELEASE.os.environ,
                {
                    VALIDATE_RELEASE.ECOSYSTEM_FIX_FORWARD_ENV: "1",
                    VALIDATE_RELEASE.ECOSYSTEM_KNOWN_GOOD_ENV: json.dumps(
                        {
                            "wyvern": "0.5.0",
                            "sc-composer": "1.4.1",
                            "sc-observability": "1.1.0",
                        }
                    ),
                    VALIDATE_RELEASE.ECOSYSTEM_EVIDENCE_ENV: str(evidence),
                },
                clear=False,
            ):
                VALIDATE_RELEASE.validate_ecosystem_currency(root, findings)

            self.assertIn('WYVERN_PIN="0.5.0"', (root / VALIDATE_RELEASE.WYVERN_PIN_FILES[0]).read_text())
            self.assertIn('$wyvernPin = "0.5.0"', (root / VALIDATE_RELEASE.WYVERN_PIN_FILES[1]).read_text())
            cargo_text = (root / "Cargo.toml").read_text()
            self.assertIn('sc-observability = "=1.1.0"', cargo_text)
            self.assertIn('sc-observability-types = "=1.1.0"', cargo_text)
            compose_text = (root / "crates/atm-template-sc-compose/Cargo.toml").read_text()
            self.assertIn('sc-composer = "=1.4.1"', compose_text)
            self.assertIn('sc-sha = "=1.4.1"', compose_text)
            evidence_text = evidence.read_text()
            self.assertIn("pinned back to last known-good: `0.5.0`", evidence_text)
            self.assertIn("https://example.test/issue/2", evidence_text)
            self.assertTrue(any("pinned back to 0.5.0" in finding.summary for finding in findings))

    @mock.patch.object(VALIDATE_RELEASE, "latest_wyvern_version", return_value="0.5.0")
    @mock.patch.object(VALIDATE_RELEASE, "latest_registry_version")
    @mock.patch.object(VALIDATE_RELEASE.shutil, "which", return_value="/usr/bin/wyvern")
    def test_healthy_latest_does_not_rewrite_pins(
        self,
        _which: mock.Mock,
        latest_registry: mock.Mock,
        _latest_wyvern: mock.Mock,
    ) -> None:
        latest_registry.side_effect = lambda _root, dependency: {
            "sc-composer": "1.5.0",
            "sc-observability": "1.2.0",
            "sc-observability-types": "1.2.0",
        }[dependency]
        before = {
            path: path.read_text(encoding="utf-8")
            for path in (
                REPO_ROOT / "Cargo.toml",
                REPO_ROOT / "crates/atm-template-sc-compose/Cargo.toml",
                *(REPO_ROOT / relative for relative in VALIDATE_RELEASE.WYVERN_PIN_FILES),
            )
        }
        findings: list[VALIDATE_RELEASE.Finding] = []
        with mock.patch.dict(
            VALIDATE_RELEASE.os.environ,
            {
                VALIDATE_RELEASE.ECOSYSTEM_FIX_FORWARD_ENV: "1",
                VALIDATE_RELEASE.ECOSYSTEM_KNOWN_GOOD_ENV: json.dumps(
                    {"sc-composer": "1.4.1", "sc-observability": "1.1.0", "wyvern": "0.4.0"}
                ),
            },
            clear=False,
        ):
            VALIDATE_RELEASE.validate_ecosystem_currency(REPO_ROOT, findings, dry_run=True)
        after = {path: path.read_text(encoding="utf-8") for path in before}
        self.assertEqual(before, after)
        self.assertFalse(any("pinned back" in finding.summary for finding in findings))

    @mock.patch.object(VALIDATE_RELEASE, "run_capture")
    def test_wyvern_release_lookup_uses_approved_repository(self, run_capture: mock.Mock) -> None:
        run_capture.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout='[{"tagName":"v0.5.0"}]', stderr=""
        )

        self.assertEqual(VALIDATE_RELEASE.latest_wyvern_version(REPO_ROOT), "0.5.0")
        command = run_capture.call_args.args[0]
        self.assertEqual(
            command[:7],
            ["gh", "release", "list", "--repo", "randlee/wyvern", "--limit", "1"],
        )

    @mock.patch.object(VALIDATE_RELEASE, "run_capture")
    def test_wyvern_release_lookup_falls_back_to_rest(self, run_capture: mock.Mock) -> None:
        run_capture.side_effect = [
            subprocess.CompletedProcess(args=[], returncode=1, stdout="", stderr="rate limited"),
            subprocess.CompletedProcess(args=[], returncode=0, stdout='{"tag_name":"v0.6.0"}', stderr=""),
        ]

        self.assertEqual(VALIDATE_RELEASE.latest_wyvern_version(REPO_ROOT), "0.6.0")
        self.assertEqual(
            run_capture.call_args_list[1].args[0],
            ["gh", "api", "repos/randlee/wyvern/releases/latest"],
        )


if __name__ == "__main__":
    unittest.main()
