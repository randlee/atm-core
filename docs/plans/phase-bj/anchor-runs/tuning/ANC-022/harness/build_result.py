#!/usr/bin/env python3
import glob
import hashlib
import json

W = "/Users/randlee/github/atm-core-worktrees/plan/phase-bj"
D = W + "/docs/plans/phase-bj/anchor-runs/tuning/ANC-022"
P = W + "/.claude/skills/plan-anchoring"


def c(v):
    return hashlib.sha256(json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()


def fs(p):
    return hashlib.sha256(open(p, "rb").read()).hexdigest()


req = json.load(open(D + "/request.json"))
oracle = json.load(open(D + "/oracle.json"))
scores = json.load(open(D + "/scores.json"))
CC = json.load(open(D + "/candidates.json"))
RC = json.load(open(D + "/candidates-relevant.json"))
base = json.load(open(P + "/assets/questions/plan.json"))["questions"]
rows = {}
for f in sorted(glob.glob(D + "/runs/calls-*.jsonl")):
    for line in open(f):
        r = json.loads(line)
        rows.setdefault(r["candidate_id"], []).append(r)


def bundle(cid):
    q = dict(base)
    if "+" in cid:
        rid, ccid = cid.split("+")
        q["relevant"] = RC[rid]
    else:
        ccid = cid
    q["contradicted"] = CC[ccid]
    return q


base_ok = {t["case_id"] for t in scores["c0-default"]["table"] if t["verdict_match"] and all(t["role_match"].values())}
ranked = []
for cid, s in scores.items():
    regress = sum(1 for t in s["table"] if t["case_id"] in base_ok and not (t["verdict_match"] and all(t["role_match"].values())))
    s["regressions"] = regress
    accepted = (s["exact_role_cells"] == s["total_role_cells"] and s["exact_semantic_verdicts"] == s["total_semantic_verdicts"]
                and s["stable_cases"] == s["total_cases"] and regress == 0)
    q = bundle(cid)
    complexity = sum(len(json.dumps(q[k])) for k in ("relevant", "contradicted"))
    key = (not accepted, -s["exact_role_cells"], -s["exact_semantic_verdicts"], -s["stable_cases"], regress,
           -s["confidence"]["cells_at_or_above_threshold"], -s["confidence"]["minimum"], -s["confidence"]["mean"], complexity, cid)
    ranked.append((key, cid, accepted, complexity))
ranked.sort()
winner = ranked[0][1]
assert ranked[0][2]

sealed_before = req["sealed_question_set"]
wq = bundle(winner)
sealed_after = {"digest": c({"questions": wq}), "questions": wq}
override = {"ANC-022": {"questions": {"relevant": wq["relevant"], "contradicted": wq["contradicted"]}}}
json.dump(override, open(D + "/override.json", "w"), indent=1, ensure_ascii=False)
json.dump(sealed_after, open(D + "/sealed-question-set.json", "w"), indent=1, ensure_ascii=False)

a22 = [r for r in req["authority"]["anchors"]["records"] if r["id"] == "ANC-022"][0]
auth = {"intent_id": "phase-bj", "intent_version": 1, "intent_sha256": req["authority"]["intent"]["sha256"],
        "anchor_id": "ANC-022", "anchor_version": a22["version"], "anchor_record_sha256": c(a22)}
oracle_block = {"sha256": c(oracle), "case_ids": [x["id"] for x in oracle["cases"]], "case_count": len(oracle["cases"]), "repeat_count": 2}
PROTO = "plan-anchor/question-tournament/2.0.0"
events = []


def add(event, cid, status, qsha, reason):
    s = scores[cid]
    rs = rows[cid]
    ev = {"event_index": len(events) + 1, "previous_event_sha256": c(events[-1]) if events else None, "event": event,
          "protocol_version": PROTO, "authority": auth, "oracle": oracle_block,
          "candidate": {"candidate_id": cid, "question_sha256": c({"questions": bundle(cid)})},
          "evidence": {"canonical_request_sha256": sorted({r["canonical_request_sha256"] for r in rs}),
                       "raw_response_sha256": sorted({a["raw_response_sha256"] for r in rs for a in r["attempts"] if "raw_response_sha256" in a}),
                       "run_ids": sorted(r["correlation_id"] for r in rs)},
          "score": {"exact_role_cells": s["exact_role_cells"], "total_role_cells": s["total_role_cells"],
                    "exact_semantic_verdicts": s["exact_semantic_verdicts"], "total_semantic_verdicts": s["total_semantic_verdicts"],
                    "stable_cases": s["stable_cases"], "total_cases": s["total_cases"], "regressions": s["regressions"],
                    "confidence": s["confidence"]},
          "decision": {"status": status, "question_set_sha256": qsha, "reason_code": reason},
          "supersedes": None if not events else {"protocol_version": PROTO, "decision_sha256": c(events[-1]["decision"])}}
    events.append(ev)


for key, cid, accepted, _ in sorted(ranked, key=lambda x: (x[1] != "c0-default", x[1])):
    if cid == winner:
        continue
    add("candidate_evaluated" if accepted else "question_set_rejected", cid, "rejected", None,
        "TOURNAMENT.RANKED_BELOW_WINNER" if accepted else "TOURNAMENT.SEMANTIC_REJECTED")
add("question_set_sealed", winner, "sealed", sealed_after["digest"], "TOURNAMENT.ACCEPTED")
history = {"schema": "plan-anchor/tuning-history/2", "history_id": "ANC-022-plan", "events": events}
json.dump(history, open(D + "/tuning-history.json", "w"), indent=1, ensure_ascii=False)
open(D + "/tuning-history.canonical.json", "w").write(json.dumps(history, sort_keys=True, separators=(",", ":"), ensure_ascii=False))

rankings = []
for i, (key, cid, accepted, complexity) in enumerate(ranked, 1):
    s = scores[cid]
    reason = ("selected: accepted, highest confidence diagnostics" if cid == winner else
              "accepted but ranked below the winner on confidence diagnostics" if accepted else
              "rejected: " + "; ".join(f"{t['case_id']} expected {t['expected']} got {[(o['relevant'][0], o['contradicted'][0]) for o in t['observed']]}"
                                       for t in s["table"] if not (t["verdict_match"] and all(t["role_match"].values()) and t["stable"])))
    rankings.append({"rank": i, "candidate_id": cid, "accepted": accepted, "exact_role_cells": s["exact_role_cells"],
                     "total_role_cells": s["total_role_cells"], "exact_semantic_verdicts": s["exact_semantic_verdicts"],
                     "total_semantic_verdicts": s["total_semantic_verdicts"], "stable_cases": s["stable_cases"],
                     "regressions": s["regressions"], "confidence": s["confidence"], "complexity_chars": complexity,
                     "question_sha256": c({"questions": bundle(cid)}), "reason": reason})
acceptance = [{"case_id": t["case_id"], "category": [x for x in oracle["cases"] if x["id"] == t["case_id"]][0]["category"],
               "expected": t["expected"], "expected_verdict": t["expected_verdict"],
               "default_observed": [(o["relevant"], o["contradicted"]) for o in [x for x in scores["c0-default"]["table"] if x["case_id"] == t["case_id"]][0]["observed"]],
               "default_semantic_verdicts": [x for x in scores["c0-default"]["table"] if x["case_id"] == t["case_id"]][0]["semantic_verdicts"],
               "sealed_observed": [(o["relevant"], o["contradicted"]) for o in t["observed"]],
               "sealed_semantic_verdicts": t["semantic_verdicts"], "sealed_production_verdicts_t0_6": t["production_verdicts_t0_6"],
               "match": t["verdict_match"] and all(t["role_match"].values()) and t["stable"]}
              for t in scores[winner]["table"]]
srcs = open(D + "/authority-sha-before.txt").read()
after = []
for line in srcs.strip().splitlines():
    h, p = line.split("  ", 1)
    after.append(fs(W + "/" + p) == h)
unchanged = all(after)
ncalls = sum(len(v) for v in rows.values())
data = {
    "operation": "triage", "gate": "planning", "disposition": "question_defect",
    "authority_citations": [
        "ANC-022 v3 statement: 'no atm-bd-console command must exist ... the directory apps/atm-bd-console and the module atm_bd_console are not that command'",
        "ANC-022 v3 context: 'answer to whether to drop the alias' and quote 'jes drop atm-bd-console' (the dropped thing is the hyphenated alias)",
        "ANC-022 v3 deterministic_check: 'command -v atm-bd-console finds nothing; rg atm-bd-console ... finds no invocation' (a space-separated `atm-bd console` matches neither)",
        "ANC-073 (locked) scope: 'atm-bd console pages' - locked authority requires work on the `atm-bd console` subcommand",
        "ANC-009 / ANC-001 (locked): the console and skill are subcommands of the atm-bd command",
        "intent.md phase-bj v1 (sha256 e5d9e1e4...): no clause keeps or reintroduces atm-bd-console"],
    "oracle_sha256": c(oracle),
    "sealed_question_set": sealed_after,
    "candidate_rankings": rankings,
    "acceptance_table": acceptance,
    "evidence": {
        "request_path": "docs/plans/phase-bj/anchor-runs/tuning/ANC-022/request.json",
        "request_sha256": c(req),
        "oracle_path": "docs/plans/phase-bj/anchor-runs/tuning/ANC-022/oracle.json",
        "oracle_file_sha256": fs(D + "/oracle.json"),
        "oracle_frozen_before_first_jev_call": True,
        "candidates_paths": ["docs/plans/phase-bj/anchor-runs/tuning/ANC-022/candidates.json", "docs/plans/phase-bj/anchor-runs/tuning/ANC-022/candidates-relevant.json"],
        "calls_paths": sorted(f.split(W + "/")[1] for f in glob.glob(D + "/runs/calls-*.jsonl")),
        "jev_calls": ncalls, "jev_errors": sum(1 for v in rows.values() for r in v if r.get("error")),
        "disputed_run": "docs/plans/phase-bj/anchor-runs/plan-20261019T011248Z-atm-phase-bj.json",
        "disputed_run_note": "run sent 5675 chars; item.txt now has 5637 chars (rewritten after the run). The current text was re-judged as oracle case known_root_item: default questions return contradicted=yes on both repeats (0.89, 0.92), so the failure reproduces on the current plan text.",
        "defect_demonstration": "Under the unchanged default contradicted question, three frozen boundary probes whose text never names atm-bd-console (subcommand_doc, subcommand_script, exception_directory_module) land contradicted on both repeats at 0.86-0.95; the authority labels them no-conflict. The approved fixtures land correctly under both default and sealed sets.",
        "relevance_note": "the default relevant question also misses the frozen silent_install probe (install-script edit, in ANC-022 scope) as not_applicable on both repeats; the affected tournament therefore reopened relevant as well. addressed is not graded: decide() ignores compliance for a prohibition.",
        "production_threshold_note": "production threshold stays 0.6. Under the sealed set, subcommand_feature lands relevant=yes at 0.62 and 0.59, so one repeat is underspecified at 0.6 in production; this is reported, not tuned away.",
        "override_path": "docs/plans/phase-bj/anchor-runs/tuning/ANC-022/override.json",
        "tuning_history_path": "docs/plans/phase-bj/anchor-runs/tuning/ANC-022/tuning-history.json",
        "tuning_history_sha256": c(history),
        "authoritative_source_hashes": "docs/plans/phase-bj/anchor-runs/tuning/ANC-022/authority-sha-before.txt (re-verified after all calls)"},
    "explanation": ("ANC-022 forbids a separate atm-bd-console executable and any invocation of it; by its own context it drops the alias of the console, and locked ANC-073 requires work on the `atm-bd console` subcommand. "
                    "The root text names only `atm-bd console` and `atm-bd skill`, never atm-bd-console, so the authority expects relevant=yes, contradicted=no (satisfied). "
                    "The default contradicted question reads the space-separated subcommand, and even the explicitly excepted directory/module, as the forbidden command: frozen probes that never name atm-bd-console land contradicted. "
                    "That is a question defect, proven on the frozen oracle independently of the live item. The reopened tournament (12 bundles, 14 cases, 2 repeats) sealed r3-touch-list (relevant) + c1-homonym-callout (contradicted) with 28/28 role cells, 14/14 verdicts, 14/14 stable, 0 regressions."),
    "planner_action": ("Do not edit the root or bj-4 for ANC-022. Install the override in docs/plans/phase-bj/anchor-runs/tuning/ANC-022/override.json into the ANC-022 plan-questions bead (or question_overrides.plan) yourself, run check-fixtures --gate plan --id ANC-022, then re-judge every bead not not_applicable for ANC-022 as a regression check. "
                       "A remaining underspecified cell at the 0.6 production threshold is a plan-quality signal, not a reason to re-tune."),
    "question_action": "reopen_affected_tournament",
    "sealed_question_set_digest_before": sealed_before["digest"],
    "sealed_question_set_digest_after": sealed_after["digest"],
    "authority_recommendations": [{
        "kind": "clarity_only", "anchor_id": "ANC-022", "anchor_version": 3,
        "ambiguity": "the statement names the excepted directory and module but not the `atm-bd console` subcommand, the closest homonym",
        "proposed_statement": "After installation no atm-bd-console command must exist, and no script, document, test or workflow in the atm-bd-orchestration repository may invoke the atm-bd-console command; the `atm-bd console` subcommand, the directory apps/atm-bd-console and the module atm_bd_console are not that command.",
        "why_meaning_unchanged": "the context says the alias is dropped, ANC-073 requires the `atm-bd console` pages, and the deterministic check cannot match the space-separated subcommand; the added words state what the record already implies. Requires Rand's approval; not applied."}],
}
metadata = {"duration_ms": 0, "tool_calls": 0, "retry_count": sum(1 for v in rows.values() for r in v if len(r["attempts"]) > 1),
            "correlation_id": req["correlation_id"], "jev_run_ids": sorted(r["correlation_id"] for v in rows.values() for r in v),
            "planner_observation_trust": "untrusted", "authoritative_sources_unchanged": unchanged}
json.dump({"success": True, "canceled": False, "aborted_by": None, "data": data, "error": None, "metadata": metadata},
          open(D + "/result.vars.json", "w"), indent=1, ensure_ascii=False)
print(winner, unchanged, ncalls, sealed_after["digest"])
