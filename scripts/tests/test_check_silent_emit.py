from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).parents[1] / "check-silent-emit.py"
SPEC = importlib.util.spec_from_file_location("check_silent_emit", SCRIPT)
assert SPEC and SPEC.loader
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


class SilentEmitGateTests(unittest.TestCase):
    def findings_for(self, source_text: str) -> list[str]:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            crate = root / "crates/example"
            source = crate / "src/example.rs"
            source.parent.mkdir(parents=True)
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["crates/*"]\n', encoding="utf-8"
            )
            (crate / "Cargo.toml").write_text(
                '[package]\nname = "example"\nversion = "0.1.0"\nedition = "2024"\n',
                encoding="utf-8",
            )
            source.write_text(source_text, encoding="utf-8")
            return GATE.collect_findings(root)

    def test_unrelated_discard_does_not_match_later_handled_emit(self) -> None:
        findings = self.findings_for(
            "fn example() {\n"
            "    let _ = sender.send(());\n"
            "    observability.emit(ev)?;\n"
            "}\n"
        )

        self.assertEqual(findings, [])

    def test_single_line_silent_emit_is_reported_at_its_line(self) -> None:
        findings = self.findings_for(
            "fn example() {\n"
            "    let _ = observability.emit(ev);\n"
            "}\n"
        )

        self.assertEqual(
            findings,
            [
                "crates/example/src/example.rs:2: silent observability emit discard; use emit_or_warn/emit_event_or_warn"
            ],
        )

    def test_multiline_silent_emit_is_still_reported(self) -> None:
        findings = self.findings_for(
            "fn example() {\n"
            "    let _ = observability\n"
            "        .emit(ev);\n"
            "}\n"
        )

        self.assertEqual(len(findings), 1)
        self.assertTrue(findings[0].startswith("crates/example/src/example.rs:2:"))

    def test_each_supported_emit_method_is_still_reported(self) -> None:
        findings = self.findings_for(
            "fn example() {\n"
            "    let _ = observability.emit(ev);\n"
            "    let _ = observability.emit_event(ev);\n"
            "    let _ = observability.emit_subsystem_event(ev);\n"
            "}\n"
        )

        self.assertEqual(len(findings), 3)
        self.assertEqual(
            [finding.split(":", 3)[1] for finding in findings],
            ["2", "3", "4"],
        )


if __name__ == "__main__":
    unittest.main()
