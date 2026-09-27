from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import unittest


SCRIPTS = Path(__file__).parents[1]
sys.path.insert(0, str(SCRIPTS))
path = SCRIPTS / "blocking-finding-gates.py"
spec = importlib.util.spec_from_file_location("gates", path)
assert spec and spec.loader
gates = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gates)


def bead(key, labels, status="open", metadata=None, dependencies=(), assignee=""):
    return {
        "id": key,
        "labels": labels,
        "status": status,
        "assignee": assignee,
        "metadata": metadata or {},
        "dependencies": [
            {"type": gates.BLOCKS_RELATION, "depends_on_id": dependency}
            for dependency in dependencies
        ],
    }


FAKE_BD = """#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

state_path = Path(os.environ["GATE_TEST_STATE"])
log_path = Path(os.environ["GATE_TEST_LOG"])
state = json.loads(state_path.read_text())
argv = sys.argv[1:]
log_path.write_text(log_path.read_text() + json.dumps(argv) + "\\n")

def row(bead_id):
    return next(item for item in state if item["id"] == bead_id)

if argv[:1] == ["show"]:
    print(json.dumps([row(argv[1])]))
elif argv[:1] == ["list"]:
    print(json.dumps(state))
elif argv[:1] == ["create"]:
    assert "--parent" not in argv, "bd 1.3.0 rejects create --parent"
    gate = argv[argv.index("--id") + 1]
    assert gate == "f-sanity"
    state.append({
        "id": gate, "status": "open", "assignee": "", "labels": ["stage:dev-sanity"],
        "metadata": json.loads(argv[argv.index("--metadata") + 1]), "dependencies": [],
    })
elif argv[:1] == ["update"]:
    assert argv == ["update", "f-sanity", "--parent", "s"]
elif argv[:2] == ["dep", "add"]:
    target, dependency = argv[2:4]
    assert argv[4:] == ["--type", "blocks"]
    row(target)["dependencies"].append({"type": "blocks", "depends_on_id": dependency})
else:
    raise AssertionError(f"unexpected bd argv: {argv}")

state_path.write_text(json.dumps(state))
"""


class BlockingGateTests(unittest.TestCase):
    def test_only_open_downstream_work_is_selected(self):
        rows = [
            bead("s", [gates.DEV_LABEL]),
            bead("s-sanity", [gates.SANITY_LABEL], metadata={"dev_bead": "s"}, dependencies=["s"]),
            bead("open", [gates.DEV_LABEL], dependencies=["s-sanity"]),
            bead("active", [gates.DEV_LABEL], "in_progress", dependencies=["s-sanity"]),
            bead("child", [gates.FIX_LABEL], metadata={"sprint_bead": "open"}),
        ]
        finding = bead("f", [gates.FINDING_LABEL], metadata={"severity": "blocking", "sprint_bead": "s"})
        self.assertEqual(gates.targets(finding, rows), ("s-sanity", ["child", "open"]))
        finding["metadata"]["severity"] = "important"
        self.assertEqual(gates.targets(finding, rows), (None, []))

    def test_deferred_finding_does_not_gate_what_it_waits_on(self):
        rows = [
            bead("s", [gates.DEV_LABEL]),
            bead("s-sanity", [gates.SANITY_LABEL], metadata={"dev_bead": "s"}, dependencies=["s"]),
            bead("open", [gates.DEV_LABEL], dependencies=["s-sanity"]),
            bead("open-sanity", [gates.SANITY_LABEL], metadata={"dev_bead": "open"}, dependencies=["open"]),
            bead("child", [gates.FIX_LABEL], metadata={"sprint_bead": "open"}),
        ]
        finding = bead("f", [gates.FINDING_LABEL], metadata={"severity": "blocking", "sprint_bead": "s"}, dependencies=["open-sanity"])
        self.assertEqual(gates.targets(finding, rows), ("s-sanity", []))

    def test_real_bd_argv_and_retry_are_idempotent(self):
        rows = [
            bead("s", [gates.DEV_LABEL]),
            bead("s-sanity", [gates.SANITY_LABEL], metadata={"dev_bead": "s"}, dependencies=["s"]),
            bead("open", [gates.DEV_LABEL], dependencies=["s-sanity"]),
            bead("active", [gates.DEV_LABEL], "in_progress", dependencies=["s-sanity"]),
            bead("child", [gates.FIX_LABEL], metadata={"sprint_bead": "open"}),
            bead("f", [gates.FINDING_LABEL], metadata={"severity": "blocking", "sprint_bead": "s"}),
            bead("important", [gates.FINDING_LABEL], metadata={"severity": "important", "sprint_bead": "s"}),
        ]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            state = root / "state.json"
            log = root / "argv.jsonl"
            fake_bd = root / "bd"
            state.write_text(json.dumps(rows))
            log.write_text("")
            fake_bd.write_text(textwrap.dedent(FAKE_BD))
            fake_bd.chmod(0o755)
            env = {**os.environ, "PATH": f"{root}:{os.environ['PATH']}", "GATE_TEST_STATE": str(state), "GATE_TEST_LOG": str(log)}
            env.pop("ATM_IDENTITY", None)
            command = [sys.executable, str(path), "--finding"]
            for finding in ("f", "f", "important"):
                run = subprocess.run([*command, finding], text=True, capture_output=True, env=env)
                self.assertEqual(run.returncode, 0, run.stderr)

            self.assertEqual(
                [json.loads(line) for line in log.read_text().splitlines()],
                [
                    ["show", "f", "--json"],
                    ["list", "--all", "-n", "0", "--json"],
                    ["create", "--id", "f-sanity", "--type", "task", "--status", "open",
                     "--title", "sanity gate for f", "--labels", "stage:dev-sanity",
                     "--metadata", '{"dev_bead": "f"}', "--silent"],
                    ["update", "f-sanity", "--parent", "s"],
                    ["dep", "add", "f-sanity", "f", "--type", "blocks"],
                    ["dep", "add", "child", "f-sanity", "--type", "blocks"],
                    ["dep", "add", "open", "f-sanity", "--type", "blocks"],
                    ["show", "f", "--json"],
                    ["list", "--all", "-n", "0", "--json"],
                    ["show", "important", "--json"],
                    ["list", "--all", "-n", "0", "--json"],
                ],
            )


if __name__ == "__main__":
    unittest.main()
