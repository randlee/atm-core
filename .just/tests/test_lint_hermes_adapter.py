from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_hermes_adapter import collect_violations


class HermesAdapterLintTests(unittest.TestCase):
    def write_adapter_source(self, root: Path, text: str) -> None:
        source_dir = root / "crates/atm-graft-python/python/atm_graft_hermes_adapter"
        source_dir.mkdir(parents=True)
        (source_dir / "__init__.py").write_text(text, encoding="utf-8")

    def test_clean_steer_adapter_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            self.write_adapter_source(root, "async def steer(text):\n    return text\n")

            self.assertEqual(collect_violations(root), [])

    def test_retired_normal_ingress_symbols_are_reported_with_locations(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            self.write_adapter_source(
                root,
                "from gateway import MessageEvent\n\n    return MessageEvent(internal=False)\n",
            )

            violations = collect_violations(root)

            self.assertEqual(
                [(item.symbol, item.line_number) for item in violations],
                [("MessageEvent", 1), ("MessageEvent", 3), ("internal=False", 3)],
            )
            self.assertIn("crates/atm-graft-python/python/atm_graft_hermes_adapter/__init__.py:3", violations[-1].render())


if __name__ == "__main__":
    unittest.main()
