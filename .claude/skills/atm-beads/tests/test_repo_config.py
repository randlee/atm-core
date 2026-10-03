"""repo_config.load() and the run-time values sprint_index_common takes from it."""
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
import repo_config  # noqa: E402
import sprint_index_common  # noqa: E402


def make_repo(base: Path, config: str | None) -> Path:
    repo = base / "repo"
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    if config is not None:
        path = repo / repo_config.RELATIVE_PATH
        path.parent.mkdir(parents=True)
        path.write_text(config)
    return repo


class LoadTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.base = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_load_returns_the_file_as_a_dict(self):
        repo = make_repo(self.base, "bead_prefix: myp\nreviewers_round1: [a, b]\n")
        config = repo_config.load(repo)
        self.assertIsInstance(config, dict)
        self.assertEqual(config["bead_prefix"], "myp")
        self.assertEqual(config["reviewers_round1"], ["a", "b"])

    def test_missing_key_is_a_named_error(self):
        repo = make_repo(self.base, "bead_prefix: myp\n")
        with self.assertRaises(repo_config.ConfigKeyMissing) as caught:
            repo_config.load(repo)["plans_dir"]
        self.assertIn("'plans_dir'", str(caught.exception))
        self.assertIn(str(repo_config.RELATIVE_PATH), str(caught.exception))

    def test_missing_file_is_a_named_error(self):
        repo = make_repo(self.base, None)
        with self.assertRaises(repo_config.ConfigNotFound) as caught:
            repo_config.load(repo)
        self.assertIn(str(repo_config.RELATIVE_PATH), str(caught.exception))

    def test_non_mapping_is_rejected(self):
        repo = make_repo(self.base, "- a\n- b\n")
        with self.assertRaises(repo_config.ConfigError):
            repo_config.load(repo)

    def test_cli_get_prints_scalars_plainly_and_lists_as_json(self):
        repo = make_repo(self.base, "lead: my-lead\nreviewers_round1: [req-x, arch-x]\n")
        run = lambda *a: subprocess.run([sys.executable, str(SCRIPTS / "repo_config.py"), *a], cwd=repo,
                                        capture_output=True, text=True)
        self.assertEqual(run("get", "lead").stdout, "my-lead\n")
        self.assertEqual(json.loads(run("get", "reviewers_round1").stdout), ["req-x", "arch-x"])
        missing = run("get", "test_command")
        self.assertEqual(missing.returncode, 2)
        self.assertIn("required key 'test_command' is missing", missing.stderr)


class RunTimeValuesTests(unittest.TestCase):
    """sprint_index_common needs no install-time rendering: prefix and plan directory are read at run time."""

    PLAN = '["x-1", "zz-x-1-sanity", []]\n["x-2", "zz-x-2-sanity", ["x-1"]]\n'

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.repo = make_repo(Path(self.tmp.name), "bead_prefix: cfgp\nplans_dir: work/plans\n")
        self.plan = self.repo / "work/plans/phase-x/sprints.jsonl"
        self.plan.parent.mkdir(parents=True)
        self.plan.write_text(self.PLAN)

    def tearDown(self):
        self.tmp.cleanup()

    def test_the_root_id_supplies_the_prefix(self):
        index = sprint_index_common.load_phase_plan(self.plan, "rootp-phase-x")
        self.assertEqual(index["root_bead_id"], "rootp-phase-x")
        self.assertEqual([r["dev_bead_id"] for r in index["sprints"]], ["rootp-x-1", "rootp-x-2"])

    def test_without_a_root_the_configured_prefix_is_used(self):
        index = sprint_index_common.load_phase_plan(self.plan)
        self.assertEqual(index["root_bead_id"], "cfgp-phase-x")
        self.assertEqual([r["dev_bead_id"] for r in index["sprints"]], ["cfgp-x-1", "cfgp-x-2"])

    def test_a_root_id_that_is_not_a_phase_root_is_rejected(self):
        with self.assertRaises(RuntimeError):
            sprint_index_common.load_phase_plan(self.plan, "not-a-root")

    def test_phase_path_uses_the_configured_plans_dir(self):
        self.assertEqual(sprint_index_common.phase_path(self.repo, "x"), self.plan)

    def test_the_source_needs_no_install_rendering(self):
        self.assertNotIn("{{", (SCRIPTS / "sprint_index_common.py").read_text())


if __name__ == "__main__":
    unittest.main()
