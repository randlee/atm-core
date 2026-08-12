from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest


MODELS_PATH = (
    Path(__file__).resolve().parents[2]
    / "crates"
    / "atm-graft-python"
    / "python"
    / "atm_graft"
    / "models.py"
)


def load_models():
    spec = importlib.util.spec_from_file_location("atm_graft_models_test", MODELS_PATH)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


class AtmGraftPythonModelTests(unittest.TestCase):
    def test_send_defaults_requires_ack_and_rejects_unknown_tool_fields(self):
        models = load_models()
        request = models.AtmSendRequest.model_validate(
            {"to": "team-lead@atm-dev", "body": "hello"}
        )
        self.assertFalse(request.requires_ack)
        with self.assertRaises(Exception):
            models.AtmSendRequest.model_validate(
                {"to": "team-lead@atm-dev", "body": "hello", "identity": "override"}
            )

    def test_read_and_list_reject_mutating_arguments(self):
        models = load_models()
        for model in (models.AtmReadRequest, models.AtmListRequest):
            with self.assertRaises(Exception):
                model.model_validate({"mark_seen": True})
            with self.assertRaises(Exception):
                model.model_validate({"acknowledge": "01TEST"})


if __name__ == "__main__":
    unittest.main()
