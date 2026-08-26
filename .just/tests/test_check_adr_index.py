from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_adr_index import find_violations


class AdrIndexTests(unittest.TestCase):
    def write_index(self, root: Path, entries: list[str]) -> None:
        adr_root = root / "docs/adr"
        adr_root.mkdir(parents=True)
        for entry in entries:
            filename = entry.partition("./")[2].rstrip(")")
            (adr_root / filename).write_text("# ADR\n", encoding="utf-8")
        (adr_root / "INDEX.md").write_text("\n".join(entries), encoding="utf-8")

    def test_accepts_unique_numbered_entries(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_index(
                root,
                [
                    "- [ADR-053 — Overlay](./ADR-053-overlay.md)",
                    "- [ADR-057 — Peer Writes](./ADR-057-peer-writes.md)",
                ],
            )
            self.assertEqual(find_violations(root), [])

    def test_rejects_duplicate_adr_number(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_index(
                root,
                [
                    "- [ADR-053 — First](./ADR-053-first.md)",
                    "- [ADR-053 — Second](./ADR-053-second.md)",
                ],
            )
            self.assertIn("duplicate ADR-053", "\n".join(find_violations(root)))

    def test_rejects_index_filename_number_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.write_index(
                root,
                ["- [ADR-057 — Peer Writes](./ADR-053-peer-writes.md)"],
            )
            self.assertIn("does not match filename", "\n".join(find_violations(root)))


if __name__ == "__main__":
    unittest.main()
