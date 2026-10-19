#!/usr/bin/env python3
"""Testability rubric tournament: frozen oracle, two byte-identical Jev calls per
case, a third call only on a first-pair choice disagreement, per-cell scoring.

usage: tournament.py <request.json> <evidence_dir> <pool.json> <candidate_id>... [--dedupe]
"""
import concurrent.futures as cf
import hashlib, json, os, pathlib, sys, threading, time

REQ_PATH, EVID, POOL = sys.argv[1], pathlib.Path(sys.argv[2]), sys.argv[3]
CAND_IDS = [a for a in sys.argv[4:] if not a.startswith("--")]
DO_DEDUPE = "--dedupe" in sys.argv

req = json.load(open(REQ_PATH))
sys.path.insert(0, str(pathlib.Path(req["runtime"]["script"]).parent))
import jev_judge as J  # noqa: E402

ROLES = ("single_constraint", "observable", "faithful", "scoped", "self_contained")
THRESH = req["production_confidence_threshold"]
REF = req["confidence_reference_threshold"]
MODEL = J.DEFAULT_MODEL
CID = req["correlation_id"]
LOCK = threading.Lock()
CALLS = EVID / "calls.jsonl"


def canon(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def sha(b):
    return hashlib.sha256(b).hexdigest()


oracle = json.load(open(EVID / "oracle.json"))
corpus = json.load(open(req["calibration_corpus"]["path"]))
assert sha(canon(corpus)) == req["calibration_corpus"]["sha256"] == oracle["corpus_sha256"]
assert sha(canon(oracle["cases"])) == oracle["cases_sha256"]
pool = json.load(open(POOL))

api_key = os.environ.get("TYPESAFE_API_KEY", "").strip()
assert J.check_api_key(api_key).__class__.__name__ == "Ok"


def redact(text):
    return text.replace(api_key, "[REDACTED]") if api_key else text


def one_call(corr, body, questions):
    """One live call with one retry for a recoverable failure; both records kept."""
    data = J.encode_body(body)
    records = []
    for attempt in (1, 2):
        t0 = time.time()
        got = J.call_jev(body, api_key, 120.0)
        rec = {"correlation_id": corr, "attempt": attempt, "request_sha256": sha(data), "elapsed_s": round(time.time() - t0, 2)}
        if got.__class__.__name__ == "Ok":
            raw = got.value
            checked = J.validate_response(raw, questions)
            rec["raw_response"] = json.loads(redact(json.dumps(raw)))
            rec["run_id"] = raw.get("run_id") or raw.get("id")
            if checked.__class__.__name__ == "Ok":
                rec["ok"] = True
                records.append(rec)
                break
            rec["ok"] = False
            rec["error"] = checked.error.to_json()["error"]
        else:
            rec["ok"] = False
            rec["error"] = got.error.to_json()["error"]
        records.append(rec)
        if not (rec.get("error") or {}).get("recoverable", False):
            break
    with LOCK:
        with open(CALLS, "a") as fh:
            for r in records:
                fh.write(json.dumps(r, sort_keys=True) + "\n")
    last = records[-1]
    return last, data, len(records) - 1


def classify(cells, expected):
    """cells: list of (choice, confidence) per call. Mirrors rubric_confirmation_outcome per cell."""
    first_agree = cells[0][0] == cells[1][0]
    if first_agree:
        choice, confs, matching = cells[0][0], [cells[0][1], cells[1][1]], [1, 2]
        stable = True
    else:
        confident = {c: [i + 1 for i, (g, p) in enumerate(cells) if g == c and p >= THRESH] for c in ("yes", "no", "unclear")}
        supported = [c for c, v in confident.items() if len(v) >= 2]
        competing = [c for c, v in confident.items() if v]
        if len(supported) == 1 and len(competing) == 1:
            choice = supported[0]
            matching = confident[choice]
            confs = [cells[i - 1][1] for i in matching]
            stable = True
        elif len(competing) > 1:
            return {"status": "unstable", "choice": None, "min_confidence": None, "stable": False, "confident": False,
                    "exact": False, "matching_calls": [], "third_call": True}
        else:
            return {"status": "inconclusive", "choice": None, "min_confidence": None, "stable": True, "confident": False,
                    "exact": False, "matching_calls": [], "third_call": True}
    mc = min(confs)
    confident_ok = mc >= THRESH
    exact = choice == expected
    status = "inconclusive" if not confident_ok else ("match" if exact else "mismatch")
    return {"status": status, "choice": choice, "min_confidence": round(mc, 3), "stable": stable, "confident": confident_ok,
            "exact": exact, "matching_calls": matching, "third_call": not first_agree, "at_reference": mc >= REF}


def eval_case(cand_id, questions, case):
    body = {"model": MODEL, "questions": questions, "state": {"anchor": case["anchor"]}}
    corr = f"{CID}:{cand_id}:{case['case_id']}"
    with cf.ThreadPoolExecutor(2) as ex:
        futs = [ex.submit(one_call, f"{corr}:call-{n}", body, questions) for n in (1, 2)]
        results = [f.result() for f in futs]
    retries = sum(r[2] for r in results)
    identical = results[0][1] == results[1][1]
    if not all(r[0]["ok"] for r in results):
        return {"case_id": case["case_id"], "error": [r[0].get("error") for r in results], "retries": retries, "identical": identical}
    answers = [r[0]["raw_response"]["answers"] for r in results]
    run_ids = [r[0]["run_id"] for r in results]
    choices = [[J.answer_choice(a[q])[0] for q in ROLES] for a in answers]
    third = None
    if choices[0] != choices[1]:
        third = one_call(f"{corr}:call-3", body, questions)
        retries += third[2]
        identical = identical and third[1] == results[0][1]
        if not third[0]["ok"]:
            return {"case_id": case["case_id"], "error": [third[0].get("error")], "retries": retries, "identical": identical}
        answers.append(third[0]["raw_response"]["answers"])
        run_ids.append(third[0]["run_id"])
    cells = {}
    for q in ROLES:
        cs = [J.answer_choice(a[q]) for a in answers]
        if third is None:
            cs = cs[:2]
        cells[q] = dict(classify(cs, case["expected"][q]), expected=case["expected"][q],
                        calls=[{"choice": c, "confidence": round(p, 3)} for c, p in cs])
    return {"case_id": case["case_id"], "cells": cells, "run_ids": run_ids, "request_sha256": sha(results[0][1]),
            "retries": retries, "identical": identical, "calls": len(answers)}


def eval_candidate(cand_id):
    questions = pool["candidates"][cand_id]["questions"]
    with cf.ThreadPoolExecutor(1) as ex:  # cases sequential within one candidate evaluator
        cases = list(ex.map(lambda c: eval_case(cand_id, questions, c), oracle["cases"]))
    flat = [c for case in cases if "cells" in case for c in case["cells"].values()]
    total = len(oracle["cases"]) * len(ROLES)
    score = {"exact_cells": sum(1 for c in flat if c["exact"] and c["confident"]),
             "confident_cells": sum(1 for c in flat if c["confident"]),
             "stable_cells": sum(1 for c in flat if c["stable"]),
             "total_cells": total}
    summary = {"two_call_cells": len(flat), "third_call_cells": sum(1 for c in flat if c["third_call"]),
               "inconclusive_cells": sum(1 for c in flat if c["status"] == "inconclusive"),
               "unstable_cells": sum(1 for c in flat if c["status"] == "unstable")}
    errors = [c for c in cases if "error" in c]
    res = {"candidate_id": cand_id, "digest": pool["candidates"][cand_id]["digest"],
           "complexity_chars": pool["candidates"][cand_id]["complexity_chars"], "cases": cases,
           "semantic_score": score, "confirmation_summary": summary,
           "mismatches": sum(1 for c in flat if c["status"] == "mismatch"),
           "at_reference_cells": sum(1 for c in flat if c.get("at_reference")),
           "errors": len(errors), "retries": sum(c["retries"] for c in cases),
           "repeat_requests_byte_identical": all(c["identical"] for c in cases),
           "accepted": (not errors and all(score[k] == total for k in ("exact_cells", "confident_cells", "stable_cells")))}
    (EVID / "results").mkdir(exist_ok=True)
    (EVID / "results" / f"{cand_id}.json").write_text(json.dumps(res, indent=1, sort_keys=True) + "\n")
    return res


def eval_dedupe():
    qs = J.load_simple_question_set("dedupe", (J.DEDUPE_QUESTION,)).value["questions"]
    out = []
    for p in corpus["dedupe_pairs"]:
        body = {"model": MODEL, "questions": qs, "state": {
            "a": {"id": p["pair_id"] + ":a", "statement": p["a"]["statement"]},
            "b": {"id": p["pair_id"] + ":b", "statement": p["b"]["statement"]}}}
        corr = f"{CID}:dedupe:{p['pair_id']}"
        with cf.ThreadPoolExecutor(2) as ex:
            rs = [f.result() for f in [ex.submit(one_call, f"{corr}:call-{n}", body, qs) for n in (1, 2)]]
        identical = rs[0][1] == rs[1][1]
        cs = [J.answer_choice(r[0]["raw_response"]["answers"][J.DEDUPE_QUESTION]) for r in rs]
        runs = [r[0]["run_id"] for r in rs]
        if cs[0][0] != cs[1][0]:
            t = one_call(f"{corr}:call-3", body, qs)
            cs.append(J.answer_choice(t[0]["raw_response"]["answers"][J.DEDUPE_QUESTION]))
            runs.append(t[0]["run_id"])
        cell = classify(cs, p["expected"])
        out.append(dict(cell, pair_id=p["pair_id"], expected=p["expected"], run_ids=runs, identical=identical,
                        request_sha256=sha(rs[0][1]), calls=[{"choice": c, "confidence": round(x, 3)} for c, x in cs],
                        retries=sum(r[2] for r in rs)))
    score = {"exact_cells": sum(1 for c in out if c["exact"] and c["confident"]),
             "confident_cells": sum(1 for c in out if c["confident"]),
             "stable_cells": sum(1 for c in out if c["stable"]), "total_cells": len(out)}
    res = {"question_set_sha256": sha(canon({"questions": qs})), "input_fields": ["a.id", "a.statement", "b.id", "b.statement"],
           "quotes_sent": False, "pairs": out, "dedupe_score": score,
           "repeat_requests_byte_identical": all(c["identical"] for c in out)}
    (EVID / "dedupe.json").write_text(json.dumps(res, indent=1, sort_keys=True) + "\n")
    return res


if __name__ == "__main__":
    with cf.ThreadPoolExecutor(4) as ex:  # at most four candidate evaluators
        futs = {ex.submit(eval_candidate, c): c for c in CAND_IDS}
        if DO_DEDUPE:
            dd = ex.submit(eval_dedupe)
        for f in cf.as_completed(futs):
            r = f.result()
            print(r["candidate_id"], r["semantic_score"], r["confirmation_summary"], "mismatch", r["mismatches"],
                  "err", r["errors"], "accepted", r["accepted"], flush=True)
        if DO_DEDUPE:
            print("dedupe", dd.result()["dedupe_score"], flush=True)
