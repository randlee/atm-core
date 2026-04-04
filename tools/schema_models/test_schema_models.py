from __future__ import annotations

import json
import unittest

from tools.schema_models.atm_message_schema import (
    AtmInboxMessage,
    AtmMissingTeamConfigAlertMessage,
)
from tools.schema_models.claude_code_message_schema import (
    ClaudeCodeIdleNotificationText,
    ClaudeCodeInboxMessage,
)


class SchemaModelTests(unittest.TestCase):
    def test_claude_native_message_validates(self) -> None:
        message = ClaudeCodeInboxMessage.model_validate(
            {
                "from": "team-lead",
                "text": "ping",
                "timestamp": "2026-04-04T18:50:03.331Z",
                "read": True,
                "summary": "ping",
                "color": "#00ff88",
            }
        )
        self.assertEqual(message.from_, "team-lead")
        self.assertEqual(message.color, "#00ff88")

    def test_claude_native_idle_payload_validates(self) -> None:
        payload = ClaudeCodeIdleNotificationText.model_validate_json(
            json.dumps(
                {
                    "type": "idle_notification",
                    "from": "quality-mgr",
                    "timestamp": "2026-04-04T18:50:03.331Z",
                    "idleReason": "available",
                }
            )
        )
        self.assertEqual(payload.type, "idle_notification")

    def test_atm_superset_message_validates(self) -> None:
        message = AtmInboxMessage.model_validate(
            {
                "from": "team-lead",
                "source_team": "atm-dev",
                "text": "ping",
                "timestamp": "2026-04-04T18:49:59.525805+00:00",
                "read": True,
                "summary": "ping",
                "message_id": "81286baa-e783-4f0c-bfea-82d070750fae",
            }
        )
        self.assertEqual(message.source_team, "atm-dev")

    def test_atm_missing_config_alert_validates(self) -> None:
        message = AtmMissingTeamConfigAlertMessage.model_validate(
            {
                "from": "arch-ctm",
                "source_team": "atm-dev",
                "text": "ATM warning: send used existing inbox fallback.",
                "timestamp": "2026-04-04T18:49:59.525805+00:00",
                "read": False,
                "summary": "ATM warning",
                "message_id": "81286baa-e783-4f0c-bfea-82d070750fae",
                "atmAlertKind": "missing_team_config",
                "missingConfigPath": "/Users/randlee/.claude/teams/atm-dev/config.json",
            }
        )
        self.assertEqual(message.atmAlertKind, "missing_team_config")


if __name__ == "__main__":
    unittest.main()
