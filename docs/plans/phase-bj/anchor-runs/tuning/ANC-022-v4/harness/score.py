#!/usr/bin/env python3
import glob
import json
import statistics
import sys

D = "/Users/randlee/github/atm-core-worktrees/plan/phase-bj/docs/plans/phase-bj/anchor-runs/tuning/ANC-022-v4"
O = json.load(open(D + "/oracle.json"))
REF = 0.8
GRADED = ("relevant", "contradicted")
cases = {c["id"]: c for c in O["cases"]}
rows = {}
for f in sorted(glob.glob(D + "/runs/calls-*.jsonl")):
    for line in open(f):
        r = json.loads(line)
        rows.setdefault(r["candidate_id"], {}).setdefault(r["case_id"], []).append(r)

summary = {}
for cid, per in sorted(rows.items()):
    exact_cells = verd = stable = 0
    table, cell_min = [], []
    for case_id, c in cases.items():
        ok_rs = [r for r in sorted(per.get(case_id, []), key=lambda r: r["repeat"]) if not r.get("error")]
        tuples = [tuple(r["answers"][k]["choice"] for k in GRADED) + (r["semantic_verdict"],) for r in ok_rs]
        is_stable = len(ok_rs) == 2 and tuples[0] == tuples[1]
        stable += is_stable
        role_ok = {}
        for role in GRADED:
            m = len(ok_rs) == 2 and all(r["answers"][role]["choice"] == c["expected"][role] for r in ok_rs)
            role_ok[role] = m
            exact_cells += m
            cell_min.append(min(r["answers"][role]["confidence"] for r in ok_rs) if ok_rs else 0.0)
        vmatch = len(ok_rs) == 2 and all(r["semantic_verdict"] == c["verdict"] for r in ok_rs)
        verd += vmatch
        table.append({"case_id": case_id, "expected": c["expected"], "expected_verdict": c["verdict"],
                      "observed": [{k: [r["answers"][k]["choice"], r["answers"][k]["confidence"]] for k in ("relevant", "addressed", "contradicted")} for r in ok_rs],
                      "semantic_verdicts": [r["semantic_verdict"] for r in ok_rs],
                      "production_verdicts": [r["production_verdict"] for r in ok_rs],
                      "role_match": role_ok, "verdict_match": vmatch, "stable": is_stable})
    n = len(cases)
    cs = sorted(cell_min)
    rel_min = sorted(min(o["relevant"][1] for o in t["observed"]) for t in table)
    summary[cid] = {"exact_role_cells": exact_cells, "total_role_cells": 2 * n, "exact_semantic_verdicts": verd,
                    "total_semantic_verdicts": n, "stable_cases": stable, "total_cases": n,
                    "confidence": {"threshold": REF, "cells_at_or_above_threshold": sum(1 for x in cs if x >= REF), "total_cells": len(cs),
                                   "minimum": cs[0], "p10": cs[max(0, int(round(0.1 * len(cs))) - 1)],
                                   "median": statistics.median(cs), "mean": round(statistics.mean(cs), 4)},
                    "relevant_role_confidence": {"minimum": rel_min[0], "mean": round(statistics.mean(rel_min), 4),
                                                 "cells_at_or_above_0_8": sum(1 for x in rel_min if x >= REF)},
                    "production_underspecified_runs": sum(1 for t in table for p in t["production_verdicts"] if p != t["expected_verdict"]),
                    "table": table}
json.dump(summary, open(D + "/scores.json", "w"), indent=1)
for cid, s in summary.items():
    print(cid, s["exact_role_cells"], "/", s["total_role_cells"], "verd", s["exact_semantic_verdicts"], "/", s["total_semantic_verdicts"],
          "stable", s["stable_cases"], "conf>=.8", s["confidence"]["cells_at_or_above_threshold"], "min", s["confidence"]["minimum"],
          "rel", s["relevant_role_confidence"], "prod_miss", s["production_underspecified_runs"])
    if "-v" in sys.argv:
        for t in s["table"]:
            if not (t["verdict_match"] and all(t["role_match"].values()) and t["stable"]) or "-vv" in sys.argv:
                print("   ", "MISS" if not t["verdict_match"] else "    ", t["case_id"], t["expected"], [(o["relevant"], o["contradicted"]) for o in t["observed"]], t["semantic_verdicts"], t["production_verdicts"])
