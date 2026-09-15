from __future__ import annotations

import json
import sys
import types
import unittest

# The isolated adapter test does not need a compiled graft wheel.  The
# production package imports the real extension; this stub is replaced per
# test before any handler is exercised.
if "atm_graft" not in sys.modules:
    sys.modules["atm_graft"] = types.ModuleType("atm_graft")

from hermes_atm import native_tools

TEST_IDENTITY = "test-agent"
TEST_TEAM = "test-team"
TEST_CHAT_ID = "local"


class _Model:
    def __init__(self, **values):
        self.__dict__.update(values)

    @classmethod
    def model_validate(cls, value):
        allowed = {"to", "body", "requires_ack", "acknowledges_message_id"}
        unknown = set(value) - allowed
        if unknown:
            raise _ValidationError()
        return cls(
            to=value.get("to"),
            body=value["body"],
            requires_ack=value.get("requires_ack", False),
            acknowledges_message_id=value.get("acknowledges_message_id"),
        )

    @classmethod
    def model_json_schema(cls):
        return {"type": "object"}


class _AckModel:
    def __init__(self, **values):
        self.__dict__.update(values)

    @classmethod
    def model_validate(cls, value):
        allowed = {"message_id", "reply"}
        unknown = set(value) - allowed
        if unknown or not value.get("message_id") or not value.get("reply"):
            raise _ValidationError()
        return cls(message_id=value["message_id"], reply=value["reply"])

    @classmethod
    def model_json_schema(cls):
        return {"type": "object"}


class _ValidationError(Exception):
    def errors(self):
        return [{"type": "extra_forbidden", "loc": ["unexpected"]}]


class _FakeSession:
    def __init__(self, _caller):
        self.calls = []

    def send_tool(self, to, body, requires_ack, acknowledges_message_id):
        self.calls.append((to, body, requires_ack, acknowledges_message_id))
        class _Result:
            message_id = "test"
            outcome = "sent"

            def to_json(self):
                return json.dumps(
                    {
                        "message_id": self.message_id,
                        "requires_ack": requires_ack,
                        "outcome": self.outcome,
                    }
                )

        return _Result()

    def ack_tool(self, message_id, reply):
        return self.send_tool(None, reply, False, message_id)


class _TypedToolError:
    code = "ATM_DAEMON_UNAVAILABLE"
    message = "the local daemon is unavailable"
    recovery = "start the local daemon and retry"
    layer = "native_client"


class _FakeAddress:
    def __init__(self, *_args):
        pass


class NativeToolsTests(unittest.TestCase):
    def setUp(self):
        self.original = native_tools.atm_graft
        self.session = None

        class FakeGraft:
            AtmSendRequest = _Model
            AtmReadRequest = _Model
            AtmListRequest = _Model
            AtmAckRequest = _AckModel
            AtmToolError = _TypedToolError
            PyAgentAddress = _FakeAddress

            def PyGraftSession(_self, caller):
                self.session = _FakeSession(caller)
                return self.session

        native_tools.atm_graft = FakeGraft()

    def tearDown(self):
        native_tools.atm_graft = self.original

    def test_send_validates_before_the_native_client_and_preserves_requires_ack(self):
        tools = native_tools.AtmNativeTools(
            identity=TEST_IDENTITY, team=TEST_TEAM, chat_id=TEST_CHAT_ID
        )
        recipient = "native-tool-recipient@test-team"
        result = json.loads(
            tools.atm_send({"to": recipient, "body": "hello", "requires_ack": True})
        )
        self.assertEqual(result["kind"], "success")
        self.assertEqual(result["result"]["message_id"], "test")
        self.assertEqual(self.session.calls, [(recipient, "hello", True, None)])

        rejected = json.loads(tools.atm_send({"to": recipient, "body": "hello", "unexpected": 1}))
        self.assertEqual(rejected["kind"], "error")
        self.assertEqual(rejected["error"]["layer"], "ingress_validation")
        self.assertEqual(len(self.session.calls), 1)

    def test_send_projects_rust_typed_error_without_exception_attribute_rebuild(self):
        tools = native_tools.AtmNativeTools(
            identity=TEST_IDENTITY, team=TEST_TEAM, chat_id=TEST_CHAT_ID
        )
        self.session.send_tool = lambda *_args: _TypedToolError()

        result = json.loads(
            tools.atm_send({"to": "native-tool-recipient@test-team", "body": "hello"})
        )

        self.assertEqual(
            result,
            {
                "kind": "error",
                "error": {
                    "code": "ATM_DAEMON_UNAVAILABLE",
                    "message": "the local daemon is unavailable",
                    "recovery": "start the local daemon and retry",
                    "layer": "native_client",
                },
            },
        )

    def test_send_forwards_optional_acknowledgement_id_without_a_destination(self):
        tools = native_tools.AtmNativeTools(
            identity=TEST_IDENTITY, team=TEST_TEAM, chat_id=TEST_CHAT_ID
        )

        result = json.loads(
            tools.atm_send(
                {"body": "received", "acknowledges_message_id": "01KZSSREKYM7G39237P0YQ3CW3"}
            )
        )

        self.assertEqual(result["kind"], "success")
        self.assertEqual(
            self.session.calls,
            [(None, "received", False, "01KZSSREKYM7G39237P0YQ3CW3")],
        )

    def test_ack_uses_the_canonical_acknowledgement_path(self):
        tools = native_tools.AtmNativeTools(
            identity=TEST_IDENTITY, team=TEST_TEAM, chat_id=TEST_CHAT_ID
        )
        result = json.loads(
            tools.atm_ack({"message_id": "01KZSSREKYM7G39237P0YQ3CW3", "reply": "received"})
        )
        self.assertEqual(result["kind"], "success")
        self.assertEqual(
            self.session.calls,
            [(None, "received", False, "01KZSSREKYM7G39237P0YQ3CW3")],
        )

    def test_registration_uses_public_plugin_context(self):
        calls = []

        class Context:
            def register_tool(self, **kwargs):
                calls.append(kwargs)

        native_tools.register_tools(
            Context(), identity=TEST_IDENTITY, team=TEST_TEAM, chat_id=TEST_CHAT_ID
        )
        self.assertEqual(
            [call["name"] for call in calls],
            ["atm_send", "atm_read", "atm_list", "atm_ack"],
        )
        self.assertTrue(all(call["toolset"] == "atm" for call in calls))
        for call in calls:
            outcome = call["handler"]({})
            self.assertIsInstance(outcome, str)
            self.assertEqual(json.loads(outcome)["kind"], "error")


if __name__ == "__main__":
    unittest.main()
