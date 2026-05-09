from __future__ import annotations

from pathlib import Path
import sys
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from check_line_counts import FileCounts
from check_line_counts import LineLimitConfig
from check_line_counts import classify_lines
from check_line_counts import evaluate_limits
from check_line_counts import format_table
from check_line_counts import limit_summary


class CheckLineCountsTests(unittest.TestCase):
    def test_classify_lines_separates_cfg_test_block_lines(self) -> None:
        lines = [
            "pub fn production() {",
            '    println!(\"prod\");',
            "}",
            "",
            "#[cfg(test)]",
            "mod tests {",
            "    use super::*;",
            "",
            "    #[test]",
            "    fn example() {",
            "        production();",
            "    }",
            "}",
        ]

        production_lines, test_lines = classify_lines(lines)

        self.assertEqual(production_lines, 3)
        self.assertEqual(test_lines, 8)

    def test_classify_lines_counts_comments_inside_correct_scope(self) -> None:
        lines = [
            "// prod comment",
            "pub fn production() {}",
            "#[cfg(test)]",
            "mod tests {",
            "    // test comment",
            "    #[test]",
            "    fn example() {}",
            "}",
        ]

        production_lines, test_lines = classify_lines(lines)

        self.assertEqual(production_lines, 1)
        self.assertEqual(test_lines, 5)

    def test_classify_lines_treats_file_level_cfg_test_as_all_test(self) -> None:
        lines = [
            "#![cfg(test)]",
            "",
            "use super::*;",
            "",
            "fn helper() {}",
        ]

        production_lines, test_lines = classify_lines(lines)

        self.assertEqual(production_lines, 0)
        self.assertEqual(test_lines, 3)

    def test_evaluate_limits_uses_configured_metrics(self) -> None:
        counts = [
            FileCounts(
                crate_name="atm-core",
                crate_root="crates/atm-core",
                path="crates/atm-core/src/example.rs",
                total_lines=1200,
                production_lines=1001,
                test_lines=20,
            )
        ]
        config = LineLimitConfig(
            max_total_lines=1100,
            max_production_lines=1000,
            max_scoped_code_lines=1010,
        )

        failures = evaluate_limits(counts, config)

        self.assertEqual(len(failures), 3)
        self.assertIn("total=1200 exceeds limit 1100", failures[0])
        self.assertIn("prod=1001 exceeds limit 1000", failures[1])
        self.assertIn("prod+test=1021 exceeds limit 1010", failures[2])

    def test_format_table_includes_crate_totals(self) -> None:
        counts = [
            FileCounts(
                crate_name="atm-core",
                crate_root="crates/atm-core",
                path="crates/atm-core/src/a.rs",
                total_lines=10,
                production_lines=8,
                test_lines=1,
            ),
            FileCounts(
                crate_name="atm-core",
                crate_root="crates/atm-core",
                path="crates/atm-core/src/b.rs",
                total_lines=20,
                production_lines=15,
                test_lines=3,
            ),
            FileCounts(
                crate_name="atm",
                crate_root="crates/atm",
                path="crates/atm/src/main.rs",
                total_lines=5,
                production_lines=4,
                test_lines=0,
            ),
        ]

        table = format_table(counts)
        joined = "\n".join(table)

        self.assertIn("crate", table[0])
        self.assertIn("prod+test", table[0])
        self.assertIn("atm-core", joined)
        self.assertIn("src/a.rs", joined)
        self.assertIn("src/b.rs", joined)
        self.assertIn("TOTAL", joined)
        self.assertIn("30", joined)
        self.assertIn("23", joined)
        self.assertIn("4", joined)

    def test_limit_summary_only_lists_enabled_limits(self) -> None:
        config = LineLimitConfig(
            max_total_lines=None,
            max_production_lines=1000,
            max_scoped_code_lines=1200,
        )

        self.assertEqual(limit_summary(config), "prod<=1000, prod+test<=1200")


if __name__ == "__main__":
    unittest.main()
