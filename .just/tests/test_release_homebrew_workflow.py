from __future__ import annotations

from pathlib import Path
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "homebrew-publish.yml"


class ReleaseHomebrewWorkflowTests(unittest.TestCase):
    def test_homebrew_channel_reads_its_configuration_from_the_manifest(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn("channel-config --manifest release/publish-artifacts.toml --channel homebrew", text)
        self.assertIn('ref: ${{ inputs.tag }}', text)

    def test_homebrew_channel_renders_every_manifest_selected_formula(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn('for index, formula in enumerate(channel["formulas"]):', text)
        self.assertIn('output = Path("homebrew-tap") / formula["path"]', text)
        self.assertNotIn("agent-team-mail.rb", text)
        self.assertNotIn("atm-beta.rb", text)

    def test_homebrew_channel_validates_every_rendered_formula(self) -> None:
        text = WORKFLOW.read_text(encoding="utf-8")

        self.assertIn('for formula in json.loads(os.environ["CHANNEL_CONFIG"])["channel"]["formulas"]:', text)
        self.assertIn('subprocess.run(["ruby", "-c", str(Path("homebrew-tap") / formula["path"])], check=True)', text)


if __name__ == "__main__":
    unittest.main()
