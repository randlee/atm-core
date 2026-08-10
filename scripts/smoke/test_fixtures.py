"""Artifact-path tests for the canonical smoke report layout."""
from __future__ import annotations

from datetime import datetime, timezone
import importlib.util
import os
from pathlib import Path
import sys
from unittest import mock
import unittest


def load_fixtures():
    path = Path(__file__).with_name("fixtures.py")
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location("smoke_fixtures", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


FIXTURES = load_fixtures()


class SmokePathsTests(unittest.TestCase):
    def test_fixture_artifacts_are_platform_host_and_run_isolated_under_site_reports(self):
        now = datetime(2026, 8, 8, 0, 12, 34, 567890, tzinfo=timezone.utc)
        with mock.patch.dict(os.environ, {"ATM_SMOKE_RUN_ID": "hardware-run"}, clear=False), mock.patch.object(
            FIXTURES, "repo_root", return_value=Path("/repo")
        ), mock.patch.object(FIXTURES.platform, "system", return_value="Windows"), mock.patch.object(
            FIXTURES.platform, "node", return_value="cwin"
        ), mock.patch.object(FIXTURES.os, "getpid", return_value=42):
            paths = FIXTURES.smoke_paths("thorough", now)
        self.assertEqual(paths.reports_root, Path("/repo/site/reports/smoke"))
        self.assertEqual(paths.report_dir, Path("/repo/site/reports/smoke/windows/cwin/hardware-run-pid42-smoke-thorough"))
        self.assertEqual(paths.markdown, paths.report_dir / "smoke-thorough.md")
        self.assertEqual(paths.json, paths.report_dir / "smoke-thorough.json")

    def test_fixture_run_id_uses_microseconds_when_not_explicitly_supplied(self):
        now = datetime(2026, 8, 8, 0, 12, 34, 567890, tzinfo=timezone.utc)
        with mock.patch.dict(os.environ, {}, clear=True), mock.patch.object(
            FIXTURES, "repo_root", return_value=Path("/repo")
        ), mock.patch.object(FIXTURES.platform, "system", return_value="Darwin"), mock.patch.object(
            FIXTURES.platform, "node", return_value="m5"
        ), mock.patch.object(FIXTURES.os, "getpid", return_value=9):
            paths = FIXTURES.smoke_paths("fast", now)
        self.assertIn("20260808T001234567890Z-pid9-smoke-fast", str(paths.report_dir))
