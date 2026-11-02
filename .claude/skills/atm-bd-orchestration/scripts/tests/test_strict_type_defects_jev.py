"""Regression tests for concrete type defects found by the BN-2 lane."""

import json
import argparse
import importlib.util
import importlib.machinery
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "sanity-split"
WRAPPER = SCRIPT.parent / "wrap_jev_receipt.py"


def load_module(path):
    loader = importlib.machinery.SourceFileLoader("_bn2_regression_" + path.stem.replace("-", "_"), str(path))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    assert spec is not None
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


class ValidMacClient:
    def verify_receipt(self, value, key):
        return True


class MalformedBeadTests(unittest.TestCase):
    def jev_module(self):
        return load_module(SCRIPT.parent / "jev_client.py")

    def merge_module(self):
        return load_module(SCRIPT.parent / "sanity-merge")

    def split_bead(self, directory, fields):
        bead = Path(directory) / "bead.json"
        bead.write_text(json.dumps({"id": "x-1", "description": "## Deliverables\n1. Example", **fields}))
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--task", "x-1-sanity", "--bead", "x-1",
             "--worktree", directory, "--branch", "fixture", "--commit", "abc",
             "--base", "base", "--lint-command", "true", "--scratch", directory,
             "--bead-json", str(bead), "--split-only"],
            capture_output=True, text=True, check=False,
        )

    def test_manifest_rejects_incorrect_field_types_at_input(self):
        module = self.merge_module()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            good = {"sha": "abc", "branch": "branch", "deliverables_total": 1, "lint": {}}
            for field, value in (("sha", 1), ("branch", []), ("deliverables_total", True), ("run_id", 42)):
                with self.subTest(field=field):
                    path.write_text(json.dumps({**good, field: value}))
                    with self.assertRaises(module.Reject):
                        module.load_manifest(path)

    def test_non_object_metadata_is_reported_without_traceback(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.split_bead(directory, {"metadata": ["invalid"]})
        self.assertEqual(result.returncode, 3)
        self.assertIn("metadata must be an object", result.stderr)
        self.assertNotIn("Traceback", result.stderr)

    def test_wrong_bead_field_types_are_reported_without_traceback(self):
        invalid = (
            ({"title": 7}, "title, description, and notes must be strings"),
            ({"description": ["bad"]}, "title, description, and notes must be strings"),
            ({"notes": ["bad"]}, "title, description, and notes must be strings"),
            ({"metadata": {"sprint_bead": 7}}, "metadata.sprint_bead must be a string"),
            ({"metadata": {"owned_paths": [7]}}, "metadata.owned_paths must be a list of strings"),
        )
        with tempfile.TemporaryDirectory() as directory:
            for fields, message in invalid:
                with self.subTest(fields=fields):
                    result = self.split_bead(directory, fields)
                    self.assertEqual(result.returncode, 3)
                    self.assertIn(message, result.stderr)
                    self.assertNotIn("Traceback", result.stderr)

    def test_receipt_assignment_rejects_non_string_commit(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            assignment = root / "assignment.json"
            assignment.write_text(json.dumps({
                "sanity_bead": "s", "dev_bead": {"id": "d"},
                "deliverable": {"number": 1}, "deliverables_total": 1, "commit": 42,
            }))
            client = root / "client.json"
            client.write_text(json.dumps({"success": False, "data": None,
                                          "error": {"message": "TYPESAFE_API_KEY is missing; no Jev evaluation ran"}}))
            result = subprocess.run(
                [sys.executable, str(WRAPPER), "--assignment", str(assignment), "--client-output", str(client)],
                capture_output=True, text=True, check=False,
            )
        self.assertEqual(result.returncode, 1)
        self.assertIn("assignment_fields", result.stderr)
        self.assertIn("commit must be strings", result.stderr)

    def test_receipt_part_with_non_object_answers_is_refused(self):
        module = load_module(WRAPPER)

        parts = [{"part": "1 of 1", "receipt": {"choice": "yes", "probabilities": {}},
                  "response": {"answers": []}}]
        with self.assertRaises(module.Refused) as refusal:
            module.client_receipts(parts, ValidMacClient(), "test")
        self.assertIn("part_response", str(refusal.exception))

    def test_merge_refuses_non_object_answers_without_traceback(self):
        module = self.merge_module()

        reply = {"success": True, "data": {"jev": [{"part": "1 of 1",
                 "receipt": {"choice": "yes", "probabilities": {}},
                 "response": {"answers": []}}]}}
        with patch.dict(os.environ, {"TYPESAFE_API_KEY": "test"}):
            problem = module.jev_reply_problem(reply, (ValidMacClient(), None), set())
        self.assertIn("response does not match", problem)

    def test_merge_refuses_non_string_request_hash(self):
        module = self.merge_module()

        reply = {"success": True, "data": {"jev": {"question": "written", "choice": "yes",
                 "request_sha256": []}, "findings": []}}
        with patch.dict(os.environ, {"TYPESAFE_API_KEY": "test"}):
            problem = module.jev_reply_problem(reply, (ValidMacClient(), None), set())
        self.assertIn("request hash is not a string", problem)

    def test_merge_refuses_non_string_lint_paths(self):
        module = self.merge_module()
        with self.assertRaises(module.Reject) as refusal:
            module.lint_result({"lint": {"exit_file": [], "log": "log"}})
        self.assertIn("exit_file and log must be strings", str(refusal.exception))

    def test_manifest_refuses_boolean_deliverable_total(self):
        module = self.merge_module()
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / "manifest.json"
            manifest.write_text(json.dumps({"sha": "sha", "branch": "branch", "deliverables_total": True,
                                            "lint": {}}))
            with self.assertRaises(module.Reject) as refusal:
                module.load_manifest(manifest)
            self.assertIn("deliverables_total must be an integer", str(refusal.exception))

    def test_merge_refuses_unhashable_selection(self):
        module = self.merge_module()

        class ReviewerVars(dict):
            def __iter__(self):
                return iter((self["results"], self["digest"]))

        with tempfile.TemporaryDirectory() as directory:
            selection = Path(directory) / "selection.json"
            selection.write_text(json.dumps([{"deliverable": 1, "llm": "done", "jev": "done", "selected": [],
                                              "reason": "", "rerun": None, "llm_sha256": "d", "jev_sha256": "d"}]))
            args = argparse.Namespace(llm_vars="llm", jev_vars="jev", selection=str(selection), client=None)
            vars_ = ReviewerVars(results=[{"success": True}], digest="d")
            with patch.object(module, "load_reviewer_vars", return_value=vars_), \
                 patch.object(module, "load_client", return_value=(None, None)), \
                 patch.object(module, "jev_reply_problem", return_value=None), \
                 patch.object(module, "reply_status", return_value=(1, "done")):
                with self.assertRaises(module.Reject) as refusal:
                    module.selected_results(args, {"deliverables_total": 1}, "s", "d")
            self.assertIn("invalid selection or reason", str(refusal.exception))

    def test_merge_handles_malformed_assignment_metadata(self):
        module = self.merge_module()
        self.assertEqual(module.deliverable_text({"assignments": [{"number": 1, "assignment": []}]}, 1),
                         "Deliverable 1")

    def test_merge_refuses_non_object_manifest(self):
        module = self.merge_module()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "manifest.json"
            path.write_text(json.dumps("sha deliverables_total branch lint"))
            with self.assertRaises(module.Reject) as refusal:
                module.load_manifest(path)
        self.assertIn("manifest must be a JSON object", str(refusal.exception))

    def test_jev_response_rejects_non_object_criteria(self):
        module = self.jev_module()
        with self.assertRaises(module.JevError) as failure:
            module.validate_response({"model": module.MODEL, "answers": {"q": {}}},
                                     {"questions": {"q": {"criteria": None}}})
        self.assertEqual(failure.exception.code, "SANITY.JEV_RESPONSE_INVALID")

    def test_jev_receipt_rejects_missing_questions(self):
        module = self.jev_module()
        with self.assertRaises(module.JevError) as failure:
            module.receipt({"questions": []}, {"answers": {}}, "test")
        self.assertEqual(failure.exception.code, "SANITY.JEV_RESPONSE_INVALID")

    def test_jev_escalations_ignore_non_array_recipients(self):
        module = self.jev_module()
        output = subprocess.CompletedProcess(["atm"], 0, stdout='{"recipients": "xy"}', stderr="")
        with patch.object(module.subprocess, "run", return_value=output):
            self.assertEqual(module.escalation_recipients(), [])

    def test_jev_startup_handles_non_object_answers(self):
        module = self.jev_module()
        args = argparse.Namespace(startup=True, request=None, assignment=None)
        with patch.object(module, "evaluate", return_value={"answers": []}):
            result = module.evaluation_result(args, clock=lambda: 0.0)
        self.assertFalse(result["success"])
        self.assertEqual(result["error"]["code"], "SANITY.JEV_RESPONSE_INVALID")

    def test_jev_split_retry_rejects_non_object_state(self):
        module = self.jev_module()
        request = {"state": "not-an-object"}
        oversized = module.JevError("SANITY.JEV_REQUEST_TOO_LARGE", "too large", True)
        with patch.object(module, "validate_request"), patch.object(module, "evaluate", side_effect=oversized):
            with self.assertRaises(module.JevError) as failure:
                module.evaluate_parts([request], "test")
        self.assertEqual(failure.exception.code, "SANITY.JEV_REQUEST_TOO_LARGE")

    def test_jev_parts_rejects_non_object_state(self):
        module = self.jev_module()
        with self.assertRaises(module.JevError) as failure:
            module.parts([{"state": []}], [{}], "test")
        self.assertEqual(failure.exception.code, "VALIDATION.INPUT")

    def test_jev_parts_rejects_missing_part_fields(self):
        module = self.jev_module()
        with self.assertRaises(module.JevError) as failure:
            module.parts([{"state": {}}], [{}], "test")
        self.assertEqual(failure.exception.code, "VALIDATION.INPUT")

    def test_jev_external_evidence_rejects_unhashable_kind(self):
        module = self.jev_module()
        with self.assertRaises(module.JevError) as failure:
            module.external_evidence({"kind": []}, ".")
        self.assertEqual(failure.exception.code, "VALIDATION.INPUT")

    def test_wrapper_rejects_boolean_deliverable_total(self):
        module = load_module(WRAPPER)
        with self.assertRaises(module.Refused):
            module.assignment_fields({"sanity_bead": "s", "dev_bead": {"id": "d"},
                                      "deliverable": {"number": 1}, "deliverables_total": True, "commit": "abc"})

    def test_wrapper_rejects_client_without_receipt_operations(self):
        module = load_module(WRAPPER)
        with tempfile.TemporaryDirectory() as directory:
            client = Path(directory) / "client.py"
            client.write_text("value = 1\n")
            with patch.object(module, "CLIENT", client):
                with self.assertRaises(module.Refused):
                    module.load_client()

    def test_merge_rejects_client_without_receipt_operations(self):
        module = self.merge_module()
        with tempfile.TemporaryDirectory() as directory:
            client = Path(directory) / "client.py"
            client.write_text("value = 1\n")
            loaded, error = module.load_client(client)
        self.assertIsNone(loaded)
        self.assertIn("receipt operations", error)
