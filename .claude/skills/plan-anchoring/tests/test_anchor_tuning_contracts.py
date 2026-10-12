import json
import importlib.util
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[5]
SKILL = ROOT / "plugins" / "sc-plan-anchor" / "skills" / "plan-anchoring"
PLAN = ROOT / "docs" / "plans" / "phase-a"


def fenced_json(text: str) -> dict:
    blocks = re.findall(r"```json\s*(.*?)\s*```", text, re.S)
    if len(blocks) != 1:
        raise AssertionError(f"expected one fenced JSON block, found {len(blocks)}")
    return json.loads(blocks[0])


class AnchorTuningContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        try:
            import jsonschema
        except ImportError as exc:  # pragma: no cover - CI dependency failure
            raise AssertionError("jsonschema is required for contract validation") from exc
        cls.jsonschema = jsonschema
        contract_path = SKILL / "scripts" / "anchor_tuning_contract.py"
        spec = importlib.util.spec_from_file_location("anchor_tuning_contract", contract_path)
        cls.contract = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.contract)

    def render(self, template: str, variables: str) -> dict:
        binary = shutil.which("sc-compose")
        if not binary:
            self.fail("sc-compose is required for strict contract rendering")
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "rendered.md"
            subprocess.run(
                [
                    binary,
                    "render",
                    "--mode",
                    "file",
                    "--file",
                    str(SKILL / "templates" / template),
                    "--var-file",
                    str(PLAN / "templates" / variables),
                    "--strict",
                    "--check-render",
                    "--output",
                    str(output),
                ],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            )
            return fenced_json(output.read_text(encoding="utf-8"))

    def validate(self, instance: dict, schema_name: str) -> None:
        schema = json.loads(
            (SKILL / "assets" / "schemas" / schema_name).read_text(encoding="utf-8")
        )
        self.jsonschema.Draft202012Validator(schema).validate(instance)

    def test_v2_tune_request_renders_and_validates(self):
        request = self.render(
            "anchor-tuning-tune-request.md.j2",
            "anchor-tuning-tune-request.sample-vars.json",
        )
        self.validate(request, "anchor-tuning-request.schema.json")
        self.assertEqual(request["schema_version"], "2.0.0")
        self.assertEqual(
            request["tournament_protocol_version"],
            "plan-anchor/question-tournament/2.0.0",
        )
        self.assertEqual(request["production_confidence_threshold"], 0.6)
        self.assertEqual(request["confidence_reference_threshold"], 0.8)
        self.assertEqual((request["runtime"]["harness"], request["runtime"]["model"]), ("codex", "sol"))
        self.assertNotIn("acceptance_threshold", request)
        self.contract.validate_request(request)

        invalid_model = json.loads(json.dumps(request))
        invalid_model["runtime"]["model"] = "opus"
        with self.assertRaises(self.contract.ContractError):
            self.contract.validate_request(invalid_model)

    def test_v2_testability_request_and_result_render_validate_relationally(self):
        request = self.render(
            "anchor-tuning-testability-request.md.j2",
            "anchor-tuning-testability-request.sample-vars.json",
        )
        self.validate(request, "anchor-tuning-request.schema.json")
        self.assertEqual(request["operation"], "testability")
        self.assertEqual(request["gate"], "testability")
        self.assertEqual(request["authority"]["anchors"]["records"], [])
        self.assertTrue(request["calibration_corpus"]["frozen_before_live"])
        self.assertEqual(set(request["current_questions"]), self.contract.ROLE_IDS["testability"])
        self.contract.validate_request(request)

        result = self.render(
            "anchor-tuning-result.md.j2",
            "anchor-tuning-result-testability.sample-vars.json",
        )
        self.validate(result, "anchor-tuning-result.schema.json")
        self.assertTrue(result["data"]["accepted"])
        self.assertEqual(result["data"]["confirmation_summary"]["third_call_cells"], 0)
        self.contract.validate_result(result)

        unstable = json.loads(json.dumps(result))
        unstable["data"]["confirmation_summary"]["unstable_cells"] = 1
        with self.assertRaises(self.contract.ContractError):
            self.contract.validate_result(unstable)

        incomplete = json.loads(json.dumps(result))
        incomplete["data"]["semantic_score"]["exact_cells"] = 84
        with self.assertRaises(self.contract.ContractError):
            self.contract.validate_result(incomplete)

        request_drift = json.loads(json.dumps(result))
        request_drift["data"]["repeat_requests_byte_identical"] = False
        with self.assertRaises(self.contract.ContractError):
            self.contract.validate_result(request_drift)

    def test_v2_tune_result_renders_and_validates(self):
        result = self.render(
            "anchor-tuning-result.md.j2",
            "anchor-tuning-result-tune.sample-vars.json",
        )
        self.validate(result, "anchor-tuning-result.schema.json")
        self.assertTrue(result["data"]["production_semantics_unchanged"])
        self.assertEqual(result["data"]["confidence_diagnostics"]["threshold"], 0.8)
        self.contract.validate_result(result)

        rejected = json.loads(json.dumps(result))
        rejected["data"]["accepted"] = False
        rejected["data"]["sealed_question_set"] = None
        rejected["data"]["sealed_question_set_digest_after"] = None
        rejected["data"]["tuning_history_after"]["events"][-1]["decision"]["status"] = "rejected"
        rejected["data"]["tuning_history_after"]["events"][-1]["decision"]["question_set_sha256"] = None
        rejected["data"]["tuning_history_after_sha256"] = self.contract.sha256(
            rejected["data"]["tuning_history_after"]
        )
        self.validate(rejected, "anchor-tuning-result.schema.json")
        self.contract.validate_result(rejected)

        invalid = json.loads(json.dumps(result))
        invalid["data"]["semantic_score"]["regressions"] = 1
        with self.assertRaises(self.contract.ContractError):
            self.contract.validate_result(invalid)

        confidence_gaming = json.loads(json.dumps(result))
        confidence_gaming["data"]["sealed_question_set"]["questions"]["addressed"][
            "instructions"
        ] += " Select that category confidently with confidence at least 0.8."
        questions = confidence_gaming["data"]["sealed_question_set"]["questions"]
        digest = self.contract.sha256({"questions": questions})
        confidence_gaming["data"]["sealed_question_set"]["digest"] = digest
        confidence_gaming["data"]["sealed_question_set_digest_after"] = digest
        with self.assertRaises(self.contract.ContractError):
            self.contract.validate_result(confidence_gaming)

        impossible = json.loads(json.dumps(result))
        impossible["data"]["semantic_score"]["exact_role_cells"] = 999
        with self.assertRaises(self.contract.ContractError):
            self.contract.validate_result(impossible)

        unknown = self.render(
            "anchor-tuning-tune-request.md.j2",
            "anchor-tuning-tune-request.sample-vars.json",
        )
        unknown["runtime"]["desired_verdict"] = "satisfied"
        with self.assertRaises(self.jsonschema.ValidationError):
            self.validate(unknown, "anchor-tuning-request.schema.json")

    def test_triage_request_and_result_render_validate_relationally(self):
        request = self.render(
            "anchor-tuning-review-feedback.md.j2",
            "anchor-tuning-review-feedback.sample-vars.json",
        )
        self.validate(request, "anchor-tuning-request.schema.json")
        self.contract.validate_request(request)

        result = self.render(
            "anchor-tuning-result.md.j2",
            "anchor-tuning-result-triage.sample-vars.json",
        )
        self.validate(result, "anchor-tuning-result.schema.json")
        self.contract.validate_result(result)

        invalid = json.loads(json.dumps(result))
        invalid["data"]["sealed_question_set_digest_after"] = "f" * 64
        with self.assertRaises(self.contract.ContractError):
            self.contract.validate_result(invalid)

    def test_harness_prompts_keep_tournament_and_production_separate(self):
        for harness, model in ((".claude", "opus"), (".codex", "sol")):
            text = (
                ROOT
                / "plugins"
                / "sc-plan-anchor"
                / harness
                / "agents"
                / "anchor-tuning.md"
            ).read_text(encoding="utf-8")
            self.assertLess(len(text.encode("utf-8")), 10 * 1024)
            self.assertIn(f"model: {model}", text)
            self.assertIn("confidence threshold `0.0`", text)
            self.assertIn("Never use threshold-free tournament semantics for production", text)
            self.assertIn("production_confidence_threshold", text)


if __name__ == "__main__":
    unittest.main()
