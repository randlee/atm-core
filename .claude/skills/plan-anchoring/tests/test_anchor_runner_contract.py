import importlib.util
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[5]
SKILL = ROOT / "plugins" / "sc-plan-anchor" / "skills" / "plan-anchoring"
MODULE_PATH = SKILL / "scripts" / "anchor_runner_contract.py"


def load_contract():
    spec = importlib.util.spec_from_file_location("anchor_runner_contract", MODULE_PATH)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class RunnerContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.contract = load_contract()

    def plugin(self, root: Path, harness: str = "codex", *, model: str = "luna", version: str = "0.5.0") -> Path:
        agent_dir = root / f".{harness}" / "agents"
        agent_dir.mkdir(parents=True)
        definition = agent_dir / "anchor-plan-review.md"
        definition.write_text(
            "---\n"
            "name: anchor-plan-review\n"
            f"version: {version}\n"
            f"model: {model}\n"
            "tools: Bash, Read\n"
            "---\n\n# Agent\n",
            encoding="utf-8",
        )
        (agent_dir / "registry.yaml").write_text(
            "agents:\n"
            "  anchor-plan-review:\n"
            f"    version: {version}\n"
            f"    path: .{harness}/agents/anchor-plan-review.md\n",
            encoding="utf-8",
        )
        return definition

    def request(self, runtime: dict) -> dict:
        return {
            "schema_version": "1.0.0",
            "runtime": runtime,
            "operation": "plan_review",
            "correlation_id": "phase-a:A1",
            "params": {"source": "markdown"},
        }

    def result(self, *, success: bool = True) -> dict:
        return {
            "schema_version": "1.0.0",
            "success": success,
            "canceled": False,
            "aborted_by": None,
            "data": {} if success else None,
            "error": None if success else {
                "code": "JEV.TIMEOUT",
                "message": "request timed out",
                "recoverable": True,
                "suggested_action": "retry once",
            },
            "metadata": {"correlation_id": "phase-a:A1"},
        }

    def test_resolves_real_claude_and_codex_registries(self):
        plugin = ROOT / "plugins" / "sc-plan-anchor"
        claude = self.contract.resolve_agent(plugin, "claude", "anchor-plan-review")
        codex = self.contract.resolve_agent(plugin, "codex", "anchor-plan-review")
        self.assertEqual((claude["model"], codex["model"]), ("sonnet", "luna"))
        self.assertRegex(claude["sha256"], r"^[0-9a-f]{64}$")

    def test_rejects_traversal_symlink_nonregular_and_identity_mismatches(self):
        cases = []
        with tempfile.TemporaryDirectory() as td:
            root = Path(td) / "plugin"
            definition = self.plugin(root)
            registry = root / ".codex" / "agents" / "registry.yaml"

            outside = Path(td) / "outside.md"
            outside.write_text(definition.read_text(encoding="utf-8"), encoding="utf-8")
            registry.write_text(
                "agents:\n  anchor-plan-review:\n    version: 0.5.0\n    path: ../outside.md\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(self.contract.RunnerContractError, "untrusted|resolve"):
                self.contract.resolve_agent(root, "codex", "anchor-plan-review")

        with tempfile.TemporaryDirectory() as td:
            root = Path(td) / "plugin"
            definition = self.plugin(root)
            real = definition.with_suffix(".real")
            definition.rename(real)
            definition.symlink_to(real)
            with self.assertRaisesRegex(self.contract.RunnerContractError, "symlink"):
                self.contract.resolve_agent(root, "codex", "anchor-plan-review")

        for bad_line, code in (
            ("name: other", "RUNTIME.IDENTITY"),
            ("version: 9.9.9", "RUNTIME.VERSION"),
            ("model: sol", "RUNTIME.MODEL"),
        ):
            with tempfile.TemporaryDirectory() as td:
                root = Path(td) / "plugin"
                definition = self.plugin(root)
                text = definition.read_text(encoding="utf-8")
                key = bad_line.split(":", 1)[0]
                text = text.replace(f"{key}: " + {"name": "anchor-plan-review", "version": "0.5.0", "model": "luna"}[key], bad_line)
                definition.write_text(text, encoding="utf-8")
                with self.assertRaises(self.contract.RunnerContractError) as caught:
                    self.contract.resolve_agent(root, "codex", "anchor-plan-review")
                self.assertEqual(caught.exception.code, code)

        with tempfile.TemporaryDirectory() as td:
            root = Path(td) / "plugin"
            definition = self.plugin(root)
            definition.unlink()
            definition.mkdir()
            with self.assertRaises(self.contract.RunnerContractError) as caught:
                self.contract.resolve_agent(root, "codex", "anchor-plan-review")
            self.assertEqual(caught.exception.code, "RUNTIME.PATH")

    def test_detects_hash_change_after_resolution(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td) / "plugin"
            definition = self.plugin(root)
            runtime = self.contract.resolve_agent(root, "codex", "anchor-plan-review")
            definition.write_text(definition.read_text(encoding="utf-8") + "changed\n", encoding="utf-8")
            with self.assertRaises(self.contract.RunnerContractError) as caught:
                self.contract.verify_resolved_agent(root, runtime)
            self.assertEqual(caught.exception.code, "RUNTIME.HASH")

    def test_request_and_result_require_exactly_one_json_fence(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td) / "plugin"
            self.plugin(root)
            runtime = self.contract.resolve_agent(root, "codex", "anchor-plan-review")
            request = self.request(runtime)
            fenced = "```json\n" + json.dumps(request) + "\n```"
            self.assertEqual(self.contract.parse_request(fenced), request)
            for invalid in (fenced + "\n" + fenced, "prefix\n" + fenced, "```json\n{bad}\n```"):
                with self.assertRaises(self.contract.RunnerContractError):
                    self.contract.parse_request(invalid)
            duplicate = fenced.replace('"schema_version": "1.0.0",', '"schema_version": "1.0.0", "schema_version": "1.0.0",')
            with self.assertRaises(self.contract.RunnerContractError) as caught:
                self.contract.parse_request(duplicate)
            self.assertEqual(caught.exception.code, "ENVELOPE.DUPLICATE_KEY")

            result = self.result()
            result_fence = "```json\n" + json.dumps(result) + "\n```"
            self.assertEqual(self.contract.parse_result(result_fence, "phase-a:A1"), result)
            with self.assertRaises(self.contract.RunnerContractError):
                self.contract.parse_result(result_fence, "other")

            unknown = dict(request)
            unknown["extra"] = True
            with self.assertRaises(self.contract.RunnerContractError):
                self.contract.validate_request(unknown)

    def test_attestation_is_redacted_and_atomic_failures_are_typed(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td) / "plugin"
            self.plugin(root)
            runtime = self.contract.resolve_agent(root, "codex", "anchor-plan-review")
            attestation = self.contract.build_attestation(
                runtime,
                "plan_review",
                "phase-a:A1",
                started_at_ms=10,
                finished_at_ms=25,
                outcome="timeout",
                source_sha256="a" * 64,
                error_code="JEV.TIMEOUT",
            )
            rendered = json.dumps(attestation)
            self.assertNotIn("sk-canary", rendered)
            self.assertNotIn("Authorization", rendered)
            destination = Path(td) / "runs" / "attestation.json"
            self.contract.atomic_write_json(destination, attestation)
            self.assertEqual(json.loads(destination.read_text(encoding="utf-8")), attestation)
            with mock.patch.object(self.contract.os, "replace", side_effect=OSError("canary secret")):
                with self.assertRaises(self.contract.RunnerContractError) as caught:
                    self.contract.atomic_write_json(destination, attestation)
            self.assertEqual(caught.exception.code, "ATTESTATION.WRITE")
            self.assertNotIn("canary", caught.exception.message)

    def test_sc_compose_invocation_has_one_valid_fenced_request(self):
        binary = shutil.which("sc-compose")
        self.assertIsNotNone(binary, "sc-compose is required for contract rendering")
        with tempfile.TemporaryDirectory() as td:
            output = Path(td) / "invocation.md"
            subprocess.run(
                [
                    binary,
                    "render",
                    "--mode",
                    "file",
                    "--file",
                    str(SKILL / "templates" / "agent-invocation.md.j2"),
                    "--var-file",
                    str(ROOT / "docs" / "plans" / "phase-a" / "templates" / "agent-invocation.sample-vars.json"),
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
            text = output.read_text(encoding="utf-8")
            blocks = re.findall(r"```json\s*(.*?)\s*```", text, re.S)
            self.assertEqual(len(blocks), 1)
            request = json.loads(blocks[0])
            self.contract.validate_request(request)


if __name__ == "__main__":
    unittest.main()
