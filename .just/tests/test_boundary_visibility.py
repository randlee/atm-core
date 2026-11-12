"""Check visibility of the six SQLite store boundary records.

SCB-BOUNDARY-005 supersedes the repo-local public type and constructor checks
removed in f9716f84f by checking each record's declared visibility. Rules 001
and 002 apply to boundary.internal_only items; these six stores are not marked
internal_only, so those rules do not apply to them.
"""

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
    def test_relaxed_store_types_and_constructors_cannot_be_widened(self) -> None:
        binary = os.environ.get("SC_LINT_BIN", "sc-lint")
        records = (
            ("async-message-store", "message_store", "SqliteMessageStore"),
            ("mail-store", "message_store", "SqliteMessageStore"),
            ("nudge-template-override-store", "nudge_template_override_store", "SqliteNudgeTemplateOverrideStore"),
            ("pending-nudge-store", "pending_nudge_store", "SqlitePendingNudgeStore"),
            ("roster-store", "roster_store", "SqliteRosterStore"),
            ("task-store", "task_store", "SqliteTaskStore"),
        )
        for record_name, module, type_name in records:
            with self.subTest(record=record_name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / "src").mkdir()
                record_dir = root / "boundaries/atm-storage-rusqlite"
                record_dir.mkdir(parents=True)
                (root / "Cargo.toml").write_text(
                    '[workspace]\n[workspace.package]\nversion="0.1.0"\n'
                    '[package]\nname="atm-storage-rusqlite"\nversion="0.1.0"\nedition="2021"\n',
                    encoding="utf-8",
                )
                (root / "src/lib.rs").write_text(f"mod {module};\n", encoding="utf-8")
                source = (ROOT / f"crates/atm-storage-rusqlite/src/{module}.rs").read_text(encoding="utf-8")
                record = (ROOT / f"boundaries/atm-storage-rusqlite/{record_name}-sqlite.toml").read_text(encoding="utf-8")
                record = re.sub(
                    r"(?ms)^\[dependencies\].*?(?=^\[references\])",
                    "[dependencies]\nallowed_dependents = []\nallowed_dependencies = []\nforbidden_edges = []\n\n",
                    record,
                )
                (record_dir / f"{record_name}-sqlite.toml").write_text(record, encoding="utf-8")
                shutil.copy(ROOT / "boundaries/planning.toml", root / "boundaries/planning.toml")
                (root / "sc-lint.toml").write_text('[analyzers.boundary]\nfail_rules=["SCB-BOUNDARY-005"]\n', encoding="utf-8")

                def run(text: str) -> subprocess.CompletedProcess[str]:
                    (root / f"src/{module}.rs").write_text(text, encoding="utf-8")
                    return subprocess.run(
                        [binary, "lint", "sc-boundary", "--root", str(root), "--json", "--rule-filter", "SCB-BOUNDARY-005"],
                        capture_output=True, text=True, timeout=30, check=False,
                    )

                baseline = run(source)
                self.assertEqual(baseline.returncode, 0, baseline.stderr or baseline.stdout)
                for original, replacement, expected in (
                    (f"pub(crate) struct {type_name}", f"pub struct {type_name}", "type `"),
                    ("pub(crate) fn new(", "pub fn new(", "constructor"),
                ):
                    with self.subTest(widened=expected):
                        self.assertIn(original, source)
                        rejected = run(source.replace(original, replacement, 1))
                        self.assertEqual(rejected.returncode, 10, rejected.stderr or rejected.stdout)
                        findings = json.loads(rejected.stdout)["data"]["findings"]
                        mismatches = [f for f in findings if f["kind"] == "declared_visibility_mismatch"]
                        self.assertTrue(any(expected in f["message"] for f in mismatches), mismatches)


if __name__ == "__main__":
    unittest.main()
