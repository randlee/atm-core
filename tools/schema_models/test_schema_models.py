from __future__ import annotations

import json
import os
import tempfile
import unittest
from unittest.mock import patch

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

TEST_TEAM = "test-team"
TEST_SENDER = "test-agent"
TEST_TEAM_LEAD = "test-lead"
TEST_QM = "test-qm"


class SchemaModelTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        super().setUpClass()
        cls._temp_home = tempfile.TemporaryDirectory()
        cls._env_patch = patch.dict(
            os.environ,
            {
                "HOME": cls._temp_home.name,
                "USERPROFILE": cls._temp_home.name,
                "TMPDIR": cls._temp_home.name,
                "TMP": cls._temp_home.name,
                "TEMP": cls._temp_home.name,
            },
            clear=False,
        )
        cls._env_patch.start()

    @classmethod
    def tearDownClass(cls) -> None:
        cls._env_patch.stop()
        cls._temp_home.cleanup()
        super().tearDownClass()

    def test_claude_native_message_validates(self) -> None:
        """Write-path: validates docs/claude-code-message-schema.md native envelope rules."""

        message = ClaudeCodeInboxMessage.model_validate(
            {
                "from": TEST_TEAM_LEAD,
                "text": "ping",
                "timestamp": "2026-04-04T18:50:03.331Z",
                "read": True,
                "summary": "ping",
                "color": "#00ff88",
            }
        )
        self.assertEqual(message.from_, TEST_TEAM_LEAD)
        self.assertEqual(message.color, "#00ff88")

    def test_claude_native_idle_payload_validates(self) -> None:
        """Write-path: validates docs/claude-code-message-schema.md idle payload rules."""

        payload = ClaudeCodeIdleNotificationText.model_validate_json(
            json.dumps(
                {
                    "type": "idle_notification",
                    "from": TEST_QM,
                    "timestamp": "2026-04-04T18:50:03.331Z",
                    "idleReason": "available",
                }
            )
        )
        self.assertEqual(payload.type, "idle_notification")

    def test_atm_superset_message_validates(self) -> None:
        """Write-path: validates docs/atm-message-schema.md legacy top-level ATM fields."""

        message = AtmInboxMessage.model_validate(
            {
                "from": TEST_TEAM_LEAD,
                "source_team": TEST_TEAM,
                "text": "ping",
                "timestamp": "2026-04-04T18:49:59.525805+00:00",
                "read": True,
                "summary": "ping",
                "message_id": "81286baa-e783-4f0c-bfea-82d070750fae",
            }
        )
        self.assertEqual(message.source_team, TEST_TEAM)
        self.assertEqual(
            str(message.message_id),
            "81286baa-e783-4f0c-bfea-82d070750fae",
        )

    def test_atm_missing_config_alert_validates(self) -> None:
        """Write-path: validates current ATM-owned alert additions during migration."""

        message = AtmMissingTeamConfigAlertMessage.model_validate(
            {
                "from": TEST_SENDER,
                "source_team": TEST_TEAM,
                "text": "ATM warning: send used existing inbox fallback.",
                "timestamp": "2026-04-04T18:49:59.525805+00:00",
                "read": False,
                "summary": "ATM warning",
                "message_id": "81286baa-e783-4f0c-bfea-82d070750fae",
                "atmAlertKind": "missing_team_config",
                "missingConfigPath": os.path.join(
                    self._temp_home.name,
                    ".claude",
                    "teams",
                    TEST_TEAM,
                    "config.json",
                ),
            }
        )
        self.assertEqual(message.atmAlertKind, "missing_team_config")

    def test_legacy_atm_top_level_alert_fields_validate(self) -> None:
        """Write-path: validates docs/legacy-atm-message-schema.md read compatibility."""

        message = LegacyAtmInboxMessage.model_validate(
            {
                "from": TEST_SENDER,
                "text": "ATM warning",
                "timestamp": "2026-04-04T18:49:59.525805+00:00",
                "read": False,
                "summary": "ATM warning",
                "message_id": "81286baa-e783-4f0c-bfea-82d070750fae",
                "source_team": TEST_TEAM,
                "atmAlertKind": "missing_team_config",
                "missingConfigPath": os.path.join(
                    self._temp_home.name,
                    ".claude",
                    "teams",
                    TEST_TEAM,
                    "config.json",
                ),
            }
        )
        self.assertEqual(message.source_team, TEST_TEAM)

    def test_forward_atm_metadata_fields_validate(self) -> None:
        """Write-path: validates docs/atm-message-schema.md forward metadata.atm rules."""

        metadata = AtmMetadataFields.model_validate(
            {
                "messageId": "01JQYVB6W51Q2E7E6T3Y4Q9N2M",
                "sourceTeam": TEST_TEAM,
                "pendingAckAt": "2026-04-04T18:49:59.525Z",
                "taskId": "TASK-123",
            }
        )
        self.assertEqual(metadata.sourceTeam, TEST_TEAM)

        envelope = AtmMetadataEnvelope.model_validate(
            {
                "from": TEST_TEAM_LEAD,
                "text": "ping",
                "timestamp": "2026-04-04T18:49:59.525Z",
                "read": True,
                "summary": "ping",
                "metadata": {
                    "atm": {
                        "messageId": "01JQYVB6W51Q2E7E6T3Y4Q9N2M",
                        "sourceTeam": TEST_TEAM,
                        "taskId": "TASK-123",
                    }
                },
            }
        )
        self.assertIsInstance(envelope.metadata, MessageMetadata)
        self.assertEqual(envelope.metadata.atm.sourceTeam, TEST_TEAM)
        self.assertEqual(envelope.metadata.atm.taskId, "TASK-123")

    def test_forward_metadata_rejects_top_level_atm_machine_fields(self) -> None:
        """Write-path: top-level ATM machine fields are forbidden on forward inbox writes."""

        with self.assertRaises(Exception):
            AtmMetadataEnvelope.model_validate(
                {
                    "from": TEST_TEAM_LEAD,
                    "text": "ping",
                    "timestamp": "2026-04-04T18:49:59.525Z",
                    "read": True,
                    "summary": "ping",
                    "message_id": "81286baa-e783-4f0c-bfea-82d070750fae",
                    "metadata": {
                        "atm": {
                            "sourceTeam": TEST_TEAM,
                        }
                    },
                }
            )

    def test_legacy_top_level_message_id_rejects_ulid(self) -> None:
        """Write-path: guards docs/atm-message-schema.md legacy top-level UUID placement."""

        with self.assertRaises(Exception):
            AtmInboxMessage.model_validate(
                {
                    "from": TEST_TEAM_LEAD,
                    "text": "ping",
                    "timestamp": "2026-04-04T18:49:59.525805+00:00",
                    "read": True,
                    "message_id": "01JQYVB6W51Q2E7E6T3Y4Q9N2M",
                }
            )

    def test_forward_metadata_message_id_rejects_uuid(self) -> None:
        """Write-path: guards docs/atm-message-schema.md forward metadata.atm ULID placement."""

        with self.assertRaises(Exception):
            AtmMetadataFields.model_validate(
                {
                    "messageId": "81286baa-e783-4f0c-bfea-82d070750fae",
                }
            )

    def test_read_path_malformed_atm_field_warns_and_degrades(self) -> None:
        """Read-path: malformed ATM-owned fields warn and degrade without dropping the message."""

        raw_message = {
            "from": TEST_TEAM_LEAD,
            "text": "ping",
            "timestamp": "2026-04-04T18:49:59.525805+00:00",
            "read": True,
            "summary": "ping",
            "message_id": "01JQYVB6W51Q2E7E6T3Y4Q9N2M",
        }

        warnings: list[str] = []

        try:
            AtmInboxMessage.model_validate(raw_message)
            self.fail("write-path validator should reject ULID in legacy top-level message_id")
        except Exception as exc:
            warnings.append(f"format warning: {exc}")

        degraded_message = dict(raw_message)
        degraded_message.pop("message_id", None)
        recovered = ClaudeCodeInboxMessage.model_validate(degraded_message)

        self.assertTrue(warnings)
        self.assertEqual(recovered.from_, TEST_TEAM_LEAD)
        self.assertEqual(recovered.text, "ping")


if __name__ == "__main__":
    unittest.main()
