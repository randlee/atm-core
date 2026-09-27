from __future__ import annotations
import importlib.util
import json
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

path = Path(__file__).parents[1] / "blocking-finding-gates.py"
spec = importlib.util.spec_from_file_location("gates", path); assert spec and spec.loader
gates = importlib.util.module_from_spec(spec); spec.loader.exec_module(gates)

def bead(key, labels, status="open", metadata=None, dependencies=(), assignee=""):
    return {"id": key, "labels": labels, "status": status, "assignee": assignee,
            "metadata": metadata or {}, "dependencies": [{"type": "blocks", "depends_on_id": x} for x in dependencies]}

class BlockingGateTests(unittest.TestCase):
    def test_only_open_downstream_work_is_selected(self):
        rows = [bead("s", ["stage:dev"]), bead("s-sanity", ["stage:dev-sanity"], metadata={"dev_bead":"s"}, dependencies=["s"]),
                bead("open", ["stage:dev"], dependencies=["s-sanity"]), bead("active", ["stage:dev"], "in_progress", dependencies=["s-sanity"]),
                bead("child", ["stage:fix"], metadata={"sprint_bead":"open"})]
        finding = bead("f", ["stage:finding"], metadata={"severity":"blocking", "sprint_bead":"s"})
        self.assertEqual(gates.targets(finding, rows), ("s-sanity", ["child", "open"]))
        finding["metadata"]["severity"] = "important"
        self.assertEqual(gates.targets(finding, rows), (None, []))

    def test_main_creates_exact_edges_once(self):
        rows = [
            bead("s", ["stage:dev"]),
            bead("s-sanity", [gates.SANITY_LABEL], metadata={"dev_bead": "s"}, dependencies=["s"]),
            bead("open", ["stage:dev"], dependencies=["s-sanity"]),
            bead("child", [gates.FIX_LABEL], metadata={"sprint_bead": "open"}),
        ]
        finding = bead("f", [gates.FINDING_LABEL], metadata={"severity": "blocking", "sprint_bead": "s"})
        writes: list[tuple[str, ...]] = []

        def fake_cmd(*args, capture=False):
            if args == ("bd", "show", "f", "--json"):
                return json.dumps([finding])
            if args == ("bd", "list", "--all", "-n", "0", "--json"):
                return json.dumps(rows)
            writes.append(args)
            if args[:2] == ("bd", "create"):
                rows.append(bead("f-sanity", [gates.SANITY_LABEL], metadata={"dev_bead": "f"}))
            elif args[:3] == ("bd", "dep", "add"):
                target, dependency = args[3], args[4]
                next(row for row in rows if row["id"] == target)["dependencies"].append(
                    {"type": "blocks", "depends_on_id": dependency}
                )
            return ""

        expected = [
            ("bd", "create", "--id", "f-sanity", "--type", "task", "--status", "open", "--parent", "f",
             "--title", "sanity gate for f", "--labels", gates.SANITY_LABEL,
             "--metadata", '{"dev_bead": "f"}', "--silent"),
            ("bd", "dep", "add", "child", "f-sanity", "--type", "blocks"),
            ("bd", "dep", "add", "open", "f-sanity", "--type", "blocks"),
        ]
        with patch.object(gates, "cmd", fake_cmd), patch.object(sys, "argv", ["blocking-finding-gates.py", "--finding", "f"]):
            self.assertEqual(gates.main(), 0)
            self.assertEqual(writes, expected)
            self.assertEqual(gates.main(), 0)
        self.assertEqual(writes, expected)

if __name__ == "__main__": unittest.main()
