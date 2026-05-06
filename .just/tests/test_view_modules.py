from __future__ import annotations

from pathlib import Path
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from view_modules import analysis_target_args
from view_modules import graphviz_svg_command


class ViewModulesTests(unittest.TestCase):
    def test_analysis_target_args_prefers_lib(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            manifest_path = Path(tempdir) / "Cargo.toml"
            manifest_path.write_text(
                """\
[package]
name = "agent-team-mail-core"
version = "1.1.2"

[lib]
name = "atm_core"
""",
                encoding="utf-8",
            )

            self.assertEqual(analysis_target_args(manifest_path), ["--lib"])

    def test_analysis_target_args_uses_first_bin_name(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            manifest_path = Path(tempdir) / "Cargo.toml"
            manifest_path.write_text(
                """\
[package]
name = "agent-team-mail"
version = "1.1.2"

[[bin]]
name = "atm"
path = "src/main.rs"
""",
                encoding="utf-8",
            )

            self.assertEqual(analysis_target_args(manifest_path), ["--bin", "atm"])

    def test_graphviz_svg_command_renders_dot_to_svg(self) -> None:
        dot_path = Path("/tmp/report/dependencies.dot")
        svg_path = Path("/tmp/report/dependencies.svg")
        self.assertEqual(
            graphviz_svg_command(dot_path, svg_path),
            ["dot", "-Tsvg", str(dot_path), "-o", str(svg_path)],
        )


if __name__ == "__main__":
    unittest.main()
