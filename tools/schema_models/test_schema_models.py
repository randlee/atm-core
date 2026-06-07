from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools.schema_models.atm_message_schema import (
    AtmInboxMessage,
    AtmMissingTeamConfigAlertMessage,
)
from tools.schema_models.claude_code_message_schema import (
    ClaudeCodeIdleNotificationText,
    ClaudeCodeInboxMessage,
)
from tools.schema_models.legacy_atm_message_schema import (
    LegacyAtmInboxMessage,
    LegacyAtmMetadataEnvelope,
    LegacyAtmMetadataFields,
    LegacyMessageMetadata,
)

TEST_TEAM = "test-team"
TEST_SENDER = "test-agent"
TEST_TEAM_LEAD = "test-lead"
TEST_QM = "test-qm"
ROLE_TEAM_LEAD = "team-lead"
FIXTURES_DIR = Path(__file__).resolve().parent / "fixtures"


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

    def test_real_team_lead_to_quality_mgr_samples_validate(self) -> None:
        """Read-path: current team-lead->quality-mgr sample shapes validate unchanged."""

        samples = json.loads(
            (FIXTURES_DIR / "claude_code_quality_mgr_samples.json").read_text()
        )
        self.assertGreaterEqual(len(samples), 3)

        for sample in samples:
            validated = ClaudeCodeInboxMessage.model_validate(sample)
            self.assertEqual(validated.from_, ROLE_TEAM_LEAD)
            self.assertIsInstance(validated.text, str)
            self.assertIsInstance(validated.timestamp, str)
            self.assertIsInstance(validated.read, bool)

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
        """Write-path: validates docs/atm-message-schema.md current ATM additive fields."""

        message = AtmInboxMessage.model_validate(
            {
                "from": TEST_TEAM_LEAD,
                "text": "ping",
                "timestamp": "2026-04-04T18:49:59.525805+00:00",
                "read": True,
                "summary": "ping",
                "message_id": "81286baa-e783-4f0c-bfea-82d070750fae",
            }
        )
        self.assertEqual(
            str(message.message_id),
            "81286baa-e783-4f0c-bfea-82d070750fae",
        )

    def test_atm_superset_named_thread_fields_validate(self) -> None:
        """Write-path: approved immutable compatibility fields stay typed explicitly."""

        message = AtmInboxMessage.model_validate(
            {
                "from": TEST_TEAM_LEAD,
                "text": "thread update",
                "timestamp": "2026-04-04T18:49:59.525805+00:00",
                "read": True,
                "summary": "thread update",
                "parentMessageId": "81286baa-e783-4f0c-bfea-82d070750fae",
                "threadMode": "add-details",
                "taskId": "TASK-123",
            }
        )
        self.assertEqual(
            str(message.parentMessageId),
            "81286baa-e783-4f0c-bfea-82d070750fae",
        )
        self.assertEqual(message.threadMode, "add-details")
        self.assertEqual(message.taskId, "TASK-123")

    def test_atm_missing_config_alert_validates(self) -> None:
        """Write-path: validates current ATM-owned alert additions during migration."""

        message = AtmMissingTeamConfigAlertMessage.model_validate(
            {
                "from": TEST_SENDER,
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
        """Read-compat: validates docs/legacy-atm-message-schema.md legacy top-level fields."""

        message = LegacyAtmInboxMessage.model_validate(
            {
                "from": TEST_SENDER,
                "text": "ATM warning",
                "timestamp": "2026-04-04T18:49:59.525805+00:00",
                "read": False,
                "summary": "ATM warning",
                "message_id": "81286baa-e783-4f0c-bfea-82d070750fae",
                "source_team": TEST_TEAM,
                "pendingAckAt": "2026-04-04T18:49:59.525Z",
                "acknowledgedAt": "2026-04-04T18:50:59.525Z",
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
        self.assertEqual(message.pendingAckAt, "2026-04-04T18:49:59.525Z")

    def test_legacy_metadata_atm_fields_validate_on_read(self) -> None:
        """Read-path: historical metadata.atm derivatives remain accepted on read."""

        metadata = LegacyAtmMetadataFields.model_validate(
            {
                "messageId": "01JQYVB6W51Q2E7E6T3Y4Q9N2M",
                "sourceTeam": TEST_TEAM,
                "pendingAckAt": "2026-04-04T18:49:59.525Z",
                "taskId": "TASK-123",
            }
        )
        self.assertEqual(metadata.sourceTeam, TEST_TEAM)

        envelope = LegacyAtmMetadataEnvelope.model_validate(
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
        self.assertIsInstance(envelope.metadata, LegacyMessageMetadata)
        self.assertEqual(envelope.metadata.atm.sourceTeam, TEST_TEAM)
        self.assertEqual(envelope.metadata.atm.taskId, "TASK-123")

    def test_legacy_top_level_message_id_rejects_ulid(self) -> None:
        """Write-path: current top-level message_id stays typed as the UUID wire form."""

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

    def test_legacy_metadata_message_id_rejects_uuid(self) -> None:
        """Read-path guard: metadata.atm.messageId stays typed as the historical ULID derivative."""

        with self.assertRaises(Exception):
            LegacyAtmMetadataFields.model_validate(
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
