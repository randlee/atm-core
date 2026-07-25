#!/usr/bin/env python3
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from analyze_logs import analyze_log_text


class AnalyzeLogsTests(unittest.TestCase):
    def test_persistence_without_peer_confirmation_is_not_receiver_proof(self) -> None:
        result = analyze_log_text(
            '{"level":"info","fields":{"outcome":"write_persisted"}}',
            [],
            require_peer_confirmation=True,
        )

        self.assertFalse(result.passed)
        self.assertIn("peer_delivery_confirmed", result.missing_events)
        self.assertTrue(result.error_records)

    def test_peer_confirmation_satisfies_receiver_proof_requirement(self) -> None:
        result = analyze_log_text(
            "\n".join(
                [
                    '{"level":"info","fields":{"outcome":"write_persisted"}}',
                    '{"level":"info","fields":{"outcome":"peer_delivery_confirmed"}}',
                ]
            ),
            [],
            require_peer_confirmation=True,
        )

        self.assertTrue(result.passed)


if __name__ == "__main__":
    unittest.main()
