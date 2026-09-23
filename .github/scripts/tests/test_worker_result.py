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
    value = result(status='failed', exit_status=1, error={'code': 'DENIED', 'message': 'upstream denied'})
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


@pytest.mark.parametrize("detail", [
    "//registry.npmjs.org/:_authToken=SYNTHETIC_SECRET",
    "https://user:SYNTHETIC_SECRET@registry.npmjs.org/pkg",
    '{"authToken":"SYNTHETIC_SECRET"}',
    '{"accessToken":"SYNTHETIC_SECRET"}',
])
def test_common_npm_credentials_are_redacted(detail):
    from worker_result import redact_diagnostic
    sanitized = redact_diagnostic(detail + "\nE403 permission denied")
    assert "SYNTHETIC_SECRET" not in sanitized
    assert "E403 permission denied" in sanitized


@pytest.mark.parametrize("key", ["authToken", "accessToken", "_authToken"])
def test_nested_camelcase_credentials_are_redacted(key):
    from worker_result import validate_result
    checked = validate_result(result(evidence={key: "SYNTHETIC_SECRET", "message": "E403 denied"}))
    assert checked["evidence"][key] == "<redacted>"
    assert checked["evidence"]["message"] == "E403 denied"


@pytest.mark.parametrize("error", [{"code": "X"}, "NPM.PUBLISH_FAILED"])
def test_failure_code_without_diagnostics_is_rejected(error):
    from worker_result import validate_result
    value = result(status="failed", exit_status=1, error=error,
                   evidence=[], registry_outcome="", verification=[], sanitized_diagnostic="")
    with pytest.raises(WorkerResultError, match="meaningful"):
        validate_result(value)
    value["sanitized_diagnostic"] = "E403: registry denied publish permission"
    assert validate_result(value)["status"] == "failed"


def test_surplus_reports_are_retained_as_contract_failures():
    extra = result(channel="pypi", evidence={"authToken": "SYNTHETIC_SECRET"})
    values = aggregate_results([result(), extra], ["npm"])
    assert len(values) == 2
    assert values[0]["status"] == "passed"
    assert values[1]["error"]["code"] == "REPORTING.CONTRACT_FAILURE"
    assert values[1]["evidence"]["unexpected_result"]["channel"] == "pypi"
    assert "SYNTHETIC_SECRET" not in json.dumps(values)


@pytest.mark.parametrize("scenario", ["passed", "failed", "missing", "malformed", "surplus", "code_only"])
def test_publisher_cli_validates_raw_responses_and_preserves_all_results(tmp_path, scenario):
    import subprocess
    first = tmp_path / "first.txt"
    value = result()
    if scenario == "failed":
        value.update(status="failed", exit_status=1, error={"code": "DENIED", "message": "E403 denied"})
    if scenario == "code_only":
        value.update(status="failed", exit_status=1, error="NPM.PUBLISH_FAILED", sanitized_diagnostic="")
    first.write_text("```json\n" + json.dumps(value) + "\n```")
    command = [sys.executable, str(Path(__file__).resolve().parents[1] / "worker_result.py"), "--expected-channels", "npm"]
    if scenario != "missing":
        command += ["--result", str(first)]
    if scenario == "malformed":
        first.write_text("not JSON")
    if scenario == "surplus":
        extra = tmp_path / "extra.txt"
        extra.write_text("```json\n" + json.dumps(result(channel="pypi")) + "\n```")
        command += ["--result", str(extra)]
    completed = subprocess.run(command, capture_output=True, text=True, timeout=30)
    assert (completed.returncode == 0) is (scenario == "passed")
    assert completed.stdout.startswith("```json\n")
    values = json.loads(completed.stdout.removeprefix("```json\n").removesuffix("\n```\n"))["results"]
    assert len(values) == (2 if scenario == "surplus" else 1)
    if scenario == "failed":
        assert values[0]["error"]["message"] == "E403 denied"
    elif scenario != "passed":
        assert values[-1]["error"]["code"] == "REPORTING.CONTRACT_FAILURE"
