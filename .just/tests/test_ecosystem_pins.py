from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
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


if __name__ == "__main__":
    unittest.main()
