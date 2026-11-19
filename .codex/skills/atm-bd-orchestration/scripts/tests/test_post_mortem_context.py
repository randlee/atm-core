import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from subprocess_helpers import (
    isolated_git_env,
    run_command,
    run_fixture_git,
    set_fixture_hooks_path,
)

MODULE = Path(__file__).resolve().parents[1] / "post_mortem_context.py"
spec = importlib.util.spec_from_file_location("context", MODULE)
assert spec is not None and spec.loader is not None
context = importlib.util.module_from_spec(spec)
spec.loader.exec_module(context)


class CollectionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo with spaces"
        self.repo.mkdir()
        self.git_env, _ = isolated_git_env(self.root)
        environ = patch.dict(os.environ, self.git_env)
        environ.start()
        self.addCleanup(environ.stop)
        run_fixture_git(["git", "init", "-q", str(self.repo)], env=self.git_env)
        set_fixture_hooks_path(self.repo, env=self.git_env)
        run_fixture_git(["git", "-C", str(self.repo), "-c", "user.name=Test", "-c",
                         "user.email=test@example.invalid", "commit", "--allow-empty", "-qm", "fixture"],
                        env=self.git_env)
        self.head = run_fixture_git(["git", "-C", str(self.repo), "rev-parse", "HEAD"],
                                    env=self.git_env).stdout.strip()
        self.real_run = subprocess.run
        self.calls = []

    def fake(self, responses):
        def run(args, **kwargs):
            if args[0] != "bd":
                return self.real_run(args, **kwargs)
            self.calls.append((args, kwargs))
            response = responses[args[2]]
            if isinstance(response, Exception):
                raise response
            return subprocess.CompletedProcess(args, *response)
        return run

    def test_fixture_commit_ignores_hostile_git_config_and_hooks(self):
        hostile_root = self.root / "hostile"
        hooks = hostile_root / "hooks"
        template = hostile_root / "template"
        hooks.mkdir(parents=True)
        template.mkdir(parents=True)
        marker = hostile_root / "hook-ran"
        for hook_name in ("pre-commit", "commit-msg"):
            hook = hooks / hook_name
            hook.write_text(f"#!/bin/sh\nprintf invoked > {marker}\nexit 1\n")
            hook.chmod(0o755)
        global_config = hostile_root / "global.gitconfig"
        global_config.write_text(
            f"[init]\n\ttemplateDir = {template}\n[core]\n\thooksPath = {hooks}\n"
            "[commit]\n\tgpgsign = true\n"
        )
        hostile = {
            **os.environ,
            "HOME": str(hostile_root / "home"),
            "GIT_CONFIG_GLOBAL": str(global_config),
            "GIT_CONFIG_SYSTEM": str(global_config),
            "GIT_TEMPLATE_DIR": str(template),
            "GIT_CONFIG_COUNT": "1",
            "GIT_CONFIG_KEY_0": "core.hooksPath",
            "GIT_CONFIG_VALUE_0": str(hooks),
        }
        fixture_root = hostile_root / "fixture"
        fixture_root.mkdir()
        repository = fixture_root / "repo"
        with patch.dict(os.environ, hostile, clear=True):
            env, _ = isolated_git_env(fixture_root)
            self.assertNotIn("GIT_CONFIG_COUNT", env)
            self.assertFalse(any(key.startswith(("GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_"))
                                 for key in env))
            run_fixture_git(["git", "init", "-q", str(repository)], env=env)
            set_fixture_hooks_path(repository, env=env)
            run_fixture_git(["git", "-C", str(repository), "config", "user.name", "fixture"], env=env)
            run_fixture_git(["git", "-C", str(repository), "config", "user.email", "fixture@example.invalid"], env=env)
            (repository / "fixture.txt").write_text("isolated\n")
            run_fixture_git(["git", "-C", str(repository), "add", "fixture.txt"], env=env)
            run_fixture_git(["git", "-C", str(repository), "commit", "-m", "fixture"], env=env)
            commit = run_fixture_git(["git", "-C", str(repository), "rev-parse", "HEAD"], env=env).stdout.strip()
        self.assertEqual(len(commit), 40)
        self.assertFalse(marker.exists())

        targets = [
            "test_sanity_merge.py::test_selected_fallback_errors_are_not_counted_twice",
            "test_handoff_contracts.py::DevSanityHandoffTests::test_rendered_assignment_has_exactly_the_reviewers_input_fields",
            "test_close_vars.py::test_rebased_fixture_layer_updates_qa_vars_and_sanity_close_context",
            "test_strict_type_defects_report.py::MissingLimitationsTests::test_missing_limitations_is_rejected_at_prepare_and_evaluation_boundaries",
        ]
        result = run_command(
            [sys.executable, "-m", "pytest", "-q", *targets],
            cwd=Path(__file__).resolve().parent,
            env=hostile,
            timeout=120,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}")
        self.assertFalse(marker.exists())

    def test_pinned_full_beads_safe_argv_and_review_prompt(self):
        bead = {"id": "atm-a.1", "description": "$(touch nope)", "notes": "all notes retained", "dependencies": [{"id": "prior"}]}
        with patch.object(context.subprocess, "run", side_effect=self.fake({"atm-a.1": (0, json.dumps([bead]), "")})):
            result = context.collect(["atm-a.1"], self.repo, "HEAD", self.root / "out")
        row = result["findings"][0]
        self.assertEqual(result["integration_sha"], self.head)
        self.assertEqual(json.loads(Path(row["bead_path"]).read_text()), bead)
        self.assertEqual(self.calls[0][0], ["bd", "show", "atm-a.1", "--json"])
        self.assertNotIn("shell", self.calls[0][1])
        prompt = Path(row["prompt_path"]).read_text()
        self.assertIn(self.head, prompt)
        self.assertIn(context.PREPARATION_REFERENCE, prompt)
        self.assertIn("Do not launch evaluation", prompt)
        self.assertFalse(Path(row["manifest_output_path"]).exists())
        self.assertTrue(result["timestamp_utc"].endswith("Z"))

    def test_failures_retained_and_later_ids_collected(self):
        responses = {"bad-command": (1, "", "bd unavailable"), "bad-json": (0, "not json", ""),
                     "wrong-id": (0, '[{"id":"different"}]', ""), "good": (0, '[{"id":"good"}]', "")}
        with patch.object(context.subprocess, "run", side_effect=self.fake(responses)):
            result = context.collect(list(responses), self.repo, self.head, self.root / "out")
        self.assertEqual([r["status"] for r in result["findings"]], ["error", "error", "error", "collected"])
        self.assertEqual(result["requested_ids"], list(responses))
        self.assertEqual(len(json.loads((self.root / "out/collection.json").read_text())["findings"]), 4)

    def test_missing_bd_executable_is_per_finding_error(self):
        with patch.object(context.subprocess, "run", side_effect=self.fake({"one": FileNotFoundError("bd")})):
            result = context.collect(["one"], self.repo, "HEAD", self.root / "out")
        self.assertEqual(result["findings"][0]["error"]["type"], "FileNotFoundError")

    def test_reused_output_refused(self):
        out = self.root / "out"
        out.mkdir()
        (out / "prior").write_text("retained")
        with self.assertRaises(FileExistsError):
            context.collect(["one"], self.repo, "HEAD", out)
        self.assertEqual((out / "prior").read_text(), "retained")

    def test_invalid_commit_does_not_fetch_beads(self):
        with patch.object(context.subprocess, "run", side_effect=self.fake({})):
            with self.assertRaises(RuntimeError):
                context.collect(["one"], self.repo, "missing-ref", self.root / "out")
        self.assertFalse(self.calls)
        self.assertFalse((self.root / "out").exists())

    def test_inventory_rejects_duplicates_and_path_injection(self):
        p = self.root / "inventory.json"
        for value in [[], {}, ["a", "a"], ["../outside"], ["-x"], ["a; touch nope"], [1]]:
            with self.subTest(value=value):
                p.write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    context.read_ids(p)
        p.write_text('["p-f.1", "p-f2"]')
        self.assertEqual(context.read_ids(p), ["p-f.1", "p-f2"])

    def test_cli_returns_nonzero_for_collection_error(self):
        inventory = self.root / "ids.json"
        inventory.write_text('["bad", "good"]')
        args = [str(MODULE), "--inventory", str(inventory), "--repo", str(self.repo), "--commit", "HEAD", "--out", str(self.root / "out")]
        responses = {"bad": (1, "", "not found"), "good": (0, '[{"id":"good"}]', "")}
        with patch("sys.argv", args), patch.object(context.subprocess, "run", side_effect=self.fake(responses)):
            self.assertEqual(context.main(), 1)


if __name__ == "__main__":
    unittest.main()
