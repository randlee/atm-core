from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"


class PublishKitSyncCiTests(unittest.TestCase):
    def test_ci_checks_the_vendored_publishing_skill_for_upstream_drift(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("repository: randlee/sc-compose", text)
        self.assertIn("feature/publish-kit-preflight-hardening", text)
        self.assertIn("scripts/sync_sc_compose_publish_kit.py", text)
        self.assertIn("--only skill", text)
        self.assertIn("--check", text)


if __name__ == "__main__":
    unittest.main()
