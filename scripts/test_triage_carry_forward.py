"""Unit tests for triage_carry_forward.py."""

from __future__ import annotations

import importlib.util
import io
import json
import tempfile
import textwrap
import unittest
from pathlib import Path
from unittest.mock import patch

_SCRIPT = Path(__file__).parent / "triage_carry_forward.py"
_SPEC = importlib.util.spec_from_file_location("triage_carry_forward", _SCRIPT)
_MOD = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_MOD)


def _write_record(path: Path) -> None:
    path.write_text(
        textwrap.dedent(
            """
            @prefix triage: <urn:atm:triage:> .
            @prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

            <urn:atm:triage:finding/FTQ-001>
              a triage:Finding ;
              triage:findingId "FTQ-001" ;
              triage:title "Process-global shutdown state in tests" ;
              triage:severity "important" ;
              triage:hasOccurrence <urn:atm:triage:occurrence/FTQ-001/R17/1> ;
              triage:hasOccurrence <urn:atm:triage:occurrence/FTQ-001/R16/1> .

            <urn:atm:triage:occurrence/FTQ-001/R17/1>
              a triage:Occurrence ;
              triage:file "crates/atm-daemon/src/tests.rs" ;
              triage:line 28 ;
              triage:status "open" ;
              triage:branch "R.17" .

            <urn:atm:triage:occurrence/FTQ-001/R16/1>
              a triage:Occurrence ;
              triage:file "crates/atm-daemon/src/tests.rs" ;
              triage:line 28 ;
              triage:status "fixed" ;
              triage:branch "R.16" .
            """
        ),
        encoding="utf-8",
    )


class TestTriageCarryForward(unittest.TestCase):
    def test_branch_filtered_payload(self):
        with tempfile.TemporaryDirectory() as tmp:
            ttl = Path(tmp) / "FTQ-001.ttl"
            _write_record(ttl)
            stdout = io.StringIO()
            with patch.object(_MOD, "require_oxigraph"), patch("sys.stdout", stdout):
                rc = _MOD.main(
                    [
                        "--branch",
                        "R.17",
                        "--ttl",
                        str(ttl),
                    ]
                )
        self.assertEqual(rc, 0)
        self.assertEqual(
            json.loads(stdout.getvalue()),
            [
                {
                    "id": "FTQ-001",
                    "severity": "important",
                    "file": "crates/atm-daemon/src/tests.rs",
                    "line": 28,
                    "summary": "Process-global shutdown state in tests",
                }
            ],
        )

    def test_missing_file_fails_closed(self):
        with self.assertRaises(SystemExit) as exc:
            _MOD.main(["--branch", "R.17", "--ttl", "/tmp/does-not-exist.ttl"])
        self.assertIn("missing triage record", str(exc.exception))


if __name__ == "__main__":
    unittest.main()
