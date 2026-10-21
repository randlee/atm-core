"""Phase authority, complete template validation and the planner's writer."""
from __future__ import annotations

import copy
import os
import sys
import tempfile
import unittest
from pathlib import Path

from phase_fixtures import TEMPLATE_DIRECTORY, TEMPLATE_NAMES, phase_data, phase_toml, run_subprocess, write_phase

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
from sprint_index_common import load_phase_config, validate_phase_config


class PhaseConfigTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.repo = Path(self.tmp.name)
        run_subprocess(["git", "init", "-q", str(self.repo)], check=True)
        self.path = write_phase(self.repo, lead="appointed-lead")
        self.data = phase_data(lead="appointed-lead")

    def write(self, data):
        self.path.write_text(phase_toml(data), encoding="utf-8")

    def test_explicit_lead_and_all_installed_templates(self):
        self.assertEqual(load_phase_config(self.repo, "t"), self.data)
        self.assertEqual(set(self.data["templates"]), set(TEMPLATE_NAMES))
        for relative in self.data["templates"].values():
            self.assertTrue((self.repo / relative).is_file())

    def test_optional_code_checkout_is_retained(self):
        data = copy.deepcopy(self.data)
        data["code_checkout"] = "../atm-bd-orchestration"
        self.write(data)
        self.assertEqual(load_phase_config(self.repo, "t"), data)

    def test_code_checkout_must_be_a_nonempty_string_when_present(self):
        for value in ("", " \t", 42, "/absolute/path"):
            with self.subTest(value=value):
                data = copy.deepcopy(self.data)
                data["code_checkout"] = value
                self.write(data)
                with self.assertRaisesRegex(RuntimeError, "code_checkout: optional repository-relative path"):
                    load_phase_config(self.repo, "t")

    def test_every_missing_blank_or_non_string_value_names_its_key(self):
        for key in ("plan", "root", "integration_branch", "lead", *TEMPLATE_NAMES):
            for value in (None, "", " \t", 42):
                with self.subTest(key=key, value=value):
                    data = copy.deepcopy(self.data)
                    target = data["templates"] if key in TEMPLATE_NAMES else data
                    if value is None:
                        target.pop(key)
                    else:
                        target[key] = value
                    self.write(data)
                    with self.assertRaisesRegex(RuntimeError, f"{key}: required") as caught:
                        load_phase_config(self.repo, "t")
                    self.assertIn(str(self.path), str(caught.exception))

    def test_each_dangling_template_path_names_its_key_and_path(self):
        for key in TEMPLATE_NAMES:
            with self.subTest(key=key):
                data = copy.deepcopy(self.data)
                data["templates"][key] = str(TEMPLATE_DIRECTORY / f"missing-{key}.xml.j2")
                self.write(data)
                with self.assertRaisesRegex(RuntimeError, f"templates.{key}:.*missing-{key}.*existing template"):
                    load_phase_config(self.repo, "t")

    def test_paths_must_stay_inside_the_installed_template_directory(self):
        outside = self.repo / "outside.xml.j2"
        outside.write_text("outside")
        escaped = self.repo / TEMPLATE_DIRECTORY / "escaped.xml.j2"
        escaped.symlink_to(outside)
        for value in (str(self.repo / self.data["templates"]["dev"]), "outside.xml.j2",
                      str(TEMPLATE_DIRECTORY / "../templates/dev-template.xml.j2"),
                      str(TEMPLATE_DIRECTORY / "escaped.xml.j2"), str(TEMPLATE_DIRECTORY)):
            with self.subTest(value=value):
                data = copy.deepcopy(self.data)
                data["templates"]["dev"] = value
                self.write(data)
                with self.assertRaisesRegex(RuntimeError, "templates.dev"):
                    load_phase_config(self.repo, "t")

    def test_each_template_rejects_file_paths_with_directory_suffixes(self):
        for key in TEMPLATE_NAMES:
            for suffix in ("/", "/."):
                with self.subTest(key=key, suffix=suffix):
                    data = copy.deepcopy(self.data)
                    data["templates"][key] += suffix
                    self.write(data)
                    with self.assertRaisesRegex(RuntimeError, f"templates.{key}") as caught:
                        load_phase_config(self.repo, "t")
                    self.assertIn(data["templates"][key], str(caught.exception))

    def test_both_readers_require_lead_and_templates_by_default(self):
        for key in ("lead", "templates"):
            with self.subTest(key=key):
                data = copy.deepcopy(self.data)
                data.pop(key)
                self.write(data)
                with self.assertRaisesRegex(RuntimeError, f"{key}: required"):
                    load_phase_config(self.repo, "t")
                with self.assertRaisesRegex(RuntimeError, f"{key}: required"):
                    validate_phase_config(self.repo, "t", data)

    def test_unknown_top_level_key_fails_in_both_readers(self):
        data = copy.deepcopy(self.data)
        data["unexpected"] = "value"
        self.write(data)
        with self.assertRaisesRegex(RuntimeError, "unknown key.*unexpected"):
            load_phase_config(self.repo, "t")
        with self.assertRaisesRegex(RuntimeError, "unknown key.*unexpected"):
            validate_phase_config(self.repo, "t", data)

    def test_unknown_template_keys_and_retired_aliases_fail(self):
        for key in ("dev_assign", "fix_review", "sanity_assign", "qa_assign", "unknown"):
            with self.subTest(key=key):
                data = copy.deepcopy(self.data)
                data["templates"][key] = data["templates"]["dev"]
                self.write(data)
                with self.assertRaisesRegex(RuntimeError, f"unknown key.*{key}"):
                    load_phase_config(self.repo, "t")

    def test_symlink_loop_names_the_template_key(self):
        target = self.repo / TEMPLATE_DIRECTORY / "loop.xml.j2"
        target.symlink_to(target)
        data = copy.deepcopy(self.data)
        data["templates"]["dev"] = str(TEMPLATE_DIRECTORY / "loop.xml.j2")
        self.write(data)
        with self.assertRaisesRegex(RuntimeError, "templates.dev") as caught:
            load_phase_config(self.repo, "t")
        self.assertIn(str(self.path), str(caught.exception))

    def test_missing_lead_has_no_environment_or_repository_fallback(self):
        data = copy.deepcopy(self.data)
        data.pop("lead")
        self.write(data)
        config = self.repo / ".claude/project/atm-bd-orchestration.yaml"
        config.parent.mkdir(parents=True)
        config.write_text("lead: repository-fallback\n")
        code = ("from pathlib import Path; from sprint_index_common import load_phase_config; "
                "load_phase_config(Path('.'), 't')")
        result = run_subprocess([sys.executable, "-c", code], cwd=self.repo, capture_output=True, text=True,
                                env={**os.environ, "PYTHONPATH": str(SCRIPTS), "ATM_IDENTITY": "env-fallback"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("lead: required", result.stderr)

    def init(self, *arguments):
        return run_subprocess([sys.executable, str(SCRIPTS / "phase-config"), "init", *arguments],
                              cwd=self.repo, capture_output=True, text=True)

    def configure_repo(self):
        config = self.repo / ".claude/project/atm-bd-orchestration.yaml"
        config.parent.mkdir(parents=True)
        config.write_text("bead_prefix: chosen\nplans_dir: work/plans\nintegration_branch_pattern: custom/{phase}\n")

    def test_init_writes_explicit_lead_and_six_defaults_without_overwrite(self):
        self.configure_repo()
        result = self.init("--phase", "new", "--lead", "new-lead")
        self.assertEqual(result.returncode, 0, result.stderr)
        expected = phase_data("new", "chosen", "work/plans", "new-lead")
        expected["integration_branch"] = "custom/new"
        self.assertEqual(load_phase_config(self.repo, "new"), expected)
        path = self.repo / ".atm-bd/phase-new.toml"
        original = path.read_bytes()
        result = self.init("--phase", "new", "--lead", "replacement")
        self.assertEqual(result.returncode, 2)
        self.assertIn("already exists", result.stderr)
        self.assertEqual(path.read_bytes(), original)

    def test_legacy_dev_fix_is_ignored_even_if_its_file_is_missing(self):
        data = copy.deepcopy(self.data)
        data["templates"]["dev_fix"] = "deleted-dev-fix.xml.j2"
        self.write(data)
        self.assertEqual(load_phase_config(self.repo, "t"), data)

    def test_init_requires_lead_and_installed_templates_before_writing(self):
        self.configure_repo()
        for args in (("--phase", "new"), ("--phase", "new", "--lead", " ")):
            result = self.init(*args)
            self.assertEqual(result.returncode, 2)
            self.assertFalse((self.repo / ".atm-bd/phase-new.toml").exists())
        (self.repo / self.data["templates"]["qa"]).unlink()
        result = self.init("--phase", "new", "--lead", "new-lead")
        self.assertEqual(result.returncode, 2)
        self.assertIn("templates.qa", result.stderr)
        self.assertFalse((self.repo / ".atm-bd/phase-new.toml").exists())


if __name__ == "__main__":
    unittest.main()
