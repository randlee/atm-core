import importlib.util
import json
from pathlib import Path

import pytest


SCRIPT = Path(__file__).parents[1] / "scripts" / "triage_report.py"
spec = importlib.util.spec_from_file_location("triage_report", SCRIPT)
triage_report = importlib.util.module_from_spec(spec)
assert spec.loader is not None
spec.loader.exec_module(triage_report)
GITHUB_STATE = triage_report._github_state
REAL_RUN_FINDINGS_VALIDATOR = triage_report._run_findings_validator

PREFIX = "@prefix triage: <urn:atm:triage:> .\n@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .\n"


@pytest.fixture(autouse=True)
def _validator_passes(monkeypatch):
    """Keep report unit tests hermetic; validator invocation has a dedicated test."""
    monkeypatch.setattr(
        triage_report,
        "_run_findings_validator",
        lambda *args: {
            "kind": "validation:pass",
            "diagnostics": [],
            "summary": {"files": 1, "findings": 1, "errors": 0, "warnings": 0},
        },
    )
    monkeypatch.setattr(
        triage_report,
        "_github_state",
        lambda _root, sprints: (
            {
                sprint["id"]: {
                    "branch": sprint["branch"],
                    "head_sha": f"{sprint['id']}-sha",
                    "target_branch": "integrate/phase-ai",
                    "pr_number": sprint["order"],
                    "pr_url": f"https://example.test/pr/{sprint['order']}",
                    "ci_status": "pass",
                    "merged": sprint["order"] == 1,
                    "delivery_attempts": [],
                }
                for sprint in sprints
            },
            "example/test",
        ),
    )


def _inputs(tmp_path: Path):
    root = tmp_path / "repo"
    structure_dir = root / ".sprints" / "AICH"
    structure_dir.mkdir(parents=True)
    (structure_dir / "structure.ttl").write_text(
        PREFIX
        + "triage:PhaseAICH a triage:Phase .\n"
        + "triage:AICH-S1 a triage:Sprint ; triage:inPhase triage:PhaseAICH ; triage:order 1 ; triage:criteria \"docs/plans/phase-ai/sprint-ai-21-pre.md\" ; triage:branch \"feature/s1\" .\n"
        + "triage:AICH-S2 a triage:Sprint ; triage:inPhase triage:PhaseAICH ; triage:order 2 ; triage:criteria \"docs/plans/phase-ai/sprint-ai-22.md\" ; triage:branch \"feature/s2\" .\n"
    )
    (structure_dir / "events.ttl").write_text(
        PREFIX
        + "triage:a1 a triage:Assignment ; triage:ofSprint triage:AICH-S1 ; triage:assignedAt \"2026-07-25T01:00:00Z\"^^xsd:dateTime .\n"
        + "triage:c1 a triage:Completion ; triage:ofSprint triage:AICH-S1 ; triage:at \"2026-07-25T02:00:00Z\"^^xsd:dateTime .\n"
        + "triage:a2 a triage:Assignment ; triage:ofSprint triage:AICH-S2 ; triage:assignedAt \"2026-07-25T03:00:00Z\"^^xsd:dateTime .\n"
    )
    findings_dir = root / ".triage" / "phase-AI" / "findings"
    findings_dir.mkdir(parents=True)
    (findings_dir / "S1.ttl").write_text(
        PREFIX
        + "triage:F1 a triage:Finding ; triage:findingId \"F1\" ; "
        + "triage:foundIn triage:AICH-S1 ; "
        + "triage:foundAt \"2026-07-25T03:00:00Z\"^^xsd:dateTime ; "
        + "triage:severity \"blocking\" ; triage:description \"live blocker\" ; "
        + "triage:hasOccurrence triage:O1 .\n"
        + "triage:F2 a triage:Finding ; triage:findingId \"F2\" ; "
        + "triage:foundIn triage:AICH-S1 ; "
        + "triage:foundAt \"2026-07-25T04:00:00Z\"^^xsd:dateTime ; "
        + "triage:severity \"minor\" ; triage:description \"live minor\" ; "
        + "triage:hasOccurrence triage:O2 .\n"
        + "triage:F3 a triage:Finding ; triage:findingId \"F3\" ; "
        + "triage:foundIn triage:AICH-S2 ; "
        + "triage:foundAt \"2026-07-25T05:00:00Z\"^^xsd:dateTime ; "
        + "triage:severity \"blocking\" ; triage:description \"closed\" ; "
        + "triage:status \"fixed\" ; triage:hasOccurrence triage:O3 .\n"
        + "triage:O1 a triage:Occurrence ; triage:branch \"feature/s1\" ; "
        + "triage:status \"open\" ; triage:closed false .\n"
        + "triage:O2 a triage:Occurrence ; triage:branch \"feature/s1\" ; "
        + "triage:status \"open\" ; triage:closed false .\n"
        + "triage:O3 a triage:Occurrence ; triage:branch \"feature/s2\" ; "
        + "triage:status \"fixed\" ; triage:closed true .\n"
    )
    qa_path = root / "qa.json"
    qa_path.write_text(json.dumps({"runs": [
        {"run_id": "S1-QA1", "aich_sprint": "AICH-S1", "run_type": "qa", "result_time_utc": "2026-07-25T03:00:00Z", "verdict": "FAIL", "blockers": 1, "important": 2, "minor": 0, "count_basis": "headline"},
        {"run_id": "S1-review", "aich_sprint": "AICH-S1", "run_type": "reviewer-only", "result_time_utc": "2026-07-25T04:00:00Z", "verdict": "PASS", "blockers": 0, "important": 0, "minor": 0},
        {"run_id": "S2-QA1", "aich_sprint": "AICH-S2", "run_type": "qa", "result_time_utc": "2026-07-25T05:00:00Z", "verdict": "PASS", "blockers": 0, "important": 0, "minor": 0},
    ]}))
    return root, qa_path


def test_live_ttl_counts_drive_sprint_gates(tmp_path):
    root, qa = _inputs(tmp_path)
    report = triage_report.build_report(root, "AICH", qa)
    first, second = report["rows"]
    assert first["qa"]["run_id"] == "S1-QA1"  # reviewer-only is excluded
    assert first["qa"]["blockers"] == 1
    assert first["qa"]["important"] == 0
    assert first["qa"]["minor"] == 1
    assert first["qa"]["reported_counts"] == {"blockers": 1, "important": 2, "minor": 0}
    assert first["ready_to_merge"] is None  # already merged is history, not ready-to-merge
    assert first["ok_to_merge"] is None
    assert second["qa"]["blockers"] == 0
    assert second["ready_to_merge"] is True
    assert second["previous_sprints_merged"] is True
    assert second["ok_to_merge"] is True
    assert "| Sprint | DEV | QA | CI | PR | Live B | Live I | Live M | Ready | OK |" in report["table"]
    assert "| AICH-S1 (AI.21-pre) | ✅ | ❌ | ✅ | #1 🏁 |" in report["table"]


def test_github_state_replaces_manual_metadata(tmp_path):
    root, qa = _inputs(tmp_path)
    report = triage_report.build_report(root, "AICH", qa)
    first, second = report["rows"]
    assert first["merged"] is True
    assert second["previous_sprints_merged"] is True
    assert second["ok_to_merge"] is True
    assert not any("metadata" in gap for gap in report["data_gaps"])


def test_missing_qa_snapshot_does_not_hide_live_sprint_counts(tmp_path):
    root, _ = _inputs(tmp_path)
    report = triage_report.build_report(root, "AICH", root / "missing-qa.json")
    first = report["rows"][0]
    assert first["qa"]["blockers"] == 1
    assert first["qa"]["important"] == 0
    assert "QA evidence master not found" in report["data_gaps"][0]


def test_report_ignores_unrelated_project_phase_findings(tmp_path):
    """A historical malformed phase cannot block the current phase report."""
    root, qa = _inputs(tmp_path)
    unrelated = root / ".triage" / "phase-U" / "findings"
    unrelated.mkdir(parents=True)
    (unrelated / "legacy.ttl").write_text("finding_id: legacy\n")

    report = triage_report.build_report(root, "AICH", qa)

    assert report["rows"][0]["qa"]["blockers"] == 1


def test_malformed_selected_finding_only_marks_attributed_row(tmp_path):
    """A broken selected TTL must not hide valid sprint rows."""
    root, qa = _inputs(tmp_path)
    broken = root / ".triage" / "phase-AI" / "findings" / "BROKEN-S1.ttl"
    broken.write_text(
        PREFIX
        + 'triage:broken a triage:Finding ; triage:foundIn triage:AICH-S1 ;\n'
    )

    validation = REAL_RUN_FINDINGS_VALIDATOR(
        root,
        root / ".triage" / "phase-AI" / "findings",
        root / ".sprints" / "AICH" / "structure.ttl",
        root / ".sprints" / "AICH" / "events.ttl",
    )
    assert validation["kind"] == "error"

    report = triage_report.build_report(root, "AICH", qa)

    assert [row["id"] for row in report["rows"]] == ["AICH-S1", "AICH-S2"]
    first, second = report["rows"]
    assert first["data_status"] == "error"
    assert first["ready_to_merge"] is False
    assert first["ok_to_merge"] is None  # merged rows are not merge candidates
    assert first["diagnostics"][0]["sprint"] == "AICH-S1"
    assert first["diagnostics"][0]["path"].endswith("BROKEN-S1.ttl")
    assert "repair Turtle syntax" in first["diagnostics"][0]["action"]
    assert second["data_status"] == "ok"
    assert second["ready_to_merge"] is True
    assert report["merge_blocked"] is True
    assert report["dispatch_blocked"] is True
    assert "BROKEN-S1.ttl" in report["table"]
    assert "Action: repair Turtle syntax" in report["table"]


def test_malformed_selected_finding_without_found_in_is_unattributed(tmp_path):
    """A broken TTL without a recoverable sprint is globally visible."""
    root, qa = _inputs(tmp_path)
    broken = root / ".triage" / "phase-AI" / "findings" / "BROKEN-UNKNOWN.ttl"
    broken.write_text(PREFIX + "triage:broken a triage:Finding ;\n")

    report = triage_report.build_report(root, "AICH", qa)

    assert any(
        item["code"] == "unattributed_malformed_finding_ttl"
        and item["sprint"] is None
        for item in report["diagnostics"]
    )
    assert "unattributed" in report["table"]
    assert "BROKEN-UNKNOWN.ttl" in report["table"]
    assert report["merge_blocked"] is True


def test_promoted_finding_gates_its_open_branch_only(tmp_path):
    root, qa = _inputs(tmp_path)
    findings = root / ".triage" / "phase-AI" / "findings" / "S1.ttl"
    with findings.open("a") as stream:
        stream.write(
            "triage:F4 a triage:Finding ; triage:findingId \"F4\" ; "
            "triage:foundIn triage:AICH-S1 ; "
            "triage:foundAt \"2026-07-25T06:00:00Z\"^^xsd:dateTime ; "
            "triage:severity \"blocking\" ; triage:description \"promoted\" ; "
            "triage:hasOccurrence triage:O4S1, triage:O4S2 .\n"
            "triage:O4S1 a triage:Occurrence ; triage:branch \"feature/s1\" ; "
            "triage:status \"closed\" ; triage:closed true .\n"
            "triage:O4S2 a triage:Occurrence ; triage:branch \"feature/s2\" ; "
            "triage:status \"open\" ; triage:closed false .\n"
        )

    report = triage_report.build_report(root, "AICH", qa)
    first, second = report["rows"]
    assert first["qa"]["blockers"] == 1
    assert second["qa"]["blockers"] == 1
    assert report["current_integration_counts"]["blockers"] == 2  # F1 + F4, once each


def test_fixed_finding_with_open_occurrence_is_diagnostic_not_blocker(tmp_path):
    root, qa = _inputs(tmp_path)
    findings = root / ".triage" / "phase-AI" / "findings" / "S1.ttl"
    with findings.open("a") as stream:
        stream.write(
            "triage:F5 a triage:Finding ; triage:findingId \"F5\" ; "
            "triage:foundIn triage:AICH-S1 ; "
            "triage:foundAt \"2026-07-25T07:00:00Z\"^^xsd:dateTime ; "
            "triage:severity \"blocking\" ; triage:description \"fixed but stale occurrence\" ; "
            "triage:status \"fixed\" ; triage:hasOccurrence triage:O5 .\n"
            "triage:O5 a triage:Occurrence ; triage:branch \"feature/s1\" ; "
            "triage:status \"open\" ; triage:closed false .\n"
        )

    report = triage_report.build_report(root, "AICH", qa)
    assert report["current_integration_counts"]["blockers"] == 1  # F1 only
    assert report["stale_occurrences"] == [
        {"finding_id": "F5", "branch": "feature/s1", "path": "S1.ttl"}
    ]
    assert "does not reopen fixed findings" in report["table"]


def test_missing_integration_worktree_is_structured_error(tmp_path, monkeypatch):
    monkeypatch.setattr(triage_report, "_git", lambda *args: "develop")
    result = triage_report.main([])
    assert result == 2


def test_malformed_structure_is_report_error(tmp_path):
    root = tmp_path / "repo"
    phase = root / ".sprints" / "AICH"
    phase.mkdir(parents=True)
    (root / ".triage").mkdir()
    (phase / "structure.ttl").write_text("not turtle [")
    try:
        triage_report.build_report(root, "AICH")
    except triage_report.ReportError as exc:
        assert "malformed Turtle" in str(exc)
    else:
        raise AssertionError("malformed structure must fail")


def test_duplicate_structure_orders_are_report_error(tmp_path):
    root, _ = _inputs(tmp_path)
    structure = root / ".sprints" / "AICH" / "structure.ttl"
    structure.write_text(
        PREFIX
        + "triage:PhaseAICH a triage:Phase .\n"
        + "triage:AICH-S1 a triage:Sprint ; triage:inPhase triage:PhaseAICH ; triage:order 1 ; triage:criteria \"docs/s1.md\" .\n"
        + "triage:AICH-S2 a triage:Sprint ; triage:inPhase triage:PhaseAICH ; triage:order 1 ; triage:criteria \"docs/s2.md\" .\n"
    )
    try:
        triage_report.build_report(root, "AICH")
    except triage_report.ReportError as exc:
        assert "orders must be unique" in str(exc)
    else:
        raise AssertionError("duplicate sprint orders must fail")


def test_findings_validator_is_called_before_rows(tmp_path, monkeypatch):
    root, qa = _inputs(tmp_path)
    calls = []

    def validator(*args):
        calls.append(args)
        return {"kind": "validation:pass", "diagnostics": []}

    monkeypatch.setattr(triage_report, "_run_findings_validator", validator)
    report = triage_report.build_report(root, "AICH", qa)
    assert len(calls) == 1
    assert calls[0][1].name == "findings"
    assert calls[0][2].name == "structure.ttl"
    assert calls[0][3].name == "events.ttl"
    assert report["validation"]["kind"] == "validation:pass"


def test_findings_validator_subprocess_passes_for_valid_schema(tmp_path):
    root, _ = _inputs(tmp_path)
    findings_dir = root / ".triage" / "phase-AI" / "findings"
    result = triage_report._run_findings_validator(
        root,
        findings_dir,
        root / ".sprints" / "AICH" / "structure.ttl",
        root / ".sprints" / "AICH" / "events.ttl",
    )
    assert result["kind"] == "validation:pass"


def test_validator_failure_blocks_report(tmp_path, monkeypatch):
    root, qa = _inputs(tmp_path)

    def validator(*args):
        raise triage_report.ReportError(
            "findings validation blocked report: validation:fail"
        )

    monkeypatch.setattr(triage_report, "_run_findings_validator", validator)
    with pytest.raises(triage_report.ReportError, match="validation blocked"):
        triage_report.build_report(root, "AICH", qa)


def test_phase_sprint_accepts_non_ai_prefix_and_rejects_bad_convention():
    assert triage_report._phase_sprint("docs/sprint-ak-7.md") == "AK.7"
    assert triage_report._phase_sprint("docs/sprint-ak-7-pre-notes.md") == "AK.7-pre"
    with pytest.raises(triage_report.ReportError, match="unsupported sprint criteria"):
        triage_report._phase_sprint("docs/sprint-ak-not-number.md")


def test_phase_sprint_accepts_concatenated_prefix_and_number():
    # This repo's longer-standing convention (phases AA through AJ, predating
    # Phase AI's hyphenated style) has no separator between the letter prefix
    # and the sprint number.
    assert triage_report._phase_sprint("docs/plans/phase-aj/sprint-AJ1.md") == "AJ.1"
    assert triage_report._phase_sprint("docs/plans/phase-aj/sprint-AJ10.md") == "AJ.10"
    assert (
        triage_report._phase_sprint("docs/plans/phase-ak/sprint-AK1-crosshost-salvage-audit.md")
        == "AK.1-crosshost"
    )


def test_current_open_replay_beats_historical_merged_pr():
    merged = {
        "number": 631,
        "state": "MERGED",
        "createdAt": "2026-07-25T16:09:01Z",
        "mergedAt": "2026-07-25T16:26:53Z",
    }
    replay = {
        "number": 640,
        "state": "OPEN",
        "createdAt": "2026-07-25T22:38:23Z",
    }
    assert triage_report._current_pr([merged, replay]) == replay


def test_github_state_reports_no_branch_when_ttl_omits_it(tmp_path, monkeypatch):
    # triage:branch is the sole source of truth for a sprint's branch; a
    # sprint that omits it must surface as a missing/unknown data gap rather
    # than a guessed branch name derived from its criteria filename.
    root, _ = _inputs(tmp_path)
    structure = triage_report._parse_ttl(root / ".sprints" / "AICH" / "structure.ttl")
    sprints = triage_report._sprints(structure, "AICH")
    monkeypatch.setattr(triage_report, "_origin_repo", lambda _root: "example/test")
    no_branch_sprints = [dict(sprint, branch=None) for sprint in sprints]
    states, repo = GITHUB_STATE(root, no_branch_sprints)
    assert repo == "example/test"
    for sprint in no_branch_sprints:
        assert states[sprint["id"]] == {}


def test_main_format_vars_includes_data_gaps(tmp_path, capsys):
    # The /sprint-report skill pipes exactly `--format vars` into sc-compose;
    # data_gaps must survive that whitelist or the rendered report silently
    # drops the diagnostics build_report() already computed.
    root, qa = _inputs(tmp_path)
    result = triage_report.main([
        "--integration-root", str(root),
        "--phase", "AICH",
        "--qa-master", str(qa),
        "--format", "vars",
    ])
    assert result == 0
    parsed = json.loads(capsys.readouterr().out)
    assert "data_gaps" in parsed
    assert isinstance(parsed["data_gaps"], list)


def test_main_blocks_report_when_data_gaps_present(tmp_path, capsys):
    # Missing source data must never render a report that looks authoritative.
    # main() has to refuse and hand the calling agent something it can act on:
    # a non-zero exit and an explicit statement that closing the gap is
    # team-lead's job, not a rendering detail to paper over.
    root, _ = _inputs(tmp_path)
    for output_format in ("vars", "table", "detailed", "json"):
        result = triage_report.main([
            "--integration-root", str(root),
            "--phase", "AICH",
            "--qa-master", str(root / "missing-qa.json"),
            "--format", output_format,
        ])
        assert result == 3
        parsed = json.loads(capsys.readouterr().out)
        assert parsed["kind"] == "data_gap"
        assert parsed["dispatch_blocked"] is True
        assert parsed["merge_blocked"] is True
        assert "reporting owner" in parsed["message"]
        assert any("QA evidence master not found" in gap for gap in parsed["data_gaps"])
        assert parsed["repair_guide"] == "docs/triage/ttl-repair.md"
        assert parsed["error"] == {
            "code": "TRIAGE.INCOMPLETE_DATA",
            "message": "Authoritative report inputs are incomplete.",
            "recoverable": True,
            "suggested_action": (
                "Execute every remediation in the named integration worktree, "
                "then rerun the report."
            ),
        }
        qa_master = next(
            item for item in parsed["remediations"]
            if item["code"] == "TTL.QA_MASTER_MISSING"
        )
        assert qa_master == {
            "code": "TTL.QA_MASTER_MISSING",
            "source": "qa_evidence_master",
            "path": "missing-qa.json",
            "sprint_id": None,
            "problem": f"QA evidence master not found: {root / 'missing-qa.json'}",
            "action": "Restore the authoritative QA evidence master with its recorded QA runs.",
            "target_branch": "integrate/phase-ai",
            "guide": "docs/triage/ttl-repair.md",
        }


def test_missing_sprint_branch_has_a_targeted_ttl_remediation(tmp_path, capsys):
    root, qa = _inputs(tmp_path)
    structure_path = root / ".sprints" / "AICH" / "structure.ttl"
    structure_path.write_text(
        structure_path.read_text().replace(' ; triage:branch "feature/s1"', "")
    )

    result = triage_report.main([
        "--integration-root", str(root),
        "--phase", "AICH",
        "--qa-master", str(qa),
        "--format", "json",
    ])

    assert result == 3
    parsed = json.loads(capsys.readouterr().out)
    assert "AICH-S1: triage:branch is missing" in parsed["data_gaps"]
    assert {
        "code": "TTL.SPRINT_BRANCH_MISSING",
        "source": "phase_structure",
        "path": ".sprints/AICH/structure.ttl",
        "sprint_id": "AICH-S1",
        "problem": "AICH-S1: triage:branch is missing",
        "action": "Add the sprint's declared triage:branch; do not infer it from the criteria filename.",
        "target_branch": "integrate/phase-ai",
        "guide": "docs/triage/ttl-repair.md",
    } in parsed["remediations"]


def test_sprint_report_skill_points_to_one_ttl_repair_guide():
    skill = (SCRIPT.parents[2] / "sprint-report" / "SKILL.md").read_text()
    assert skill.count("docs/triage/ttl-repair.md") == 1
    assert "remediations[]" in skill


def test_triage_report_skill_displays_findings_and_uses_the_repair_contract():
    skill = (SCRIPT.parents[1] / "SKILL.md").read_text()
    assert skill.count("docs/triage/ttl-repair.md") == 1
    assert "remediations[]" in skill
    assert "Findings and evidence displayed" in skill
    assert "live unresolved B/I/M" in skill


def test_github_state_prefers_open_replay_and_retains_merged_history(tmp_path, monkeypatch):
    root, _ = _inputs(tmp_path)
    structure = triage_report._parse_ttl(root / ".sprints" / "AICH" / "structure.ttl")
    sprints = triage_report._sprints(structure, "AICH")
    monkeypatch.setattr(triage_report, "_origin_repo", lambda _root: "example/test")
    monkeypatch.setattr(
        triage_report,
        "_github_prs",
        lambda _root, _repo, branch: [
            {
                "number": 631,
                "state": "MERGED",
                "createdAt": "2026-07-25T16:09:01Z",
                "mergedAt": "2026-07-25T16:26:53Z",
                "headRefOid": "old",
                "baseRefName": "integrate/phase-ai",
                "mergeCommit": {"oid": "merge"},
                "url": "https://example.test/631",
                "statusCheckRollup": [{"conclusion": "SUCCESS"}],
            },
            {
                "number": 640,
                "state": "OPEN",
                "createdAt": "2026-07-25T22:38:23Z",
                "mergedAt": None,
                "headRefOid": "new",
                "baseRefName": "integrate/phase-ai",
                "mergeCommit": None,
                "url": "https://example.test/640",
                "statusCheckRollup": [{"conclusion": "FAILURE"}],
            },
        ] if branch == "feature/s1" else [],
    )
    state, repo = GITHUB_STATE(root, sprints)
    assert repo == "example/test"
    assert state["AICH-S1"]["pr_number"] == 640
    assert state["AICH-S1"]["merged"] is False
    assert state["AICH-S1"]["ci_status"] == "fail"
    assert [attempt["pr_number"] for attempt in state["AICH-S1"]["delivery_attempts"]] == [631, 640]
