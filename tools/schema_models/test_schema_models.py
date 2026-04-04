from __future__ import annotations

import json
import unittest

from tools.schema_models.atm_message_schema import (
    AtmInboxMessage,
    AtmMetadataEnvelope,
    AtmMetadataFields,
    AtmMissingTeamConfigAlertMessage,
    MessageMetadata,
)
from tools.schema_models.claude_code_message_schema import (
    ClaudeCodeIdleNotificationText,
    ClaudeCodeInboxMessage,
)
from tools.schema_models.legacy_atm_message_schema import LegacyAtmInboxMessage


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

    def test_legacy_atm_top_level_alert_fields_validate(self) -> None:
        message = LegacyAtmInboxMessage.model_validate(
            {
                "from": "arch-ctm",
                "text": "ATM warning",
                "timestamp": "2026-04-04T18:49:59.525805+00:00",
                "read": False,
                "summary": "ATM warning",
                "message_id": "81286baa-e783-4f0c-bfea-82d070750fae",
                "source_team": "atm-dev",
                "atmAlertKind": "missing_team_config",
                "missingConfigPath": "/Users/randlee/.claude/teams/atm-dev/config.json",
            }
        )
        self.assertEqual(message.source_team, "atm-dev")

    def test_forward_atm_metadata_fields_validate(self) -> None:
        metadata = AtmMetadataFields.model_validate(
            {
                "messageId": "01JQYVB6W51Q2E7E6T3Y4Q9N2M",
                "sourceTeam": "atm-dev",
                "pendingAckAt": "2026-04-04T18:49:59.525Z",
            }
        )
        self.assertEqual(metadata.sourceTeam, "atm-dev")

        envelope = AtmMetadataEnvelope.model_validate(
            {
                "from": "team-lead",
                "text": "ping",
                "timestamp": "2026-04-04T18:49:59.525Z",
                "read": True,
                "summary": "ping",
                "metadata": {
                    "atm": {
                        "messageId": "01JQYVB6W51Q2E7E6T3Y4Q9N2M",
                        "sourceTeam": "atm-dev",
                    }
                },
            }
        )
        self.assertIsInstance(envelope.metadata, MessageMetadata)
        self.assertEqual(envelope.metadata.atm.sourceTeam, "atm-dev")


if __name__ == "__main__":
    unittest.main()
