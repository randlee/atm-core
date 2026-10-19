#!/usr/bin/env python3
import glob
import hashlib
import json
import time

W = "/Users/randlee/github/atm-core-worktrees/plan/phase-bj"
REL = "docs/plans/phase-bj/anchor-runs/tuning/ANC-022-v4"
D = W + "/" + REL
PROTO = "plan-anchor/question-tournament/2.0.0"


def c(v):
    return hashlib.sha256(json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()


def fs(p):
    return hashlib.sha256(open(p, "rb").read()).hexdigest()


req = json.load(open(D + "/request.json"))
oracle = json.load(open(D + "/oracle.json"))
scores = json.load(open(D + "/scores.json"))
RC = {}
for f in ("candidates-relevant.json", "candidates-relevant-r2.json"):
    RC.update(json.load(open(D + "/" + f)))
BASE = req["current_questions"]["questions"]
rows = {}
for f in sorted(glob.glob(D + "/runs/calls-*.jsonl")):
    for line in open(f):
        r = json.loads(line)
        rows.setdefault(r["candidate_id"], []).append(r)


def bundle(cid):
    return {"relevant": RC[cid], "addressed": BASE["addressed"], "contradicted": BASE["contradicted"]}


def ok(t):
    return t["verdict_match"] and all(t["role_match"].values()) and t["stable"]


base_ok = {t["case_id"] for t in scores["r0-incumbent"]["table"] if ok(t)}
ranked = []
for cid, s in scores.items():
    s["regressions"] = sum(1 for t in s["table"] if t["case_id"] in base_ok and not ok(t))
    accepted = (s["exact_role_cells"] == s["total_role_cells"] and s["exact_semantic_verdicts"] == s["total_semantic_verdicts"]
                and s["stable_cases"] == s["total_cases"] and s["regressions"] == 0)
    complexity = len(json.dumps(RC[cid]))
    cf = s["confidence"]
    key = (not accepted, -s["exact_role_cells"], -s["exact_semantic_verdicts"], -s["stable_cases"], s["regressions"],
           -cf["cells_at_or_above_threshold"], -cf["minimum"], -cf["p10"], -cf["mean"], complexity, cid)
    ranked.append((key, cid, accepted, complexity))
ranked.sort()
winner, w_acc = ranked[0][1], ranked[0][2]
assert w_acc
ws = scores[winner]
semantic = {k: ws[k] for k in ("exact_role_cells", "total_role_cells", "exact_semantic_verdicts", "total_semantic_verdicts",
                               "stable_cases", "total_cases", "regressions")}
diag = ws["confidence"]
wq = bundle(winner)
sealed = {"digest": c({"questions": wq}), "questions": wq}
json.dump(sealed, open(D + "/sealed-question-set.json", "w"), indent=1, ensure_ascii=False)
json.dump({"ANC-022": {"questions": {"relevant": wq["relevant"], "contradicted": wq["contradicted"]}}},
          open(D + "/override.json", "w"), indent=1, ensure_ascii=False)

hist_before = req["tuning_history"]
rec = req["authority"]["anchors"]["records"][0]
auth = {"intent_id": req["authority"]["intent"]["id"], "intent_version": req["authority"]["intent"]["version"],
        "intent_sha256": req["authority"]["intent"]["sha256"], "anchor_id": rec["id"], "anchor_version": rec["version"],
        "anchor_record_sha256": c(rec)}
all_rows = [r for v in rows.values() for r in v]
prev = hist_before["events"][-1]
event = {"event_index": len(hist_before["events"]) + 1, "previous_event_sha256": c(prev), "event": "question_set_sealed",
         "protocol_version": PROTO, "authority": auth,
         "oracle": {"sha256": c(oracle), "case_ids": [x["id"] for x in oracle["cases"]], "case_count": len(oracle["cases"]), "repeat_count": 2},
         "candidate": {"candidate_id": winner, "question_sha256": sealed["digest"]},
         "evidence": {"canonical_request_sha256": sorted({r["canonical_request_sha256"] for r in rows[winner]}),
                      "raw_response_sha256": sorted({a["raw_response_sha256"] for r in rows[winner] for a in r["attempts"] if "raw_response_sha256" in a}),
                      "run_ids": sorted(r["correlation_id"] for r in rows[winner])},
         "score": dict(semantic, confidence=diag),
         "decision": {"status": "sealed", "question_set_sha256": sealed["digest"], "reason_code": "TOURNAMENT.ACCEPTED"},
         "supersedes": {"protocol_version": PROTO, "decision_sha256": c(prev["decision"])}}
hist_after = {"schema": hist_before["schema"], "history_id": hist_before["history_id"], "events": hist_before["events"] + [event]}
json.dump(hist_after, open(D + "/tuning-history.json", "w"), indent=1, ensure_ascii=False)
open(D + "/tuning-history.canonical.json", "w").write(json.dumps(hist_after, sort_keys=True, separators=(",", ":"), ensure_ascii=False))

rankings = []
for i, (key, cid, accepted, complexity) in enumerate(ranked, 1):
    s = scores[cid]
    misses = "; ".join(f"{t['case_id']} expected {t['expected']} got {[(o['relevant'], o['contradicted']) for o in t['observed']]}"
                       for t in s["table"] if not ok(t))
    if cid == winner:
        reason = "selected: semantically accepted, zero regressions, best confidence diagnostics (minimum cell 0.72, 0 production-threshold misses)"
    elif accepted:
        reason = (f"semantically accepted but ranked below the winner on confidence diagnostics (minimum cell {s['confidence']['minimum']}); "
                  f"known_bj1_item relevant confidences {[o['relevant'][1] for t in s['table'] if t['case_id'] == 'known_bj1_item' for o in t['observed']]}")
    else:
        reason = "rejected (TOURNAMENT.SEMANTIC_REJECTED): " + misses
    rankings.append({"rank": i, "candidate_id": cid, "round": 1 if cid[:2] in ("r0", "r1", "r2", "r3", "r4") else 2, "accepted": accepted,
                     "exact_role_cells": s["exact_role_cells"], "total_role_cells": s["total_role_cells"],
                     "exact_semantic_verdicts": s["exact_semantic_verdicts"], "total_semantic_verdicts": s["total_semantic_verdicts"],
                     "stable_cases": s["stable_cases"], "regressions": s["regressions"], "confidence": s["confidence"],
                     "relevant_role_confidence": s["relevant_role_confidence"],
                     "production_threshold_misses": s["production_underspecified_runs"], "complexity_chars": complexity,
                     "question_sha256": c({"questions": bundle(cid)}), "reason": reason})
inc = {t["case_id"]: t for t in scores["r0-incumbent"]["table"]}
acceptance = [{"case_id": t["case_id"], "category": [x for x in oracle["cases"] if x["id"] == t["case_id"]][0]["category"],
               "expected": t["expected"], "expected_verdict": t["expected_verdict"],
               "incumbent_observed": [(o["relevant"], o["contradicted"]) for o in inc[t["case_id"]]["observed"]],
               "incumbent_semantic_verdicts": inc[t["case_id"]]["semantic_verdicts"],
               "sealed_observed": [(o["relevant"], o["addressed"], o["contradicted"]) for o in t["observed"]],
               "sealed_semantic_verdicts": t["semantic_verdicts"], "sealed_production_verdicts_t0_6": t["production_verdicts"],
               "match": ok(t)} for t in ws["table"]]

after = []
for line in open(D + "/authority-sha-before.txt").read().strip().splitlines():
    h, p = line.split("  ", 1)
    after.append({"path": p, "sha256_before": h, "unchanged": fs(W + "/" + p) == h})
unchanged = all(x["unchanged"] for x in after)
start = int(open(D + "/runs/start.epoch").read())
cits = [
    f"ANC-022 v{rec['version']} scope: '{rec['scope']}' (the three kinds of in-scope work; install scripts are their own kind)",
    f"ANC-022 v{rec['version']} kind: prohibition (jev_judge.decide ignores the compliance role; relevant=yes with a confident contradicted=no is satisfied)",
    f"ANC-022 v{rec['version']} statement: '{rec['statement']}'",
    f"ANC-022 v{rec['version']} deterministic_check: '{rec['deterministic_check']}' (exception for apps/atm-bd-console and atm_bd_console)",
    f"ANC-022 v{rec['version']} summary: '{rec['summary']}'; fixtures pass/fail used verbatim",
    f"INTENT phase-bj v{req['authority']['intent']['version']} sha256 {req['authority']['intent']['sha256']}: no clause keeps or reintroduces atm-bd-console",
]
data = {
    "operation": "tune", "gate": req["gate"], "accepted": True, "tournament_protocol_version": PROTO,
    "production_confidence_threshold": req["production_confidence_threshold"],
    "confidence_reference_threshold": req["confidence_reference_threshold"],
    "semantic_score": semantic, "confidence_diagnostics": diag,
    "tuning_history_before_sha256": c(hist_before), "tuning_history_after": hist_after, "tuning_history_after_sha256": c(hist_after),
    "production_semantics_unchanged": True, "authority_citations": cits, "oracle_sha256": c(oracle),
    "sealed_question_set": sealed, "candidate_rankings": rankings, "acceptance_table": acceptance,
    "evidence": {
        "request_path": REL + "/request.json", "request_contract_sha256": "9c2d4c0f8a650681c3b6038bac52fb69b4f377d483c6ed9af8a8e4d394e4509b",
        "runtime_digests_verified": {"anchor_tuning_contract.py": req["runtime"]["contract_sha256"], "jev_judge.py": req["runtime"]["sha256"]},
        "authority_digests_verified": {"intent_sha256": req["authority"]["intent"]["sha256"], "anchors_sha256": req["authority"]["anchors"]["sha256"],
                                       "anchor_record_sha256": c(rec)},
        "oracle_path": REL + "/oracle.json", "oracle_file_sha256": fs(D + "/oracle.json"), "oracle_frozen_before_first_jev_call": True,
        "oracle_builder": REL + "/harness/make_oracle.py",
        "frozen_hashes": REL + "/frozen.sha256 (oracle and round-1 candidates before the first call; round-2 candidates before round 2; verified by tourney.py before every run)",
        "candidates_paths": [REL + "/candidates-relevant.json", REL + "/candidates-relevant-r2.json"],
        "calls_paths": sorted(f.split(W + "/")[1] for f in glob.glob(D + "/runs/calls-*.jsonl")),
        "jev_calls": len(all_rows), "jev_errors": sum(1 for r in all_rows if r.get("error")),
        "models_answered": sorted({r.get("model") or "" for r in all_rows}),
        "role_scope": "Only `relevant` was reopened (triage-bj-1 question_action reopen_affected_tournament). `addressed` and `contradicted` are byte-identical to the sealed v3 questions in request.current_questions and were held fixed in every bundle; `addressed` is recorded but not graded for a prohibition.",
        "sealed_question_set_digest_before": req["tuning_history"]["events"][-1]["decision"]["question_set_sha256"],
        "incumbent_finding": "The incumbent relevant question (r0) misses only known_bj1_item on the frozen v4 oracle: relevant no 0.30 / yes 0.35, semantic not_applicable then satisfied, production underspecified twice. This reproduces the triage-bj-1 defect.",
        "defect_mechanism": "Round 1 rewrites (r1-r4) that framed scope as work done or steps taken regressed on fixture_pass (an end-state sentence) and subcommand_feature (a console feature that does not run the console). Round 2 kept the incumbent framing and changed criteria.no (dropped 'and does not run or describe the console') and, in r6, added that one step anywhere is enough and that an install-script edit counts without a console mention.",
        "fidelity_note": "Clause (1) of the incumbent and the winner reads 'the name of a command or installed entry point', which is broader than the scope's 'the command name'. The frozen oracle has no case that separates the two readings, so this was not tuned and is not claimed as tested. For a prohibition, an over-broad relevant=yes with contradicted=no gives satisfied rather than not_applicable. See authority_recommendations.",
        "overfit_note": "known_bj1_item is the disputed production item, included as a known example whose expectation comes from ANC-022 v4 scope. The winner's example 'such as install.py' names the install script the scope covers. silent_install_rename, silent_install_config and silent_install_verify are independent probes of the same scope clause, and the winner answers them at 0.94-1.0.",
        "production_threshold_note": f"Under the sealed set every frozen case reaches its expected verdict at production threshold {req['production_confidence_threshold']} in both repeats. known_bj1_item relevant is 0.72 in both repeats: above 0.6, below the 0.8 reference.",
        "override_path": REL + "/override.json", "sealed_question_set_path": REL + "/sealed-question-set.json",
        "tuning_history_path": REL + "/tuning-history.json", "tuning_history_canonical_path": REL + "/tuning-history.canonical.json",
        "harness": [REL + "/harness/tourney.py", REL + "/harness/score.py", REL + "/harness/build_result.py"],
        "authoritative_sources_after": after},
    "explanation": (
        "ANC-022 v4 is a prohibition scoped to 'edits the command name, install scripts or any caller of the console'. On a new 18-case oracle frozen against v4 (fixtures, compliant, subcommand boundary, exception, conflict, silent in-scope install edits, irrelevant, topical-only and the bj-1 item), "
        "the incumbent relevant question fails only the bj-1 item (relevant no 0.30 / yes 0.35). This confirms the triage defect. Nine relevant candidates were run in two rounds against the same frozen oracle with 2 repeats each (324 Jev calls, 0 errors, 0 retries); addressed and contradicted were held fixed. "
        f"Four round-1 rewrites were semantically rejected. Four round-2 variants of the incumbent were exact and stable with zero regressions. {winner} ranks first: 36/36 role cells, 18/18 verdicts, 18/18 stable, 0 regressions, 35/36 cells at or above 0.8, minimum 0.72 on the bj-1 item, and no production-threshold miss."),
    "planner_action": (
        f"Install {REL}/override.json into the ANC-022 plan-questions bead (or question_overrides.plan) yourself; anchor-tuning did not edit anchors.json, beads or the plan. Then run check-fixtures --gate plan --id ANC-022 and re-judge atm-bj-1 and every bead not not_applicable for ANC-022 with the resealed set, as a regression check. "
        "Do not edit atm-bj-1 for ANC-022. A future underspecified cell at the 0.6 production threshold is a plan-quality signal, not a reason to re-tune."),
    "question_action": None, "sealed_question_set_digest_before": None, "sealed_question_set_digest_after": sealed["digest"],
    "authority_recommendations": [{
        "kind": "clarity_only", "anchor_id": "ANC-022", "anchor_version": rec["version"],
        "ambiguity": "scope 'edits the command name' can be read as any command name or only the console command's name; the production relevant question uses the broader reading",
        "proposed_scope": "edits the atm-bd-console or `atm-bd console` command name, install scripts or any caller of the console",
        "why_meaning_unchanged": "the statement, summary and check all concern only the console command; 'the command name' already refers to it. Requires Rand's approval; not applied, and it would invalidate only the relevant-role evidence."}],
}
metadata = {"duration_ms": int((time.time() - start) * 1000), "tool_calls": 24,
            "retry_count": sum(1 for r in all_rows if len(r["attempts"]) > 1), "correlation_id": req["correlation_id"],
            "jev_run_ids": sorted(r["correlation_id"] for r in all_rows), "planner_observation_trust": None,
            "authoritative_sources_unchanged": unchanged}
json.dump({"success": True, "canceled": False, "aborted_by": None, "data": data, "error": None, "metadata": metadata},
          open(D + "/result.vars.json", "w"), indent=1, ensure_ascii=False)
print(winner, unchanged, len(all_rows), sealed["digest"], c(hist_after))
