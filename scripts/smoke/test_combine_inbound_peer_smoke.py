"""Unit tests for strict cross-host XHTML pane validation."""
from __future__ import annotations

from datetime import datetime, timedelta, timezone
import importlib.util
import tempfile
import unittest
from pathlib import Path


def load_combiner():
    path = Path(__file__).with_name("combine_inbound_peer_smoke.py")
    spec = importlib.util.spec_from_file_location("combine_inbound_peer_smoke", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


COMBINER = load_combiner()


def pane(host: str, generated_at: datetime) -> str:
    return (
        '<html><head><meta name="smoke-host" content="'
        + host
        + '" /><meta name="smoke-generated-at" content="'
        + generated_at.isoformat().replace("+00:00", "Z")
        + '" /></head><body><p>evidence</p></body></html>'
    )


class CombineInboundPeerSmokeTests(unittest.TestCase):
    def test_rejects_missing_pane(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "absent"):
                COMBINER.load_current_pane(Path(directory) / "m5.xhtml", "m5", 30)

    def test_rejects_wrong_host_label(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "m5.xhtml"
            path.write_text(pane("other", datetime.now(timezone.utc)), encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "not labelled"):
                COMBINER.load_current_pane(path, "m5", 30)

    def test_rejects_stale_or_malformed_pane(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "m5.xhtml"
            path.write_text(pane("m5", datetime.now(timezone.utc) - timedelta(minutes=31)), encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "outdated"):
                COMBINER.load_current_pane(path, "m5", 30)
            path.write_text(
                '<html><head><meta name="smoke-host" content="m5" /></head><body>missing metadata</body></html>',
                encoding="utf-8",
            )
            with self.assertRaisesRegex(RuntimeError, "missing required"):
                COMBINER.load_current_pane(path, "m5", 30)

    def test_accepts_current_well_formed_pane(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "m5.xhtml"
            path.write_text(pane("m5", datetime.now(timezone.utc)), encoding="utf-8")
            loaded = COMBINER.load_current_pane(path, "m5", 30)
            self.assertIn("<h2>m5</h2>", loaded)


if __name__ == "__main__":
    unittest.main()
