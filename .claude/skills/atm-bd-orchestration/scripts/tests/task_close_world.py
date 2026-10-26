"""Shared close-hook test world; importing it does not collect another test module."""
from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
import copy
import importlib.util
import io
import json
from pathlib import Path
import subprocess

SCRIPTS = Path(__file__).parents[1]
SPEC = importlib.util.spec_from_file_location("task_close", SCRIPTS / "atm-task-close.py")
assert SPEC is not None and SPEC.loader is not None
hook = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(hook)
EXAMPLES = SCRIPTS.parent / "examples"
# Completion values the hook derives from git and the PR, so the lead file never carries them.
DERIVED = ("commit", "generated_at", "pr_number", "pr_url", "rebased_onto")
BD_SCHEMA = (SCRIPTS / "tests/fixtures/bd-1.3.1/bd-schema.json").read_text()


def cache_call(argv):
    """A bd call the hook's bd-cache makes to apply a write (backend check, pre-read, read-back); never a decision read."""
    return argv[:2] in (["bd", "version"], ["bd", "schema"]) or (argv[:2] == ["bd", "list"] and "--id" in argv)


def bd_row(bead, **fields):
    """A bead in `bd list --json` shape, so the hook's bd-cache accepts it."""
    return {"id": bead, "title": bead, "status": "open", "priority": 2, "issue_type": "task", "labels": [],
            "metadata": {}, "created_at": "2026-10-19T00:00:00Z", "updated_at": "2026-10-19T00:00:00Z",
            "dependency_count": 0, "dependent_count": 0, "comment_count": 0, **fields}


def example_values(role):
    values = json.loads((EXAMPLES / f"task-close-vars-{role}.json").read_text())
    if role in ("dev", "fix"):
        values.update(json.loads((EXAMPLES / f"task-close-result-{role}.json").read_text()))
    return values


class World:
    def __init__(self, root, monkeypatch):
        self.root = root
        self.values = {}
        for role in ("qa", "sanity", "fix", "dev"):
            self.values.update(example_values(role))
        self.values.update(primary_checkout=str(root), worktree_path=str(root))
        self.values["rebased_onto"] = "b" * 40
        self.values["carry_forward"] = ""
        (root / "scripts").mkdir()
        (root / "scripts/example.py").write_text("pass\n")
        # HEAD is stable even when a test removes commit from the dispatch values.
        self.head = self.values["commit"]
        self.pr_number = self.values["pr_number"]
        (root / ".atm-bd").mkdir()
        (root / ".beads").mkdir()
        (root / ".beads/metadata.json").write_text(json.dumps({"dolt_database": "testdb", "project_id": "p-1"}))
        self.stamps = 0
        (root / ".atm-bd/phase-bj.toml").write_text('lead = "phase-lead"\n')
        (root / ".atm.toml").write_text('[atm]\ndefault_team = "test-team"\n')
        config = root / hook.repo_config.RELATIVE_PATH
        config.parent.mkdir(parents=True)
        config.write_text('bead_prefix: atm\ndev_sanity_member: sanity\nqa_member: quality\nteam_lead_member: team-lead\n')
        self.path = root / "dispatch.json"
        self.result_path = root / "result.json"
        self.beads = {}
        for name in ("checked_bead", "sanity_bead", "qa_bead"):
            bead = self.values[name]
            self.beads[bead] = bd_row(bead, status="in_progress", assignee="developer")
        self.beads[self.values["checked_bead"]]["metadata"]["sanity_bead"] = self.values["sanity_bead"]
        self.beads[self.values["checked_bead"]]["metadata"].update(
            pr_number=self.pr_number, pr_url=self.values["pr_url"])
        self.beads[self.values["sanity_bead"]]["metadata"]["dev_bead"] = self.values["checked_bead"]
        self.beads["atm-phase-bj.stack"] = json.loads((SCRIPTS / "tests/fixtures/stack-bead.json").read_text())
        self.tasks = {self.values[name]: [] for name in ("dev_task_id", "sanity_task_id", "qa_task_id")}
        self.tasks[self.values["dev_task_id"]].append(self.assignment(1))
        self.messages, self.calls, self.writes = [], [], []
        self.sleeps = []
        self.fail_after: int | None = None
        self.github_failure = False
        self.git_network_failure = None
        self.timeout_command = None
        self.concurrent_metadata = None
        self.reassign_after_write = None
        self.overrides = {}
        monkeypatch.setenv("ATM_IDENTITY", "caller")
        monkeypatch.delenv("BEADS_DIR", raising=False)

    def assignment(self, number):
        return dict(event="assigned", message_id=f"assignment-{number}", seq=number,
                    at=f"2026-10-19T00:{number:02d}:00Z", to_state="assigned")

    def run(self, role="dev", *, skip_checks=False, **changes):
        self.values.update(changes)
        task = self.values.get({"dev": "dev_task_id", "fix": "dev_task_id", "sanity": "sanity_task_id", "qa": "qa_task_id"}[role])
        if task in self.tasks and not self.tasks[task]:
            self.tasks[task].append(self.assignment(1))
        extra = []
        if role in ("dev", "fix"):
            fields = hook.header(hook.TEMPLATES / "task-close-vars.j2")["result_variables_by_role"][role]
            self.result_path.write_text(json.dumps({name: self.values[name] for name in fields if name in self.values}))
            self.path.write_text(json.dumps({name: value for name, value in self.values.items()
                                             if name not in fields and name not in DERIVED}))
            extra = ["--result", str(self.result_path)]
        else:
            self.path.write_text(json.dumps(self.values))
        stdout, stderr = io.StringIO(), io.StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            args = ["--role", role, "--vars", str(self.path)] + extra + (["--skip-checks"] if skip_checks else [])
            result = hook.main(args, runner=self, clock=lambda: 42, sleep=self.sleeps.append)
        assert stdout.getvalue() == stderr.getvalue()
        output = json.loads(stdout.getvalue())
        assert result == int("err" in output)
        return output

    def __call__(self, argv, **kwargs):
        self.calls.append((argv, kwargs))
        if self.timeout_command and argv[:len(self.timeout_command)] == self.timeout_command:
            raise subprocess.TimeoutExpired(argv, kwargs["timeout"])
        if tuple(argv) in self.overrides:
            code, stdout = self.overrides[tuple(argv)]
            return subprocess.CompletedProcess(argv, code, stdout, "injected failure" if code else "")
        if argv[0] == "git" and argv[1] == self.git_network_failure:
            return subprocess.CompletedProcess(argv, 1, "", "remote unavailable")
        output, write = "", False
        if argv[:3] == ["bd", "where", "--json"]:
            output = {"path": str(self.root / ".beads")}
        elif argv[:2] == ["bd", "version"]:
            output = "bd version 1.3.1 (dev: fake)\n"
        elif argv[:2] == ["bd", "schema"]:
            output = BD_SCHEMA
        elif argv[:2] == ["bd", "list"] and "--id" in argv:
            output = [self.beads[bead] for bead in argv[argv.index("--id") + 1].split(",") if bead in self.beads]
        elif argv[:2] == ["bd", "show"]:
            output = [self.beads[argv[2]]]
        elif argv[:2] == ["bd", "list"]:
            output = [row for row in self.beads.values() if row["metadata"].get("dev_bead")]
        elif argv[:2] == ["bd", "update"]:
            write = True
            record = self.beads[argv[2]]
            if self.concurrent_metadata:
                record["metadata"].update(self.concurrent_metadata)
                self.concurrent_metadata = None
            for name in ("status", "assignee", "metadata"):
                if "--" + name in argv:
                    value = argv[argv.index("--" + name) + 1]
                    if name == "metadata":
                        record[name] = json.loads(Path(value[1:]).read_text()) if value.startswith("@") else json.loads(value)
                    else:
                        record[name] = value
            for index, flag in enumerate(argv):
                if flag == "--set-metadata":
                    key, value = argv[index + 1].split("=", 1)
                    try:
                        parsed = json.loads(value)
                    except ValueError:
                        parsed = value
                    # bd infers scalars, but treats JSON arrays/objects as strings.
                    record["metadata"][key] = value if isinstance(parsed, (list, dict)) else parsed
        elif argv[:2] == ["bd", "close"]:
            write = True
            self.beads[argv[2]]["status"] = "closed"
            self.beads[argv[2]]["close_reason"] = argv[argv.index("--reason") + 1]
        elif argv[:3] == ["atm", "task", "events"]:
            output = {"events": self.tasks.get(argv[3], [])}
        elif argv[:3] == ["atm", "task", "close"]:
            write = True
            if "--template" in argv:
                values = json.loads(Path(argv[argv.index("--vars") + 1]).read_text())
                body = hook.sc_render.render(argv[argv.index("--template") + 1], values)
                assert argv[3] in body
            self.tasks[argv[3]].append(dict(event=argv[4], to_state="complete", close_outcome=argv[4]))
        elif argv[:2] == ["atm", "send"] or argv[:3] == ["atm", "task", "assign"]:
            write = True
            recipient = argv[3] if argv[1] == "task" else argv[2]
            values, template_type = {}, None
            if "--template" in argv:
                template = Path(argv[argv.index("--template") + 1])
                values = json.loads(Path(argv[argv.index("--vars") + 1]).read_text())
                template_type = hook.header(template)["metadata"]["type"]
                body = hook.sc_render.render(template, values)
            else:
                body = argv[3]
            if argv[1] == "task":
                task = argv[argv.index("--task-id") + 1]
                assert task in body
                events = self.tasks[task]
                event = self.assignment(len(events) + 1)
                if events:
                    event["event"] = "reopened" if events[-1].get("to_state") in hook.TERMINAL_STATES else "reassigned"
                events.append(event)
            self.messages.append(dict(type=template_type, values=values, sender=kwargs["env"]["ATM_IDENTITY"],
                                      recipient=recipient, body=body, at="2026-10-19T23:00:00Z"))
        elif argv[0] == "git":
            if argv[1] == "rev-parse":
                output = self.values.get("commit", self.head) if argv[2] == "HEAD" else "b" * 40
                if "branch" in self.values and argv[2] == f"refs/remotes/origin/{self.values['branch']}":
                    output = self.values.get("commit", self.head)
            elif argv[1] == "ls-remote":
                output = self.values.get("commit", self.head) + "\trefs/heads/branch"
            elif argv[1:4] == ["remote", "get-url", "origin"]:
                output = "git@github.com:example/repo.git"
            else:
                assert argv[1] in ("status", "fetch", "merge-base"), argv
        elif argv[:2] == ["gh", "api"]:
            if self.github_failure == "raise":
                raise OSError("GitHub unavailable")
            if self.github_failure:
                return subprocess.CompletedProcess(argv, 1, "", "HTTP 429")
            output = [{"number": self.values.get("pr_number", self.pr_number), "head": {"sha": self.values.get("commit", self.head)}, "base": {"ref": "deliberately-different"}}]
        else:
            raise AssertionError(argv)
        if write and argv[0] == "bd":
            self.stamps += 1
            self.beads[argv[2]]["updated_at"] = f"2026-10-19T01:{self.stamps // 60:02d}:{self.stamps % 60:02d}Z"
        if write:
            self.writes.append((copy.deepcopy(argv), kwargs["env"]["ATM_IDENTITY"]))
            if len(self.writes) == self.reassign_after_write:
                task = self.values["sanity_task_id"]
                event = self.assignment(2)
                event["event"] = "reopened"
                self.tasks[task].append(event)
            if len(self.writes) == self.fail_after:
                return subprocess.CompletedProcess(argv, 1, "", "failed after committed effect")
        return subprocess.CompletedProcess(argv, 0, output if isinstance(output, str) else json.dumps(output), "")


