"""Integration tests for the raw findings validator."""

import importlib.util
from pathlib import Path


SCRIPTS = Path(__file__).parent
PREFIX = (
    "@prefix triage: <urn:atm:triage:> .\n"
    "@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n"
)


def _validator():
    spec = importlib.util.spec_from_file_location(
        "validate_findings", SCRIPTS / "validate-findings.py"
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


def _structure() -> str:
    return PREFIX + (
        "triage:S1 a triage:Sprint ; triage:order 1 .\n"
    )


def test_reports_missing_fields_and_fails(tmp_path, capsys):
    validator = _validator()
    structure = tmp_path / "structure.ttl"
    structure.write_text(_structure())
    findings = tmp_path / "findings"
    findings.mkdir()
    (findings / "F-1.ttl").write_text(
        PREFIX
        + "triage:f1 a triage:Finding ; triage:findingId \"F-1\" .\n"
    )

    rc = validator.main(
        [
            "--findings-dir",
            str(findings),
            "--structure",
            str(structure),
            "--max-results",
            "2",
        ]
    )
    output = capsys.readouterr().out
    assert rc == 1
    assert "validated 1 file(s), 1 finding(s)" in output
    assert "#error:" in output
    assert "truncated" in output


def test_regex_limits_validation_scope(tmp_path, capsys):
    validator = _validator()
    findings = tmp_path / "findings"
    findings.mkdir()
    for name in ("AI21-001", "AI10-001"):
        (findings / f"{name}.ttl").write_text(
            PREFIX
            + f"<urn:atm:triage:finding/{name}> a triage:Finding ; "
            + f"triage:findingId \"{name}\" .\n"
        )

    rc = validator.main(
        [
            "--findings-dir",
            str(findings),
            "--finding-id-regex",
            r"^AI21-",
        ]
    )
    output = capsys.readouterr().out
    assert rc == 1
    assert "1 finding(s)" in output
    assert "AI21-001" in output
    assert "AI10-001" not in output
