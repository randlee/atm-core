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

    def test_send_acknowledgement_shape_omits_destination_and_disallows_nested_ack(self):
        models = load_models()
        acknowledgement = models.AtmSendRequest.model_validate(
            {"body": "received", "acknowledges_message_id": "01KZSV6V4JDE569ENXJD8KZ0RC"}
        )
        self.assertIsNone(acknowledgement.to)
        self.assertFalse(acknowledgement.requires_ack)
        for invalid in (
            {
                "to": "team-lead@atm-dev",
                "body": "received",
                "acknowledges_message_id": "01KZSV6V4JDE569ENXJD8KZ0RC",
            },
            {
                "body": "received",
                "requires_ack": True,
                "acknowledges_message_id": "01KZSV6V4JDE569ENXJD8KZ0RC",
            },
        ):
            with self.assertRaises(Exception):
                models.AtmSendRequest.model_validate(invalid)

    def test_read_and_list_reject_mutating_arguments(self):
        models = load_models()
        for model in (models.AtmReadRequest, models.AtmListRequest):
            with self.assertRaises(Exception):
                model.model_validate({"mark_seen": True})
            with self.assertRaises(Exception):
                model.model_validate({"acknowledge": "01TEST"})

    def test_tool_schema_describes_every_model_field(self):
        models = load_models()
        expected = {
            models.AtmSendRequest: {"to", "body", "requires_ack", "acknowledges_message_id"},
            models.AtmReadRequest: {
                "selection",
                "message_id",
                "task",
                "contains",
                "since",
                "from_agent",
            },
            models.AtmListRequest: {
                "selection",
                "limit",
                "task",
                "contains",
                "since",
                "from_agent",
            },
        }
        for model, fields in expected.items():
            properties = model.model_json_schema()["properties"]
            self.assertEqual(set(properties), fields)
            for name in fields:
                self.assertTrue(properties[name].get("description"), name)

        send = models.AtmSendRequest.model_json_schema()["properties"]
        self.assertIn("bare identity", send["to"]["description"])
        self.assertIn("reviewer@release", send["to"]["description"])
        self.assertIn("defaults to false", send["requires_ack"]["description"])
        self.assertIn("Acknowledge", send["acknowledges_message_id"]["description"])
        for model in (models.AtmReadRequest, models.AtmListRequest):
            selection = model.model_json_schema()["properties"]["selection"]["description"]
            for value in ("actionable", "all", "unread", "pending_ack"):
                self.assertIn(value, selection)


if __name__ == "__main__":
    unittest.main()
