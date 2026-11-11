from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class BoundaryVisibilityTest(unittest.TestCase):
    def test_roster_store_and_constructor_cannot_be_widened(self) -> None:
        binary = os.environ.get("SC_LINT_BIN", "sc-lint")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "src").mkdir()
            record_dir = root / "boundaries/atm-storage-rusqlite"
            record_dir.mkdir(parents=True)
            (root / "Cargo.toml").write_text(
                '[workspace]\n[workspace.package]\nversion="0.1.0"\n'
                '[package]\nname="atm-storage-rusqlite"\nversion="0.1.0"\nedition="2021"\n',
                encoding="utf-8",
            )
            (root / "src/lib.rs").write_text("mod roster_store;\n", encoding="utf-8")
            source = (ROOT / "crates/atm-storage-rusqlite/src/roster_store.rs").read_text(encoding="utf-8")
            record = (ROOT / "boundaries/atm-storage-rusqlite/roster-store-sqlite.toml").read_text(encoding="utf-8")
            record = re.sub(
                r"(?ms)^\[dependencies\].*?(?=^\[references\])",
                "[dependencies]\nallowed_dependents = []\nallowed_dependencies = []\nforbidden_edges = []\n\n",
                record,
            )
            (record_dir / "roster-store-sqlite.toml").write_text(record, encoding="utf-8")
            shutil.copy(ROOT / "boundaries/planning.toml", root / "boundaries/planning.toml")
            (root / "sc-lint.toml").write_text('[analyzers.boundary]\nfail_rules=["SCB-BOUNDARY-005"]\n', encoding="utf-8")

            def run(text: str) -> subprocess.CompletedProcess[str]:
                (root / "src/roster_store.rs").write_text(text, encoding="utf-8")
                return subprocess.run(
                    [binary, "lint", "sc-boundary", "--root", str(root), "--json", "--rule-filter", "SCB-BOUNDARY-005"],
                    capture_output=True, text=True, timeout=30, check=False,
                )

            baseline = run(source)
            self.assertEqual(baseline.returncode, 0, baseline.stderr or baseline.stdout)
            self.assertIn("pub(crate) struct SqliteRosterStore", source)
            self.assertIn("pub(crate) fn new(", source)
            widened = source.replace("pub(crate) struct SqliteRosterStore", "pub struct SqliteRosterStore", 1)
            widened = widened.replace("pub(crate) fn new(", "pub fn new(", 1)
            rejected = run(widened)
            self.assertEqual(rejected.returncode, 10, rejected.stderr or rejected.stdout)
            findings = json.loads(rejected.stdout)["data"]["findings"]
            mismatches = [f for f in findings if f["kind"] == "declared_visibility_mismatch"]
            self.assertTrue(any("constructor" in f["message"] for f in mismatches), mismatches)
            self.assertTrue(any("type `" in f["message"] for f in mismatches), mismatches)


if __name__ == "__main__":
    unittest.main()
