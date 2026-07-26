"""Unit/integration tests for the raw findings validator.

The validator has three observable outcomes.  ``validation:pass`` and
``validation:fail`` are successful executions (the latter is a normal gate
failure caused by finding metadata); ``error`` means the validator itself
could not complete.
"""

import importlib.util
import json
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

    result = validator.run_validation(
        findings_dir=findings,
        structure=structure,
    )
    assert result.kind == "validation:fail"
    assert result.summary.errors == 2
    assert result.summary.warnings == 2
    assert all(line.startswith(("#error:", "#warning:")) for line in result.diagnostics)


def test_valid_finding_is_validation_pass(tmp_path):
    validator = _validator()
    structure = tmp_path / "structure.ttl"
    structure.write_text(_structure())
    findings = tmp_path / "findings"
    findings.mkdir()
    (findings / "F-1.ttl").write_text(
        PREFIX
        + (
            "triage:f1 a triage:Finding ; triage:findingId \"F-1\" ; "
            "triage:foundIn triage:S1 ; "
            "triage:foundAt \"2026-07-01T12:00:00Z\"^^xsd:dateTime ; "
            "triage:severity \"important\" ; triage:description \"Issue\" .\n"
        )
    )

    result = validator.run_validation(findings_dir=findings, structure=structure)
    assert result.kind == "validation:pass"
    assert result.summary == validator.ValidationSummary(files=1, findings=1)
    assert result.diagnostics == ()


def test_rejects_non_repository_relative_occurrence_and_legacy_worktree_paths(
    tmp_path,
):
    validator = _validator()
    structure = tmp_path / "structure.ttl"
    structure.write_text(_structure())
    findings = tmp_path / "findings"
    findings.mkdir()
    (findings / "F-1.ttl").write_text(
        PREFIX
        + (
            "triage:f1 a triage:Finding ; triage:findingId \"F-1\" ; "
            "triage:foundIn triage:S1 ; "
            "triage:foundAt \"2026-07-01T12:00:00Z\"^^xsd:dateTime ; "
            "triage:severity \"important\" ; triage:description \"Issue\" ; "
            "triage:hasOccurrence triage:o1 .\n"
            "triage:o1 a triage:Occurrence ; "
            "triage:file \"/checkout/src/lib.rs\" ; "
            "triage:occursIn triage:w1 .\n"
            "triage:w1 a triage:WorktreeSnapshot ; "
            "triage:path \"../feature-worktree\" .\n"
            "triage:w2 a triage:WorktreeSnapshot ; "
            "triage:path \"/abs/orphan-worktree\" .\n"
        )
    )

    result = validator.run_validation(findings_dir=findings, structure=structure)

    assert result.kind == "validation:fail"
    assert result.summary.errors == 3
    assert any("invalid triage:file" in line for line in result.diagnostics)
    assert sum("invalid triage:path" in line for line in result.diagnostics) == 2


def test_warning_only_metadata_is_validation_pass(tmp_path):
    validator = _validator()
    findings = tmp_path / "findings"
    findings.mkdir()
    (findings / "F-1.ttl").write_text(
        PREFIX
        + (
            "triage:f1 a triage:Finding ; triage:foundIn triage:S1 ; "
            "triage:foundAt \"2026-07-01T12:00:00Z\"^^xsd:dateTime .\n"
        )
    )

    result = validator.run_validation(findings_dir=findings)
    assert result.kind == "validation:pass"
    assert result.summary.errors == 0
    assert result.summary.warnings == 3
    assert all(line.startswith("#warning:") for line in result.diagnostics)


def test_malformed_turtle_is_error_result(tmp_path):
    validator = _validator()
    findings = tmp_path / "findings"
    findings.mkdir()
    (findings / "BROKEN.ttl").write_text("this is not valid Turtle [")

    result = validator.run_validation(findings_dir=findings)
    assert result.kind == "error"
    assert result.summary.errors == 1
    assert result.summary.findings == 0
    assert result.diagnostics[0].startswith("#error:")
    assert "malformed Turtle" in result.diagnostics[0]


def test_missing_directory_is_error_result(tmp_path):
    validator = _validator()
    result = validator.run_validation(findings_dir=tmp_path / "does-not-exist")
    assert result.kind == "error"
    assert result.summary.errors == 1
    assert "findings directory does not exist" in result.diagnostics[0]


def test_missing_structure_input_is_error_result(tmp_path):
    validator = _validator()
    findings = tmp_path / "findings"
    findings.mkdir()
    result = validator.run_validation(
        findings_dir=findings,
        structure=tmp_path / "missing-structure.ttl",
    )
    assert result.kind == "error"
    assert result.summary.errors == 1
    assert "input file does not exist" in result.diagnostics[0]


def test_declared_scope_rejects_finding_for_undeclared_sprint(tmp_path):
    validator = _validator()
    structure = tmp_path / "structure.ttl"
    structure.write_text(PREFIX + "triage:PhaseF a triage:Phase .\n")
    findings = tmp_path / "findings"
    findings.mkdir()
    (findings / "F-1.ttl").write_text(
        PREFIX
        + (
            "triage:f1 a triage:Finding ; triage:findingId \"F-1\" ; "
            "triage:foundIn triage:S1 ; "
            "triage:foundAt \"2026-07-01T12:00:00Z\"^^xsd:dateTime ; "
            "triage:severity \"important\" ; triage:description \"Issue\" .\n"
        )
    )

    result = validator.run_validation(findings_dir=findings, structure=structure)

    assert result.kind == "validation:fail"
    assert result.summary.errors == 1
    assert "undeclared sprint" in result.diagnostics[0]


def test_invalid_regex_is_error_result_and_cli_exit_two(tmp_path, capsys):
    validator = _validator()
    findings = tmp_path / "findings"
    findings.mkdir()

    result = validator.run_validation(
        findings_dir=findings,
        finding_id_regex="[unterminated",
    )
    assert result.kind == "error"
    assert "invalid validator configuration" in result.message

    rc = validator.main(
        [
            "--findings-dir",
            str(findings),
            "--finding-id-regex",
            "[unterminated",
            "--json",
        ]
    )
    assert rc == 2
    payload = json.loads(capsys.readouterr().out)
    assert payload["kind"] == "error"
    assert payload["diagnostics"] == []


def test_json_result_is_discriminated_and_preserves_validation_fail(tmp_path, capsys):
    validator = _validator()
    findings = tmp_path / "findings"
    findings.mkdir()
    (findings / "F-1.ttl").write_text(
        PREFIX + "triage:f1 a triage:Finding ; triage:findingId \"F-1\" .\n"
    )

    rc = validator.main(["--findings-dir", str(findings), "--json"])
    assert rc == 1
    payload = json.loads(capsys.readouterr().out)
    assert payload["kind"] == "validation:fail"
    assert payload["summary"]["errors"] == 2
    assert sum(item.startswith("#error:") for item in payload["diagnostics"]) == 2
    assert sum(item.startswith("#warning:") for item in payload["diagnostics"]) == 2


def test_broken_sparql_is_error_result(tmp_path):
    validator = _validator()
    findings = tmp_path / "findings"
    findings.mkdir()
    broken_scripts = tmp_path / "scripts"
    broken_scripts.mkdir()
    (broken_scripts / "validate-findings.sparql").write_text("SELECT definitely broken")

    result = validator.run_validation(
        findings_dir=findings,
        script_dir=broken_scripts,
    )
    assert result.kind == "error"
    assert "SPARQL query failed" in result.message


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


def test_path_validation_respects_finding_id_scope(tmp_path):
    validator = _validator()
    findings = tmp_path / "findings"
    findings.mkdir()
    (findings / "mixed.ttl").write_text(
        PREFIX
        + (
            "triage:a a triage:Finding ; triage:findingId \"AI21-001\" ; "
            "triage:foundIn triage:S1 ; "
            "triage:foundAt \"2026-07-01T12:00:00Z\"^^xsd:dateTime ; "
            "triage:severity \"important\" ; triage:description \"A\" ; "
            "triage:hasOccurrence triage:oa .\n"
            "triage:oa triage:file \"/abs/selected.rs\" .\n"
            "triage:b a triage:Finding ; triage:findingId \"AI10-001\" ; "
            "triage:foundIn triage:S1 ; "
            "triage:foundAt \"2026-07-01T12:00:00Z\"^^xsd:dateTime ; "
            "triage:severity \"important\" ; triage:description \"B\" ; "
            "triage:hasOccurrence triage:ob .\n"
            "triage:ob triage:file \"/abs/unselected.rs\" .\n"
        )
    )

    result = validator.run_validation(
        findings_dir=findings,
        finding_id_regex=r"^AI21-",
    )

    assert result.kind == "validation:fail"
    assert result.summary.findings == 1
    assert len(result.diagnostics) == 1
    assert "selected.rs" in result.diagnostics[0]
