from __future__ import annotations

import sys
import types
import unittest

# The isolated adapter test does not need a compiled graft wheel.  The
# production package imports the real extension; this stub is replaced per
# test before any handler is exercised.
if "atm_graft" not in sys.modules:
    sys.modules["atm_graft"] = types.ModuleType("atm_graft")

from hermes_atm import native_tools


class _Model:
    def __init__(self, **values):
        self.__dict__.update(values)

    @classmethod
    def model_validate(cls, value):
        allowed = {"to", "body", "requires_ack"}
        unknown = set(value) - allowed
        if unknown:
            raise _ValidationError()
        return cls(
            to=value["to"],
            body=value["body"],
            requires_ack=value.get("requires_ack", False),
        )

    @classmethod
    def model_json_schema(cls):
        return {"type": "object"}


class _ValidationError(Exception):
    def errors(self):
        return [{"type": "extra_forbidden", "loc": ["unexpected"]}]


class _FakeSession:
    def __init__(self, _caller):
        self.calls = []

    def send_tool(self, to, body, requires_ack):
        self.calls.append((to, body, requires_ack))
        return types.SimpleNamespace(
            message_id="test", requires_ack=requires_ack, outcome="sent"
        )

    def reconnect(self):
        self.reconnect_calls = getattr(self, "reconnect_calls", 0) + 1


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
            AtmToolError = _TypedToolError
            PyAgentAddress = _FakeAddress

            def PyGraftSession(_self, caller):
                self.session = _FakeSession(caller)
                return self.session

        native_tools.atm_graft = FakeGraft()

    def tearDown(self):
        native_tools.atm_graft = self.original

    def test_send_validates_before_the_native_client_and_preserves_requires_ack(self):
        tools = native_tools.AtmNativeTools(identity="skillrx", team="hermes", chat_id="local")
        recipient = "native-tool-recipient@hermes"
        result = tools.atm_send({"to": recipient, "body": "hello", "requires_ack": True})
        self.assertEqual(result["kind"], "success")
        self.assertEqual(result["result"]["message_id"], "test")
        self.assertEqual(self.session.calls, [(recipient, "hello", True)])

        rejected = tools.atm_send({"to": recipient, "body": "hello", "unexpected": 1})
        self.assertEqual(rejected["kind"], "error")
        self.assertEqual(rejected["error"]["layer"], "ingress_validation")
        self.assertEqual(len(self.session.calls), 1)

    def test_send_projects_typed_error_with_safe_retry_once_recovery(self):
        tools = native_tools.AtmNativeTools(identity="skillrx", team="hermes", chat_id="local")
        self.session.send_tool = lambda *_args: _TypedToolError()

        result = tools.atm_send({"to": "native-tool-recipient@hermes", "body": "hello"})

        self.assertEqual(result["kind"], "error")
        self.assertEqual(result["error"]["code"], "ATM_DAEMON_UNAVAILABLE")
        self.assertEqual(result["error"]["message"], "the local daemon is unavailable")
        self.assertEqual(result["error"]["layer"], "native_client")
        self.assertIn("retry this send once", result["error"]["recovery"])
        self.assertIn("was not replayed", result["error"]["recovery"])
        self.assertEqual(self.session.reconnect_calls, 1)

    def test_send_refreshes_connection_without_replaying_an_ambiguous_write(self):
        tools = native_tools.AtmNativeTools(identity="skillrx", team="hermes", chat_id="local")
        self.session.send_tool = lambda *_args: _TypedToolError()

        result = tools.atm_send({"to": "native-tool-recipient@hermes", "body": "hello"})

        self.assertEqual(result["kind"], "error")
        self.assertEqual(self.session.reconnect_calls, 1)
        self.assertIn("was not replayed", result["error"]["recovery"])

    def test_read_retries_once_after_a_refreshed_connection(self):
        attempts = []

        def call():
            attempts.append("call")
            if len(attempts) == 1:
                return _TypedToolError()
            return types.SimpleNamespace(value="recovered")

        reconnects = []
        result = native_tools._invoke(
            call,
            lambda outcome: {"value": outcome.value},
            reconnect=lambda: reconnects.append("reconnected"),
            retry_after_reconnect=True,
        )

        self.assertEqual(result, {"kind": "success", "result": {"value": "recovered"}})
        self.assertEqual(attempts, ["call", "call"])
        self.assertEqual(reconnects, ["reconnected"])

    def test_registration_uses_public_plugin_context(self):
        calls = []

        class Context:
            def register_tool(self, **kwargs):
                calls.append(kwargs)

        native_tools.register_tools(Context(), identity="skillrx", team="hermes", chat_id="local")
        self.assertEqual([call["name"] for call in calls], ["atm_send", "atm_read", "atm_list"])
        self.assertTrue(all(call["toolset"] == "atm" for call in calls))


if __name__ == "__main__":
    unittest.main()
