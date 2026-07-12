from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import tempfile
import textwrap
import unittest


REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT_PATH = REPO_ROOT / "scripts" / "verify_user_docs.py"
SPEC = importlib.util.spec_from_file_location("verify_user_docs", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class VerifyUserDocsTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tempdir = tempfile.TemporaryDirectory()
        self.root = Path(self.tempdir.name)

    def tearDown(self) -> None:
        self.tempdir.cleanup()

    def write_doc(self, relative_path: str, body: str) -> Path:
        path = self.root / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(body).strip() + "\n", encoding="utf-8")
        return path

    def test_extract_fenced_blocks_returns_language_and_ordinal(self) -> None:
        blocks = MODULE.extract_fenced_blocks(
            "<!-- doc-path: README.md -->\n"
            "```json\n{}\n```\n"
            "```bash\necho hi\n```\n"
        )

        self.assertEqual(len(blocks), 2)
        self.assertEqual(blocks[0].language, "json")
        self.assertEqual(blocks[0].ordinal, 1)
        self.assertEqual(blocks[1].language, "bash")
        self.assertEqual(blocks[1].ordinal, 2)

    def test_validate_relative_links_accepts_nested_relative_targets(self) -> None:
        self.write_doc(
            "README.md",
            """
            # Docs

            [Mailbox](guides/mailbox.md)
            """,
        )
        self.write_doc(
            "guides/mailbox.md",
            """
            # Mailbox

            [Back](../README.md)
            """,
        )

        self.assertEqual(MODULE.validate_relative_links(self.root), [])

    def test_validate_relative_links_rejects_absolute_and_missing_targets(self) -> None:
        self.write_doc(
            "README.md",
            """
            # Docs

            [Bad absolute](https://example.com)
            [Missing](missing.md)
            """,
        )

        errors = MODULE.validate_relative_links(self.root)

        self.assertEqual(len(errors), 2)
        self.assertTrue(any("must stay relative" in error for error in errors))
        self.assertTrue(any("broken relative link target" in error for error in errors))

    def test_verify_installed_copy_reports_missing_files(self) -> None:
        source = self.root / "source"
        installed = self.root / "installed"
        (source / "examples").mkdir(parents=True)
        installed.mkdir(parents=True)
        (source / "README.md").write_text("# Root\n", encoding="utf-8")
        (source / "examples/demo.json").write_text("{}\n", encoding="utf-8")
        (installed / "README.md").write_text("# Root\n", encoding="utf-8")

        errors = MODULE.verify_installed_copy(source, installed)

        self.assertEqual(errors, ["installed copy missing file `examples/demo.json`"])

    def test_validate_reviewed_for_release_requires_matching_version(self) -> None:
        self.write_doc(
            "README.md",
            """
            ---
            reviewed_for_release: 1.2.3
            ---

            # Docs
            """,
        )

        errors = MODULE.validate_reviewed_for_release(self.root, "1.3.0")

        self.assertEqual(
            errors,
            ["README.md: reviewed_for_release is 1.2.3, expected 1.3.0"],
        )


if __name__ == "__main__":
    unittest.main()
