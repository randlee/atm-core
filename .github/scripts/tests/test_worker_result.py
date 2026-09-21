import json
from pathlib import Path
import sys

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from worker_result import WorkerResultError, aggregate_results, parse_fenced_result


def result(**overrides):
    value = {
        "channel": "npm", "status": "passed", "tag": "v1.2.3", "commit": "a" * 40,
        "command": ["npm", "publish"], "exit_status": 0, "error": None, "attempts": 1,
        "workflow_url": "unavailable", "job_url": "unavailable", "evidence": ["run log"],
        "registry_outcome": "published", "verification": ["integrity matched"], "sanitized_diagnostic": "",
        "checks": [{"kind": "publish", "status": "passed"}], "required_checks": [],
    }
    value.update(overrides)
    return value


def test_success_result_requires_full_provenance():
    parsed = parse_fenced_result("before\n```json\n" + json.dumps(result()) + "\n```\nafter")
    assert parsed["status"] == "passed"


def test_failure_result_preserves_error_details():
    parsed = parse_fenced_result("```json\n" + json.dumps(result(status="failed", exit_status=1, error={"code": "NPM.PUBLISH_FAILED", "details": "denied"})) + "\n```")
    assert parsed["error"]["details"] == "denied"


def test_passed_nonzero_exit_is_a_contract_failure():
    with pytest.raises(WorkerResultError, match="exit_status"):
        parse_fenced_result("```json\n" + json.dumps(result(exit_status=1)) + "\n```")


def test_passed_report_requires_checks_and_meaningful_provenance():
    with pytest.raises(WorkerResultError):
        incomplete = result()
        del incomplete["checks"]
        parse_fenced_result("```json\n" + json.dumps(incomplete) + "\n```")
    with pytest.raises(WorkerResultError):
        parse_fenced_result("```json\n" + json.dumps(result(workflow_url=None)) + "\n```")
    with pytest.raises(WorkerResultError):
        parse_fenced_result("```json\n" + json.dumps(result(command=[])) + "\n```")


def test_checks_and_required_checks_have_structured_entries_and_consistent_success():
    with pytest.raises(WorkerResultError):
        parse_fenced_result("```json\n" + json.dumps(result(checks=[{}])) + "\n```")
    with pytest.raises(WorkerResultError):
        parse_fenced_result("```json\n" + json.dumps(result(required_checks=[{}])) + "\n```")
    with pytest.raises(WorkerResultError):
        parse_fenced_result("```json\n" + json.dumps(result(checks=[{"kind": "publish", "status": "failed"}])) + "\n```")
    with pytest.raises(WorkerResultError, match="outstanding required_checks"):
        parse_fenced_result("```json\n" + json.dumps(result(required_checks=[{"kind": "approval", "reason": "pending"}])) + "\n```")


def test_nested_check_types_fail_closed_without_losing_valid_channels():
    values = aggregate_results(
        [result(), {**result(), "channel": "pypi", "checks": [{"kind": "publish", "status": []}]}],
        ["npm", "pypi"],
    )
    assert values[0]["status"] == "passed"
    assert values[1]["error"]["code"] == "REPORTING.CONTRACT_FAILURE"


@pytest.mark.parametrize("text", ["", "```json\n{}\n```", "```json\nnot-json\n```"])
def test_missing_or_malformed_result_fails_closed(text):
    with pytest.raises(WorkerResultError):
        parse_fenced_result(text)


def test_aggregation_rejects_missing_and_mismatched_workers():
    missing = aggregate_results([None], ["npm"])
    assert missing[0]["error"]["code"] == "REPORTING.CONTRACT_FAILURE"
    mismatch = aggregate_results([result(channel="pypi")], ["npm"])
    assert mismatch[0]["error"]["code"] == "REPORTING.CONTRACT_FAILURE"


def test_aggregation_preserves_valid_channel_when_another_is_missing():
    values = aggregate_results([result(), None], ["npm", "pypi"])
    assert values[0]["status"] == "passed"
    assert values[1]["error"]["code"] == "REPORTING.CONTRACT_FAILURE"
    assert values[1]["checks"][0]["status"] == "failed"
    assert values[1]["command"]
    assert values[1]["required_checks"] == []


def test_aggregation_converts_malformed_objects_and_scalars_to_contract_failures():
    values = aggregate_results([result(), {**result(), "channel": "pypi", "status": []}], ["npm", "pypi"])
    assert values[0]["status"] == "passed"
    assert values[1]["error"]["code"] == "REPORTING.CONTRACT_FAILURE"
    scalar = aggregate_results(["not-an-object"], ["npm"])
    assert scalar[0]["error"]["code"] == "REPORTING.CONTRACT_FAILURE"


@pytest.mark.parametrize("status", ["apparently_available", "taken"])
def test_inquiry_statuses_may_have_no_error(status):
    assert parse_fenced_result("```json\n" + json.dumps(result(status=status)) + "\n```")["status"] == status


@pytest.mark.parametrize('field', ['error', 'evidence', 'verification', 'registry_outcome'])
def test_nested_credentials_are_sanitized_without_mutating_input(field):
    from copy import deepcopy
    from worker_result import validate_result
    value = result(status='failed', exit_status=1, error={'code': 'DENIED'})
    value[field] = {'details': [{'token': 'SYNTHETIC_SECRET', 'message': 'E403 denied'}]}
    original = deepcopy(value)
    checked = validate_result(value)
    assert 'SYNTHETIC_SECRET' not in json.dumps(checked)
    assert checked[field]['details'][0]['message'] == 'E403 denied'
    assert value == original


@pytest.mark.parametrize('scheme', ['Basic', 'token', 'Bearer'])
def test_headers_in_any_report_text_are_redacted(scheme):
    from worker_result import validate_result
    checked = validate_result(result(sanitized_diagnostic=f'Authorization: {scheme} SYNTHETIC_SECRET\nE403 denied'))
    assert 'SYNTHETIC_SECRET' not in json.dumps(checked)
    assert 'E403 denied' in checked['sanitized_diagnostic']


def test_command_credentials_are_redacted_and_arguments_preserved():
    from worker_result import validate_result
    checked = validate_result(result(command=['npm', '--token', 'SYNTHETIC_SECRET', '--registry=https://registry.npmjs.org', 'NPM_TOKEN=ANOTHER_SECRET']))
    assert checked['command'] == ['npm', '--token', '<redacted>', '--registry=https://registry.npmjs.org', 'NPM_TOKEN=<redacted>']


def test_aggregation_sanitizes_valid_reports_and_retains_other_contract_failures():
    values = aggregate_results([result(evidence={'access_token':'SYNTHETIC_SECRET'}), None], ['npm', 'pypi'])
    assert values[0]['status'] == 'passed'
    assert 'SYNTHETIC_SECRET' not in json.dumps(values)
    assert values[1]['status'] == 'failed'
