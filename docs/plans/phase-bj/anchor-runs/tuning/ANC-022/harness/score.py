#!/usr/bin/env python3
import glob
import json
import statistics
import sys

D = "/Users/randlee/github/atm-core-worktrees/plan/phase-bj/docs/plans/phase-bj/anchor-runs/tuning/ANC-022"
O = json.load(open(D + "/oracle.json"))
cases = {c["id"]: c for c in O["cases"]}
rows = {}
for f in sorted(glob.glob(D + "/runs/calls-*.jsonl")):
    for line in open(f):
        r = json.loads(line)
        rows.setdefault(r["candidate_id"], {}).setdefault(r["case_id"], []).append(r)

summary = {}
for cid, per in sorted(rows.items()):
    exact_cells = verd = stable = 0
    confs = []
    table = []
    for case_id, c in cases.items():
        rs = sorted(per.get(case_id, []), key=lambda r: r["repeat"])
        ok_rs = [r for r in rs if not r.get("error")]
        tuples = [tuple(r["answers"][k]["choice"] for k in ("relevant", "contradicted")) for r in ok_rs]
        is_stable = len(ok_rs) == 2 and tuples[0] == tuples[1]
        stable += is_stable
        cell_ok = {}
        for role in ("relevant", "contradicted"):
            match = len(ok_rs) == 2 and all(r["answers"][role]["choice"] == c["expected"][role] for r in ok_rs)
            cell_ok[role] = match
            exact_cells += match
            confs += [r["answers"][role]["confidence"] for r in ok_rs]
        vmatch = len(ok_rs) == 2 and all(r["semantic_verdict"] == c["verdict"] for r in ok_rs)
        verd += vmatch
        table.append({"case_id": case_id, "expected": c["expected"], "expected_verdict": c["verdict"],
                      "observed": [{k: [r["answers"][k]["choice"], r["answers"][k]["confidence"]] for k in ("relevant", "addressed", "contradicted")} for r in ok_rs],
                      "semantic_verdicts": [r["semantic_verdict"] for r in ok_rs],
                      "production_verdicts_t0_6": [r["production_verdict_t0_6"] for r in ok_rs],
                      "role_match": cell_ok, "verdict_match": vmatch, "stable": is_stable})
    n = len(cases)
    confs_sorted = sorted(confs)
    summary[cid] = {"exact_role_cells": exact_cells, "total_role_cells": 2 * n, "exact_semantic_verdicts": verd,
                    "total_semantic_verdicts": n, "stable_cases": stable, "total_cases": n,
                    "confidence": {"threshold": 0.8, "cells_at_or_above_threshold": sum(1 for x in confs if x >= 0.8) // 2 if False else None,
                                   "minimum": min(confs), "p10": confs_sorted[max(0, int(0.1 * len(confs)) - 1)] if confs else 0,
                                   "median": statistics.median(confs), "mean": round(statistics.mean(confs), 4)},
                    "table": table}
    # confidence per role cell: min across the two repeats
    cell_min = []
    for t in table:
        for role in ("relevant", "contradicted"):
            vals = [o[role][1] for o in t["observed"]]
            cell_min.append(min(vals) if vals else 0.0)
    summary[cid]["confidence"]["cells_at_or_above_threshold"] = sum(1 for x in cell_min if x >= 0.8)
    summary[cid]["confidence"]["total_cells"] = len(cell_min)
    cs = sorted(cell_min)
    summary[cid]["confidence"].update({"minimum": cs[0], "p10": cs[max(0, int(round(0.1 * len(cs))) - 1)],
                                       "median": statistics.median(cs), "mean": round(statistics.mean(cs), 4)})
json.dump(summary, open(D + "/scores.json", "w"), indent=1)
for cid, s in summary.items():
    print(cid, s["exact_role_cells"], "/", s["total_role_cells"], "verd", s["exact_semantic_verdicts"], "/", s["total_semantic_verdicts"],
          "stable", s["stable_cases"], "conf>=.8", s["confidence"]["cells_at_or_above_threshold"], "min", s["confidence"]["minimum"])
    if "-v" in sys.argv:
        for t in s["table"]:
            if not (t["verdict_match"] and all(t["role_match"].values()) and t["stable"]):
                print("   MISS", t["case_id"], t["expected"], t["observed"], t["semantic_verdicts"])
