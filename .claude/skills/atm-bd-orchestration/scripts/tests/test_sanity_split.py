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
import unittest.mock

SCRIPTS = Path(__file__).parents[1]
SCRIPT = SCRIPTS / "sanity-split"
MERGE = SCRIPTS / "sanity-merge"
FAKE_LINT = "sh -c 'echo lint-ran; exit 1'"

# Runs `sanity-split lint-supervisor` with one test hook: Popen records the lint process group id (the
# child's pid; the supervisor starts it in a new session) atomically, before the supervisor waits, so a
# test knows the group whether or not the command ever started. With --gated the supervisor's wait
# expires (TimeoutExpired) when the test writes a byte to this driver's stdin instead of on the clock.
DRIVER = """import importlib.machinery, importlib.util, os, subprocess, sys
from pathlib import Path
script, pgid_file, gated, argv = sys.argv[1], Path(sys.argv[2]), sys.argv[3] == "--gated", sys.argv[4:]
loader = importlib.machinery.SourceFileLoader("sanity_split", script)
split = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
loader.exec_module(split)


class Recorded(subprocess.Popen):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        partial = pgid_file.with_name(pgid_file.name + ".tmp")
        partial.write_text(str(self.pid))
        os.replace(partial, pgid_file)

    def wait(self, timeout=None):
        if gated and timeout is not None:
            sys.stdin.buffer.read(1)
            raise subprocess.TimeoutExpired(self.args, timeout)
        return super().wait(timeout)


split.subprocess.Popen = Recorded
sys.exit(split.supervise(argv))
"""


def wait_for(condition, what, seconds=10, interval=0.05):
    """Poll `condition` until it returns a truthy value and return that value; fail after a hard deadline."""
    deadline = time.monotonic() + seconds
    while True:
        value = condition()
        if value:
            return value
        if time.monotonic() > deadline:
            raise AssertionError(f"{what}: not reached within {seconds}s")
        time.sleep(interval)


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        pass
    return True


def kill_group(pgid):
    """Test cleanup: SIGKILL a process group; a group that is already gone (or, on macOS, only exiting
    members: EPERM) is fine."""
    try:
        os.killpg(pgid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass


def stop_supervisor(pid):
    """Test cleanup: SIGTERM a supervisor still running (it stops a lint group the test may never have
    learned), then SIGKILL its group whatever happened."""
    try:
        os.killpg(pid, signal.SIGTERM)
        wait_for(lambda: not alive(pid), f"supervisor {pid} stopped")
    except (ProcessLookupError, PermissionError, AssertionError):
        pass
    kill_group(pid)


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
        out = subprocess.run(argv, capture_output=True, text=True)
        try:
            supervisor = json.loads(out.stdout)["lint"]["pid"]
        except (ValueError, KeyError, TypeError):
            supervisor = None
        if supervisor:
            self.addCleanup(stop_supervisor, supervisor)    # the supervisor leads its own session
        return out

    def wait_lint(self, exit_file):
        exit_file = Path(exit_file)
        return wait_for(lambda: exit_file.exists() and exit_file.read_text().strip(), f"exit file {exit_file.name}")

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

    def test_rerun_assignment_is_the_manifest_assignment_with_context(self):
        out = self.run_split()
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest_file = self.root / "manifest.json"
        manifest_file.write_text(out.stdout)
        assignment = json.loads(out.stdout)["assignments"][1]["assignment"]
        context = [{"path": "crates/types/src/lib.rs", "why": "declares the module"}]
        # agents/dev-sanity.md step 4, rerun
        rerun = subprocess.run(["jq", "--argjson", "n", "2", "--argjson", "context", json.dumps(context),
                                ".assignments[] | select(.number == $n) | .assignment | .context = $context",
                                str(manifest_file)], capture_output=True, text=True)
        self.assertEqual(rerun.returncode, 0, rerun.stderr)
        variables = {"sanity_bead": assignment["sanity_bead"], "dev_bead_id": assignment["dev_bead"]["id"],
                     "dev_bead_title": assignment["dev_bead"]["title"],
                     "deliverable_number": 2, "deliverable_text": assignment["deliverable"]["text"],
                     **{key: assignment[key] for key in ("deliverables_total", "owned_paths", "changed_files",
                                                         "files_outside_owned_paths", "worktree_path", "branch",
                                                         "commit", "base_sha")},
                     "context": context}
        rendered = subprocess.run(["sc-compose", "render", "--strict", "--root", str(SCRIPTS.parent / "templates"),
                                   "--file", "dev-sanity-assignment.json.j2", "--var-file", "/dev/stdin"],
                                  input=json.dumps(variables), capture_output=True, text=True)
        self.assertEqual(rendered.returncode, 0, rendered.stderr)
        self.assertEqual(json.loads(rerun.stdout), json.loads(rendered.stdout))
        self.assertEqual(json.loads(rerun.stdout)["context"], context)

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
        with self.subTest("tool lock and log files are not dirt"):
            (self.repo.wt / ".beads.gate.lock").write_text("")
            (self.repo.wt / ".sc-compose").mkdir()
            (self.repo.wt / ".sc-compose" / "log.jsonl").write_text("{}")
            out = self.run_split()
            self.assertEqual(out.returncode, 0, out.stderr)
            (self.repo.wt / ".beads.gate.lock").unlink()
            shutil.rmtree(self.repo.wt / ".sc-compose")
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

    def test_layer_prs_diff_only_the_sprints_own_layers(self):
        """After a dev-fix with another sprint's layer between, the changed files are the sprint's layer ranges only."""
        la = (self.repo.base_sha, self.repo.sha)
        git(self.repo.wt, "checkout", "-q", "-b", "sprint/y")
        self.repo.commit("docs/other-sprint.md", "other sprint\n", "other sprint")
        git(self.repo.wt, "push", "-q", "-u", "origin", "sprint/y")
        lb_head = git(self.repo.wt, "rev-parse", "HEAD")
        git(self.repo.wt, "checkout", "-q", "-b", "fix/x")
        self.repo.commit("crates/types/src/retry_tests.rs", "// 503\n", "fix")
        git(self.repo.wt, "push", "-q", "-u", "origin", "fix/x")
        f_head = git(self.repo.wt, "rev-parse", "HEAD")
        prs = self.root / "prs.json"
        prs.write_text(json.dumps({"1": {"baseRefOid": la[0], "headRefOid": la[1]}, "3": {"baseRefOid": lb_head, "headRefOid": f_head}}))
        fake = self.root / "bin"
        fake.mkdir()
        (fake / "gh").write_text(f"#!{sys.executable}\nimport json, sys\nprs = json.load(open({str(prs)!r}))\n"
                                 "sys.exit(1) if sys.argv[3] not in prs else print(json.dumps(prs[sys.argv[3]]))\n")
        (fake / "gh").chmod(0o755)
        path = f"{fake}{os.pathsep}{os.environ['PATH']}"
        with unittest.mock.patch.dict(os.environ, {"PATH": path}):
            whole = self.run_split(None, None, f_head, "fix/x", "develop")
            own = self.run_split(None, None, f_head, "fix/x", "sprint/y", "--layer-pr", "1", "--layer-pr", "3")
            stale = self.run_split(None, None, f_head, "fix/x", "sprint/y", "--layer-pr", "3", "--layer-pr", "1")
            missing = self.run_split(None, None, f_head, "fix/x", "sprint/y", "--layer-pr", "1", "--layer-pr", "9")
        self.assertIn("docs/other-sprint.md", json.loads(whole.stdout)["changed_files"])  # one span carries the other sprint
        self.assertEqual(own.returncode, 0, own.stderr)
        manifest = json.loads(own.stdout)
        self.assertEqual(manifest["changed_files"], ["crates/types/src/retry.rs", "crates/types/src/retry_tests.rs"])
        self.assertEqual(manifest["files_outside_owned_paths"], [])
        self.assertEqual(stale.returncode, 4, stale.stderr)  # the checked PR goes last and must be at the pinned sha
        self.assertEqual(missing.returncode, 3, missing.stderr)
        for out in (whole, own):
            self.wait_lint(json.loads(out.stdout)["lint"]["exit_file"])

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

    def owned_group(self, command):
        """Prefix the lint command so its shell, the group leader, publishes its process group id atomically."""
        marker = self.root / "lint.pgid"
        return f"echo $$ > {marker}.tmp && mv {marker}.tmp {marker} && {command}", marker

    def read_marker(self, marker):
        """The first number in an atomically written marker; its last number is the lint process group id."""
        numbers = wait_for(lambda: marker.exists() and marker.read_text().split(), f"marker {marker.name}")
        self.addCleanup(kill_group, int(numbers[-1]))
        return int(numbers[0])

    def assert_group_gone(self, pgid, message):
        """Only this test's lint process group is inspected, never processes of concurrent runs."""
        with self.assertRaises(ProcessLookupError, msg=message):
            os.killpg(pgid, 0)

    def supervise(self, command, timeout_seconds, gated=False):
        """Start the lint supervisor through DRIVER; return it, its exit file and the recorded group id file."""
        driver, pgid_file = self.root / "driver.py", self.root / "lint.pgid"
        exit_file = self.root / "lint.exit"
        driver.write_text(DRIVER)
        proc = subprocess.Popen(
            [sys.executable, str(driver), str(SCRIPT), str(pgid_file), "--gated" if gated else "--clock",
             "--worktree", str(self.repo.wt), "--command", command, "--log", str(self.root / "lint.log"),
             "--exit-file", str(exit_file), "--timeout-seconds", str(timeout_seconds)],
            stdin=subprocess.PIPE if gated else subprocess.DEVNULL, start_new_session=True)
        self.addCleanup(proc.wait)
        self.addCleanup(lambda: proc.stdin and proc.stdin.close())
        self.addCleanup(kill_group, proc.pid)
        self.addCleanup(lambda: pgid_file.exists() and kill_group(int(pgid_file.read_text())))
        return proc, exit_file, pgid_file

    def finished(self, proc):
        return wait_for(lambda: proc.poll() is not None, f"supervisor {proc.pid} exit") and proc.returncode

    def test_lint_timeout_reaches_the_merge_as_lint_unavailable(self):
        out = self.run_split(None, None, None, "sprint/x", "develop", "--lint-timeout-seconds", "1",
                             lint="exec sleep 3017")
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        self.assertEqual(manifest["lint"]["timeout_seconds"], 1)
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "timeout")
        merged = self.merge(manifest, [result(1, self.repo.sha), result(2, self.repo.sha)])
        self.assertEqual(merged.returncode, 3, merged.stderr)
        self.assertEqual(json.loads(merged.stdout)["error"]["code"], "SANITY.LINT_UNAVAILABLE")
        self.assertEqual(json.loads(merged.stdout)["verdict"], "CANNOT_RUN")

    def test_lint_supervisor_times_out_and_kills_the_command(self):
        """Real clock: the group id is recorded at Popen, so it is known even if the command never started."""
        proc, exit_file, pgid_file = self.supervise("exec sleep 3017", 1)
        self.assertEqual(self.finished(proc), 0)
        self.assertEqual(exit_file.read_text().strip(), "timeout")
        self.assert_group_gone(int(pgid_file.read_text()), "the lint command survived its timeout")

    def test_lint_supervisor_is_stopped_with_its_process_group(self):
        lint, group = self.owned_group("exec sleep 3018")
        out = self.run_split(lint=lint)
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        pgid = self.read_marker(group)          # the lint command is running before the supervisor is stopped
        os.killpg(manifest["lint"]["pid"], signal.SIGTERM)
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "cancelled")
        self.assert_group_gone(pgid, "the lint command survived the cancellation")

    def test_lint_supervisor_cancellation_kills_a_term_resistant_command(self):
        marker = self.root / "resistant.pid"
        script = self.root / "resistant.py"
        script.write_text("import os, pathlib, signal, time\n"
                          "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
                          f"pathlib.Path({str(marker)!r} + '.tmp').write_text(str(os.getpid()) + ' ' + str(os.getpgid(0)))\n"
                          f"os.replace({str(marker)!r} + '.tmp', {str(marker)!r})\n"
                          "time.sleep(3019)\n")
        out = self.run_split(lint=f"{sys.executable} {script}")
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        child_pid = self.read_marker(marker)
        pgid = int(marker.read_text().split()[1])
        os.killpg(manifest["lint"]["pid"], signal.SIGTERM)
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "cancelled")
        self.wait_gone(manifest["lint"]["pid"])    # the supervisor exits right after writing the exit file
        self.assertFalse(alive(child_pid), "the TERM-resistant lint command survived the cancellation")
        self.assert_group_gone(pgid, "a member of the lint process group survived the cancellation")

    def resistant_descendant(self):
        """A lint command whose shell backgrounds a SIGTERM-ignoring python and waits on it."""
        marker = self.root / "descendant.pid"
        helper = self.root / "descendant.py"
        helper.write_text("import os, pathlib, signal, time\n"
                          "signal.signal(signal.SIGTERM, signal.SIG_IGN)\n"
                          f"pathlib.Path({str(marker)!r} + '.tmp').write_text(str(os.getpid()) + ' ' + str(os.getpgid(0)))\n"
                          f"os.replace({str(marker)!r} + '.tmp', {str(marker)!r})\n"
                          "time.sleep(3020)\n")
        return f"{sys.executable} {helper} & wait", marker

    def wait_gone(self, pid):
        wait_for(lambda: not alive(pid), f"process {pid} gone")

    def test_lint_supervisor_cancellation_kills_a_term_resistant_descendant(self):
        lint, marker = self.resistant_descendant()
        out = self.run_split(lint=lint)
        self.assertEqual(out.returncode, 0, out.stderr)
        manifest = json.loads(out.stdout)
        descendant = self.read_marker(marker)
        pgid = int(marker.read_text().split()[1])
        os.killpg(manifest["lint"]["pid"], signal.SIGTERM)
        self.assertEqual(self.wait_lint(manifest["lint"]["exit_file"]), "cancelled")
        self.wait_gone(manifest["lint"]["pid"])
        self.assertFalse(alive(descendant), "the TERM-ignoring descendant survived the cancellation")
        self.assert_group_gone(pgid, "a member of the lint process group survived the cancellation")

    def test_lint_supervisor_timeout_kills_a_term_resistant_descendant(self):
        """Gated clock: the timeout expires only once the descendant exists, however slow its startup."""
        lint, marker = self.resistant_descendant()
        proc, exit_file, pgid_file = self.supervise(lint, 1800, gated=True)
        descendant = self.read_marker(marker)
        proc.stdin.write(b"t")
        proc.stdin.close()
        self.assertEqual(self.finished(proc), 0)
        self.assertEqual(exit_file.read_text().strip(), "timeout")
        self.assertFalse(alive(descendant), "the TERM-ignoring descendant survived the timeout")
        self.assert_group_gone(int(pgid_file.read_text()), "a member of the lint process group survived the timeout")

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
