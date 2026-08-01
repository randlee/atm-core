from __future__ import annotations

from pathlib import Path
import json
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.public_redaction import public_string
from scripts.public_redaction import public_value


class PublicRedactionTests(unittest.TestCase):
    def test_redacts_all_supported_ci_path_roots(self) -> None:
        fixture = json.loads((ROOT / ".just/fixtures/fuzz/redaction-paths.json").read_text())
        for value in fixture.values():
            redacted = public_string(value)
            self.assertEqual(redacted, "<redacted-path>", value)

    def test_drops_sensitive_keys_and_redacts_nested_free_text(self) -> None:
        value = public_value(
            {
                "peer_host": "runner.internal",
                "worktree_path": "/home/runner/work/atm-core",
                "message": "temporary output at /private/var/folders/zz/tmp/report",
            }
        )
        self.assertNotIn("peer_host", value)
        self.assertNotIn("worktree_path", value)
        self.assertEqual(value["message"], "temporary output at <redacted-path>")


if __name__ == "__main__":
    unittest.main()
