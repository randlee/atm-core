import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

MODULE = Path(__file__).resolve().parents[1] / "post_mortem_context.py"
spec = importlib.util.spec_from_file_location("context", MODULE)
context = importlib.util.module_from_spec(spec)
spec.loader.exec_module(context)


class CollectionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo with spaces"
        self.repo.mkdir()
        subprocess.run(["git", "init", "-q", str(self.repo)], check=True)
        subprocess.run(["git", "-C", str(self.repo), "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "--allow-empty", "-qm", "fixture"], check=True)
        self.head = subprocess.check_output(["git", "-C", str(self.repo), "rev-parse", "HEAD"], text=True).strip()
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
        p.write_text('["atm-f.1", "atm-f2"]')
        self.assertEqual(context.read_ids(p), ["atm-f.1", "atm-f2"])

    def test_cli_returns_nonzero_for_collection_error(self):
        inventory = self.root / "ids.json"
        inventory.write_text('["bad", "good"]')
        args = [str(MODULE), "--inventory", str(inventory), "--repo", str(self.repo), "--commit", "HEAD", "--out", str(self.root / "out")]
        responses = {"bad": (1, "", "not found"), "good": (0, '[{"id":"good"}]', "")}
        with patch("sys.argv", args), patch.object(context.subprocess, "run", side_effect=self.fake(responses)):
            self.assertEqual(context.main(), 1)


if __name__ == "__main__":
    unittest.main()
