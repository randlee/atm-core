from __future__ import annotations

import json
from pathlib import Path
import sys
import tempfile
import types
import unittest

# Hook configuration validation does not use the compiled graft extension.
# Keep this module runnable with the documented source-only test command.
sys.modules.setdefault("atm_graft", types.ModuleType("atm_graft"))

from hermes_atm.hook import _configuration
from hermes_atm import HermesAtmRuntimeError


BASE_CONFIGURATION = {
    "schema_version": 1,
    "profile": "test-profile",
    "atm_home": "/tmp/atm",
    "identity": "test-agent",
    "team": "test-team",
    "chat_id": "100000001",
    "workspace_root": "/tmp/workspace",
}


class HookConfigurationTests(unittest.TestCase):
    def write_configuration(self, value: dict) -> Path:
        directory = Path(self.temporary.name)
        path = directory / "config.json"
        path.write_text(json.dumps(value), encoding="utf-8")
        return path

    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_missing_platform_defaults_to_telegram(self) -> None:
        configuration = _configuration(self.write_configuration(BASE_CONFIGURATION))

        self.assertEqual(configuration["platform"], "telegram")

    def test_invalid_platform_values_raise_runtime_error(self) -> None:
        for value in ("", "  ", None, 42):
            with self.subTest(platform=value):
                configuration = {**BASE_CONFIGURATION, "platform": value}
                with self.assertRaises(HermesAtmRuntimeError):
                    _configuration(self.write_configuration(configuration))


if __name__ == "__main__":
    unittest.main()
