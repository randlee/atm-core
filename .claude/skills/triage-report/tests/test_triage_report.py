import importlib.util
import json
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "scripts" / "triage_report.py"
spec = importlib.util.spec_from_file_location("triage_report", SCRIPT)
triage_report = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(triage_report)

PREFIX = "@prefix triage: <urn:atm:triage:> .\n@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n"


def _inputs(tmp_path: Path):
    root = tmp_path / "repo"
    structure_dir = root / ".sprints" / "AICH"
    structure_dir.mkdir(parents=True)
    (structure_dir / "structure.ttl").write_text(
        PREFIX
        + "triage:PhaseAICH a triage:Phase .\n"
        + "triage:AICH-S1 a triage:Sprint ; triage:inPhase triage:PhaseAICH ; triage:order 1 ; triage:criteria \"docs/s1.md\" .\n"
        + "triage:AICH-S2 a triage:Sprint ; triage:inPhase triage:PhaseAICH ; triage:order 2 ; triage:criteria \"docs/s2.md\" .\n"
    )
    (structure_dir / "events.ttl").write_text(
        PREFIX
        + "triage:a1 a triage:Assignment ; triage:ofSprint triage:AICH-S1 ; triage:assignedAt \"2026-07-25T01:00:00Z\"^^xsd:dateTime .\n"
        + "triage:c1 a triage:Completion ; triage:ofSprint triage:AICH-S1 ; triage:at \"2026-07-25T02:00:00Z\"^^xsd:dateTime .\n"
        + "triage:a2 a triage:Assignment ; triage:ofSprint triage:AICH-S2 ; triage:assignedAt \"2026-07-25T03:00:00Z\"^^xsd:dateTime .\n"
    )
    qa_path = root / "qa.json"
    qa_path.write_text(json.dumps({"runs": [
        {"run_id": "S1-QA1", "aich_sprint": "AICH-S1", "run_type": "qa", "result_time_utc": "2026-07-25T03:00:00Z", "verdict": "FAIL", "blockers": 1, "important": 2, "minor": 0, "count_basis": "headline"},
        {"run_id": "S1-review", "aich_sprint": "AICH-S1", "run_type": "reviewer-only", "result_time_utc": "2026-07-25T04:00:00Z", "verdict": "PASS", "blockers": 0, "important": 0, "minor": 0},
        {"run_id": "S2-QA1", "aich_sprint": "AICH-S2", "run_type": "qa", "result_time_utc": "2026-07-25T05:00:00Z", "verdict": "PASS", "blockers": 0, "important": 0, "minor": 0},
    ]}))
    metadata = root / "metadata.json"
    metadata.write_text(json.dumps({"sprints": [
        {"id": "AICH-S1", "branch": "feature/s1", "head_sha": "abc", "pr_number": 1, "ci_status": "pass", "merged": True},
        {"id": "AICH-S2", "branch": "feature/s2", "head_sha": "def", "pr_number": 2, "ci_status": "pending", "merged": False},
    ]}))
    return root, qa_path, metadata


def test_latest_authoritative_qa_and_gates(tmp_path):
    root, qa, metadata = _inputs(tmp_path)
    report = triage_report.build_report(root, "AICH", qa, metadata)
    first, second = report["rows"]
    assert first["qa"]["run_id"] == "S1-QA1"  # reviewer-only is excluded
    assert first["qa"]["blockers"] == 1
    assert first["ready_to_merge"] is False
    assert first["ok_to_merge"] is False
    assert second["ready_to_merge"] is True
    assert second["previous_sprints_merged"] is True
    assert second["ok_to_merge"] is True
    assert "| Sprint | DEV | QA | CI | PR | B | I | M | Ready | OK |" in report["table"]
    assert "❌" in report["table"] and "🏁" in report["table"]


def test_unknown_merge_is_fail_closed(tmp_path):
    root, qa, _ = _inputs(tmp_path)
    report = triage_report.build_report(root, "AICH", qa)
    first, second = report["rows"]
    assert first["merged"] is None
    assert first["ok_to_merge"] is False  # blockers are known and nonzero
    assert second["previous_sprints_merged"] is None
    assert second["ok_to_merge"] is None
    assert report["data_gaps"]


def test_missing_integration_worktree_is_structured_error(tmp_path, monkeypatch):
    monkeypatch.setattr(triage_report, "_git", lambda *args: "develop")
    result = triage_report.main([])
    assert result == 2


def test_malformed_structure_is_report_error(tmp_path):
    root = tmp_path / "repo"
    phase = root / ".sprints" / "AICH"
    phase.mkdir(parents=True)
    (phase / "structure.ttl").write_text("not turtle [")
    try:
        triage_report.build_report(root, "AICH")
    except triage_report.ReportError as exc:
        assert "malformed Turtle" in str(exc)
    else:
        raise AssertionError("malformed structure must fail")
