from __future__ import annotations

import sys
import unittest
from pathlib import Path

JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from atm_lint_test_scope import is_rust_test_cfg_attribute
from atm_lint_test_scope import rust_file_test_scope


class AtmLintTestScopeTests(unittest.TestCase):
    def test_test_utils_cfg_marks_only_its_item_and_body(self) -> None:
        lines = [
            "pub fn production() {}",
            '#[cfg(any(test, feature = "test-support"))]',
            'pub const TEST_HELPER: &str = "fixture";',
            "pub fn still_production() {}",
        ]
        self.assertEqual(rust_file_test_scope(Path("crates/x/src/lib.rs"), lines), [False, True, True, False])

    def test_negated_test_cfg_is_not_a_test_scope(self) -> None:
        self.assertFalse(is_rust_test_cfg_attribute("#[cfg(not(test))]"))
        self.assertFalse(is_rust_test_cfg_attribute('#[cfg(feature = "contest")]'))
        self.assertTrue(is_rust_test_cfg_attribute('#[cfg(any(test, feature = "test-support"))]'))
