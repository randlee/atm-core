from __future__ import annotations

from pathlib import Path
import json
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from view_lines import run


class ViewLinesTests(unittest.TestCase):
    def test_run_writes_line_inventory_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            repo_root = Path(tempdir)
            (repo_root / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/atm-core"]\nresolver = "2"\n', encoding="utf-8")
            (repo_root / ".just").mkdir()
            (repo_root / ".just/lint-config.toml").write_text("[line_counts]\nmax_production_lines = 1000\n", encoding="utf-8")
            crate_dir = repo_root / "crates" / "atm-core" / "src"
            crate_dir.mkdir(parents=True)
            (repo_root / "crates/atm-core/Cargo.toml").write_text('[package]\nname = "agent-team-mail-core"\nversion = "1.1.2"\n', encoding="utf-8")
            (crate_dir / "lib.rs").write_text("pub fn example() {}\n", encoding="utf-8")
            self.assertEqual(run(repo_root), 0)
            summary = json.loads((repo_root / "artifacts/view/lines/summary.json").read_text(encoding="utf-8"))
            self.assertEqual(len(summary["crate_totals"]), 1)
            self.assertEqual(summary["crate_totals"][0]["crate"], "atm-core")
