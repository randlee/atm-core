"""sanity-split renders one assignment per numbered deliverable of a pinned, pushed commit."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

SCRIPTS = Path(__file__).parents[1]
SCRIPT = SCRIPTS / "sanity-split"
MERGE = SCRIPTS / "sanity-merge"
FAKE_LINT = "sh -c 'echo lint-ran; exit 1'"

DESCRIPTION = """## Goal

Retry helper.

## Deliverables

1. Add `pub mod retry` with `pub fn is_retryable(status: u16) -> bool`
   in the types crate.
2. Unit tests for 429, 503 and 404.

## Non-closure

Nothing else.
"""


def bead(description=DESCRIPTION, owned=("crates/types/src/**",)):
    return [{"id": "atm-x-1", "title": "x-1: retry", "description": description, "design": "## Design\n\nplain",
             "acceptance_criteria": "- true for 429\n- tests", "metadata": {"owned_paths": list(owned)}}]


def git(cwd, *argv):
    return subprocess.run(["git", "-C", str(cwd), *argv], check=True, capture_output=True, text=True).stdout.strip()


def result(number, sha, findings=()):
    return {"success": True, "error": None, "data": {
        "sanity_bead": "atm-x-1-sanity", "dev_bead": "atm-x-1", "deliverable": number, "commit_checked": sha,
        "findings": list(findings)}}


class Repo:
    """A clone with a bare origin: develop pushed, sprint/x pushed with one change."""

    def __init__(self, root):
        self.origin = root / "origin.git"
        self.wt = root / "wt"
        subprocess.run(["git", "init", "-q", "--bare", "-b", "develop", str(self.origin)], check=True)
        subprocess.run(["git", "clone", "-q", str(self.origin), str(self.wt)], check=True, capture_output=True)
        git(self.wt, "config", "user.email", "t@example.com")
        git(self.wt, "config", "user.name", "t")
        git(self.wt, "checkout", "-q", "-b", "develop")
        self.commit("crates/types/src/lib.rs", "mod query;\n", "base")
        self.commit("docs/notes.md", "old notes\n", "notes")
        git(self.wt, "push", "-q", "-u", "origin", "develop")
        git(self.wt, "checkout", "-q", "-b", "sprint/x")
        self.commit("crates/types/src/retry.rs", "pub fn is_retryable(s: u16) -> bool { s == 429 }\n", "retry")
        git(self.wt, "push", "-q", "-u", "origin", "sprint/x")
        self.sha = git(self.wt, "rev-parse", "HEAD")
        self.base_sha = git(self.wt, "rev-parse", "origin/develop")

    def commit(self, path, text, message):
        target = self.wt / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
        git(self.wt, "add", "-A")
        git(self.wt, "commit", "-q", "-m", message)

    def push_head(self):
        git(self.wt, "push", "-q", "origin", "sprint/x")
        self.sha = git(self.wt, "rev-parse", "HEAD")
        return self.sha


class SanitySplit(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root, ignore_errors=True)
        self.repo = Repo(self.root)
        self.scratch = self.root / "scratch"

    def run_split(self, bead_json=None, worktree=None, commit=None, branch="sprint/x", base="develop", *extra,
                  lint=FAKE_LINT):
        bead_file = self.root / "bead.json"
        bead_file.write_text(json.dumps(bead_json if bead_json is not None else bead()))
        argv = [str(SCRIPT), "--task", "atm-x-1-sanity", "--bead", "atm-x-1", "--worktree", str(worktree or self.repo.wt),
                "--branch", branch, "--commit", commit or self.repo.sha, "--base", base, "--lint-command", lint,
                "--scratch", str(self.scratch), "--bead-json", str(bead_file), *extra]
        return subprocess.run(argv, capture_output=True, text=True)

    def wait_lint(self, exit_file, seconds=10):
        for _ in range(int(seconds / 0.05)):
            if Path(exit_file).exists():
                return Path(exit_file).read_text().strip()
            time.sleep(0.05)
        self.fail("lint never wrote its exit file")

    def merge(self, manifest, results):
        manifest_file = self.root / "manifest.json"
        manifest_file.write_text(json.dumps(manifest))
        completed = time.time()
        return subprocess.run([str(MERGE), str(manifest_file), "atm-x-1-sanity", "atm-x-1", "x",
                               "--reviewer", "sanity-llm", "--started-at", str(completed), "--completed-at", str(completed)],
                              input=json.dumps(results), capture_output=True, text=True)

    def test_manifest_has_fixed_reviewer_roster_and_rejects_obsolete_override(self):
        out = self.run_split(lint="echo ran >> lint-count")
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "0")
        count_file = self.repo.wt / "lint-count"
        self.assertEqual(count_file.read_text(), "ran\n")
        count_file.unlink()
        self.assertEqual(manifest["reviewers"], ["sanity-llm", "sanity-jev", "sanity-selected"])
        self.assertEqual(manifest["operational_reviewer"], "sanity-selected")

        rejected = self.run_split(None, None, None, "sprint/x", "develop", "--reviewers", "both")
        self.assertEqual(rejected.returncode, 1)
        self.assertIn("unrecognized arguments: --reviewers both", rejected.stderr)

    def test_happy_path_renders_one_assignment_per_deliverable(self):
        out = self.run_split(commit=self.repo.sha[:8])
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        self.assertEqual((manifest["sha"], manifest["base_sha"], manifest["deliverables_total"]),
                         (self.repo.sha, self.repo.base_sha, 2))
        self.assertEqual(manifest["changed_files"], ["crates/types/src/retry.rs"])
        self.assertEqual(manifest["files_outside_owned_paths"], [])
        self.assertEqual([a["number"] for a in manifest["assignments"]], [1, 2])
        first = manifest["assignments"][0]["assignment"]
        self.assertEqual(first["deliverable"]["number"], 1)
        self.assertIn("pub mod retry", first["deliverable"]["text"])
        self.assertIn("in the types crate.", first["deliverable"]["text"])
        self.assertEqual(first["deliverables_total"], 2)
        self.assertEqual(first["context"], [])
        self.assertNotIn("acceptance_criteria", first)
        self.assertNotIn("design", first)
        self.assertEqual(first["commit"], self.repo.sha)
        self.assertEqual(first["base_sha"], self.repo.base_sha)
        self.assertEqual(first["owned_paths"], ["crates/types/src/**"])
        self.assertEqual(first["sanity_bead"], "atm-x-1-sanity")
        self.assertEqual(manifest["assignments"][1]["assignment"]["deliverable"]["text"],
                         "Unit tests for 429, 503 and 404.")
        self.assertEqual((manifest["lint"]["timeout_seconds"], type(manifest["lint"]["pid"])), (1800, int))
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "1")
        self.assertIn("lint-ran", Path(manifest["lint"]["log"]).read_text())
        self.assertEqual(sorted(os.listdir(self.scratch)), ["atm-x-1-sanity-lint.exit", "atm-x-1-sanity-lint.log"])
        # the same tree that lint saw: merge accepts two clean results
        merged = self.merge(manifest, [result(1, self.repo.sha), result(2, self.repo.sha)])
        self.assertEqual(merged.returncode, 0, merged.stderr)
        self.assertEqual(json.loads(merged.stdout)["verdict"], "FAIL")   # the fake lint exits 1

    def test_plan_invalid(self):
        cases = {
            "no section": DESCRIPTION.replace("## Deliverables", "## Things"),
            "bulleted not numbered": DESCRIPTION.replace("1. Add", "- Add").replace("2. Unit", "- Unit"),
            "empty": "## Deliverables\n\n## Non-closure\n\nx\n",
            "numbering gap": DESCRIPTION.replace("2. Unit", "3. Unit"),
            "bullet before the first item": "## Deliverables\n- Required auth check.\n1. Add a comment.\n",
            "bullet between items": "## Deliverables\n1. One.\n- Required auth check.\n2. Two.\n",
            "paragraph between items": "## Deliverables\n1. One.\n\nAlso do the auth check.\n\n2. Two.\n",
            "fenced example only": "## Deliverables\n```text\n1. This is an example, not a deliverable.\n```\n",
            "unclosed fence": "## Deliverables\n1. One.\n```\n2. Two.\n",
            "numbered line inside a longer outer fence is not an item": (
                "## Deliverables\n1. Document:\n````markdown\n```\n2. Example only.\n```\n````\n3. Tests.\n"),
            "sub-heading inside the section": (
                "## Deliverables\n1. Comment.\n### Required auth work\n- Add authorization check.\n2. Tests.\n"),
            "fence closed by a longer mixed line": "## Deliverables\n1. Document:\n```\n```~\n2. Example only.\n",
            "fence closed by a shorter line": "## Deliverables\n1. Document:\n````\n```\n2. Example only.\n",
            "heading inside a fence does not end the section": (
                "## Deliverables\n1. Document:\n```markdown\n## Example\n- stray bullet after the fence\n```\n- stray\n2. Tests.\n"),
        }
        for name, description in cases.items():
            with self.subTest(name):
                out = self.run_split(bead(description))
                self.assertEqual(out.returncode, 2, out.stderr)
                self.assertIn("SANITY.PLAN_INVALID", out.stderr)
                self.assertFalse(self.scratch.exists(), "lint must not start for an invalid plan")
        out = self.run_split(bead(cases["numbered line inside a longer outer fence is not an item"]))
        self.assertIn("numbering is not 1..2: [1, 3]", out.stderr)
        out = self.run_split(bead(cases["sub-heading inside the section"]))
        self.assertIn("sub-heading, which is not part of a numbered list: '### Required auth work'", out.stderr)
        for name in ("fence closed by a longer mixed line", "fence closed by a shorter line"):
            self.assertIn("unclosed fenced block", self.run_split(bead(cases[name])).stderr, name)

    def test_plan_items_keep_wrapped_text_sub_bullets_and_fenced_examples(self):
        description = ("## Deliverables\n\n"
                       "1. Add the retry module\n"
                       "with the predicate below.\n"
                       "   - covers 429\n"
                       "   - covers 5xx\n\n"
                       "2. Document it:\n"
                       "   ```rust\n"
                       "   1. not a deliverable\n"
                       "   ```\n"
                       "3. Tests.\n")
        out = self.run_split(bead(description), None, None, "sprint/x", "develop", "--split-only")
        self.assertEqual(out.returncode, 0, out.stderr)
        texts = [a["assignment"]["deliverable"]["text"] for a in json.loads(out.stdout)["assignments"]]
        self.assertEqual(texts[0], "Add the retry module with the predicate below. - covers 429 - covers 5xx")
        self.assertEqual(texts[1], "Document it: ```rust 1. not a deliverable ```")
        self.assertEqual(texts[2], "Tests.")

    def test_fences_hide_headings_and_shorter_fences(self):
        cases = {
            "heading inside a fence": (
                "## Deliverables\n1. Document:\n```markdown\n## Example\ncontent\n```\n2. Tests.\n\n## Non-closure\n\n3. not ours\n",
                ["Document: ```markdown ## Example content ```", "Tests."]),
            "three backticks inside four": (
                "## Deliverables\n1. Document:\n````markdown\n```\n2. Example only.\n```\n````\n2. Tests.\n",
                ["Document: ````markdown ``` 2. Example only. ``` ````", "Tests."]),
            "tildes do not close backticks": (
                "## Deliverables\n1. Document:\n```\n~~~\n2. Example only.\n```\n2. Tests.\n",
                ["Document: ``` ~~~ 2. Example only. ```", "Tests."]),
            "closing fence may be longer and padded": (
                "## Deliverables\n1. Document:\n```\n2. Example only.\n   `````  \n2. Tests.\n",
                ["Document: ``` 2. Example only. `````", "Tests."]),
            "a level-2 heading after the list ends the section": (
                "## Deliverables\n1. One.\n2. Two.\n## Non-closure\n### Sub\n- bullets here are fine\n",
                ["One.", "Two."]),
        }
        for name, (description, expected) in cases.items():
            with self.subTest(name):
                out = self.run_split(bead(description), None, None, "sprint/x", "develop", "--split-only")
                self.assertEqual(out.returncode, 0, out.stderr)
                texts = [a["assignment"]["deliverable"]["text"] for a in json.loads(out.stdout)["assignments"]]
                self.assertEqual(texts, expected)

    def test_commit_mismatch(self):
        with self.subTest("dirty tree"):
            (self.repo.wt / "scratch.txt").write_text("x")
            out = self.run_split()
            self.assertEqual((out.returncode, "SANITY.COMMIT_MISMATCH" in out.stderr), (4, True), out.stderr)
            (self.repo.wt / "scratch.txt").unlink()
        with self.subTest("HEAD elsewhere"):
            git(self.repo.wt, "checkout", "-q", "develop")
            out = self.run_split()
            self.assertEqual(out.returncode, 4, out.stderr)
            git(self.repo.wt, "checkout", "-q", "sprint/x")
        with self.subTest("not pushed"):
            self.repo.commit("crates/types/src/retry.rs", "pub fn is_retryable(s: u16) -> bool { s >= 500 }\n", "more")
            out = self.run_split(commit=git(self.repo.wt, "rev-parse", "HEAD"))
            self.assertEqual(out.returncode, 4, out.stderr)
            self.assertIn("origin/sprint/x", out.stderr)

    def test_target_unreadable(self):
        with self.subTest("worktree missing"):
            out = self.run_split(worktree=self.root / "nowhere")
            self.assertEqual((out.returncode, "SANITY.TARGET_UNREADABLE" in out.stderr), (3, True), out.stderr)
        with self.subTest("unknown commit"):
            out = self.run_split(commit="deadbeef")
            self.assertEqual(out.returncode, 3, out.stderr)
        with self.subTest("unknown base"):
            out = self.run_split(base="integrate/nope")
            self.assertEqual(out.returncode, 3, out.stderr)

    def test_files_outside_owned_paths(self):
        self.repo.commit("docs/other.md", "stray\n", "stray")
        out = self.run_split(commit=self.repo.push_head())
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        self.assertEqual(manifest["changed_files"], ["crates/types/src/retry.rs", "docs/other.md"])
        self.assertEqual(manifest["files_outside_owned_paths"], ["docs/other.md"])
        self.assertEqual(manifest["assignments"][0]["assignment"]["files_outside_owned_paths"], ["docs/other.md"])
        self.wait_lint(manifest["lint"]["exit_file"])

    def test_renames_list_source_and_destination(self):
        with self.subTest("outside -> inside the fence"):
            git(self.repo.wt, "mv", "docs/notes.md", "crates/types/src/notes.md")
            git(self.repo.wt, "commit", "-q", "-m", "move in")
            out = self.run_split(commit=self.repo.push_head())
            self.assertEqual(out.returncode, 0, out.stderr)
            manifest = json.loads(out.stdout)
            self.assertEqual(manifest["changed_files"],
                             ["crates/types/src/notes.md", "crates/types/src/retry.rs", "docs/notes.md"])
            self.assertEqual(manifest["files_outside_owned_paths"], ["docs/notes.md"])
            self.wait_lint(manifest["lint"]["exit_file"])
        with self.subTest("inside -> outside the fence"):
            (self.repo.wt / "src").mkdir()
            git(self.repo.wt, "mv", "crates/types/src/lib.rs", "src/lib.rs")
            git(self.repo.wt, "commit", "-q", "-m", "move out")
            out = self.run_split(commit=self.repo.push_head())
            self.assertEqual(out.returncode, 0, out.stderr)
            manifest = json.loads(out.stdout)
            self.assertIn("src/lib.rs", manifest["changed_files"])
            self.assertIn("crates/types/src/lib.rs", manifest["changed_files"])
            self.assertEqual(manifest["files_outside_owned_paths"], ["docs/notes.md", "src/lib.rs"])
            self.wait_lint(manifest["lint"]["exit_file"])

    def test_merge_rejects_a_worktree_that_moved_after_the_split(self):
        out = self.run_split()
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        self.wait_lint(manifest["lint"]["exit_file"])
        (self.repo.wt / "crates/types/src/retry.rs").write_text("pub fn is_retryable(_: u16) -> bool { true }\n")
        merged = self.merge(manifest, [result(1, self.repo.sha), result(2, self.repo.sha)])
        self.assertEqual(merged.returncode, 3, merged.stderr)
        self.assertEqual(json.loads(merged.stdout)["error"]["code"], "SANITY.COMMIT_MISMATCH")
        self.assertEqual(json.loads(merged.stdout)["verdict"], "CANNOT_RUN")
        git(self.repo.wt, "checkout", "-q", "--", ".")
        git(self.repo.wt, "checkout", "-q", "develop")
        merged = self.merge(manifest, [result(1, self.repo.sha), result(2, self.repo.sha)])
        self.assertEqual(merged.returncode, 3, merged.stderr)
        self.assertEqual(json.loads(merged.stdout)["error"]["code"], "SANITY.COMMIT_MISMATCH")
        self.assertEqual(json.loads(merged.stdout)["verdict"], "CANNOT_RUN")

    def test_lint_supervisor_captures_every_part_of_a_compound_command(self):
        lint = "sh -c 'echo \"  --> src/a.rs:3:1\"; exit 1' && true"
        out = self.run_split(lint=lint)
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "1")
        self.assertIn("--> src/a.rs:3:1", Path(manifest["lint"]["log"]).read_text())
        merged = self.merge(manifest, [result(1, self.repo.sha), result(2, self.repo.sha)])
        self.assertEqual(merged.returncode, 0, merged.stderr)
        vars_ = json.loads(merged.stdout)
        self.assertEqual((vars_["verdict"], vars_["findings_count"]), ("FAIL", 1))
        self.assertIn("- `src/a.rs:3` lint:", vars_["lint_md"])

    def test_lint_supervisor_always_writes_the_exit_file(self):
        for lint, expected in (("exec false", "1"), ("exit 3", "3"), ("true", "0")):
            with self.subTest(lint):
                out = self.run_split(lint=lint)
                self.assertEqual(out.returncode, 0, out.stderr)
                self.assertEqual(self.wait_lint(json.loads(out.stdout)["lint"]["exit_file"]), expected)

    def test_lint_supervisor_times_out_and_kills_the_command(self):
        out = self.run_split(None, None, None, "sprint/x", "develop", "--lint-timeout-seconds", "1", lint="sleep 3017")
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        self.assertEqual(manifest["lint"]["timeout_seconds"], 1)
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "timeout")
        time.sleep(0.2)
        leftover = subprocess.run(["pgrep", "-f", "sleep 3017"], capture_output=True, text=True)
        self.assertEqual(leftover.stdout.strip(), "", "the lint command survived its timeout")
        merged = self.merge(manifest, [result(1, self.repo.sha), result(2, self.repo.sha)])
        self.assertEqual(merged.returncode, 3, merged.stderr)
        self.assertEqual(json.loads(merged.stdout)["error"]["code"], "SANITY.LINT_UNAVAILABLE")
        self.assertEqual(json.loads(merged.stdout)["verdict"], "CANNOT_RUN")

    def test_lint_supervisor_is_stopped_with_its_process_group(self):
        out = self.run_split(lint="sleep 3018")
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        time.sleep(0.3)
        os.killpg(manifest["lint"]["pid"], signal.SIGTERM)
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "cancelled")
        time.sleep(0.2)
        leftover = subprocess.run(["pgrep", "-f", "sleep 3018"], capture_output=True, text=True)
        self.assertEqual(leftover.stdout.strip(), "", "the lint command survived the cancellation")

    def test_lint_supervisor_cancellation_kills_a_term_resistant_command(self):
        marker = self.root / "resistant.pid"
        script = self.root / "resistant.py"
        script.write_text("import os, pathlib, signal, time\n"
                          "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
                          f"pathlib.Path({str(marker)!r}).write_text(str(os.getpid()))\n"
                          "time.sleep(3019)\n")
        out = self.run_split(lint=f"{sys.executable} {script}")
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        for _ in range(200):
            if marker.exists():
                break
            time.sleep(0.05)
        child_pid = int(marker.read_text())
        os.killpg(manifest["lint"]["pid"], signal.SIGTERM)
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "cancelled")
        for _ in range(100):   # the supervisor exits right after writing the exit file
            if subprocess.run(["kill", "-0", str(manifest["lint"]["pid"])], capture_output=True).returncode != 0:
                break
            time.sleep(0.05)
        self.assertNotEqual(subprocess.run(["kill", "-0", str(manifest["lint"]["pid"])], capture_output=True).returncode, 0)
        self.assertNotEqual(subprocess.run(["kill", "-0", str(child_pid)], capture_output=True).returncode, 0,
                            "the TERM-resistant lint command survived the cancellation")
        leftover = subprocess.run(["pgrep", "-f", "resistant.py"], capture_output=True, text=True)
        self.assertEqual(leftover.stdout.strip(), "")

    def resistant_descendant(self):
        """A lint command whose shell backgrounds a SIGTERM-ignoring python and waits on it."""
        marker = self.root / "descendant.pid"
        helper = self.root / "descendant.py"
        helper.write_text("import os, pathlib, signal, time\n"
                          "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
                          f"pathlib.Path({str(marker)!r}).write_text(str(os.getpid()))\n"
                          "time.sleep(3020)\n")
        return f"{sys.executable} {helper} & wait", marker

    def alive(self, pid):
        return subprocess.run(["kill", "-0", str(pid)], capture_output=True).returncode == 0

    def wait_gone(self, pid, seconds=10):
        for _ in range(int(seconds / 0.05)):
            if not self.alive(pid):
                return
            time.sleep(0.05)
        self.fail(f"process {pid} is still alive")

    def test_lint_supervisor_cancellation_kills_a_term_resistant_descendant(self):
        lint, marker = self.resistant_descendant()
        out = self.run_split(lint=lint)
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        for _ in range(200):
            if marker.exists():
                break
            time.sleep(0.05)
        descendant = int(marker.read_text())
        os.killpg(manifest["lint"]["pid"], signal.SIGTERM)
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "cancelled")
        self.wait_gone(manifest["lint"]["pid"])
        self.assertFalse(self.alive(descendant), "the TERM-ignoring descendant survived the cancellation")
        self.assertEqual(subprocess.run(["pgrep", "-f", "descendant.py"], capture_output=True, text=True).stdout, "")

    def test_lint_supervisor_timeout_kills_a_term_resistant_descendant(self):
        lint, marker = self.resistant_descendant()
        out = self.run_split(None, None, None, "sprint/x", "develop", "--lint-timeout-seconds", "1", lint=lint)
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "timeout")
        descendant = int(marker.read_text())
        self.wait_gone(manifest["lint"]["pid"])
        self.assertFalse(self.alive(descendant), "the TERM-ignoring descendant survived the timeout")

    def test_split_only_skips_git_and_lint(self):
        (self.repo.wt / "dirty.txt").write_text("would fail pinning")
        out = self.run_split(None, None, "abc1234", "sprint/x", "develop", "--split-only")
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        self.assertEqual((manifest["sha"], manifest["deliverables_total"], manifest["lint"]), ("abc1234", 2, None))
        self.assertEqual(manifest["changed_files"], [])
        self.assertEqual(len(manifest["assignments"]), 2)
        self.assertFalse(self.scratch.exists())

    def test_usage(self):
        out = subprocess.run([str(SCRIPT), "--task", "t"], capture_output=True, text=True)
        self.assertEqual(out.returncode, 1)


if __name__ == "__main__":
    unittest.main()
