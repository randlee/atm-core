#!/usr/bin/env python3
"""ANC-022 v4 relevant-role tournament: frozen oracle x relevant candidates x 2 repeats.

addressed and contradicted stay the sealed questions from request.current_questions; only `relevant` varies.
"""
import concurrent.futures as cf
import hashlib
import json
import os
import sys
import urllib.error
import urllib.request

W = "/Users/randlee/github/atm-core-worktrees/plan/phase-bj"
D = W + "/docs/plans/phase-bj/anchor-runs/tuning/ANC-022-v4"
sys.path.insert(0, W + "/.claude/skills/plan-anchoring/scripts")
import jev_judge as J  # noqa: E402

assert os.environ.get("PLAN_ANCHOR_ALLOW_REMOTE_JUDGE") == "1", "remote judge not allowed"
for line in open(D + "/frozen.sha256"):
    h, p = line.split()
    assert hashlib.sha256(open(p, "rb").read()).hexdigest() == h, f"frozen file changed: {p}"
REQ = json.load(open(D + "/request.json"))
ORACLE = json.load(open(D + "/oracle.json"))
RC = {}
for _f in ("candidates-relevant.json", "candidates-relevant-r2.json", "candidates-relevant-r3.json"):
    if os.path.exists(D + "/" + _f):
        RC.update(json.load(open(D + "/" + _f)))
BASE = REQ["current_questions"]["questions"]
ROLES = REQ["current_questions"]["roles"]
KEY = os.environ["TYPESAFE_API_KEY"].strip()
ANCHOR = {"id": "ANC-022", "statement": ORACLE["anchor_statement"], "scope": ORACLE["anchor_scope"]}
PROD = REQ["production_confidence_threshold"]
only = sys.argv[1:]


def sha(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def bundle(cid):
    q = {k: BASE[k] for k in ("relevant", "addressed", "contradicted")}
    q["relevant"] = RC[cid]
    return q


def post(body_bytes: bytes):
    req = urllib.request.Request(J.JEV_URL, data=body_bytes, method="POST",
                                 headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json",
                                          "User-Agent": "plan-anchoring/anchor-tuning"})
    try:
        with urllib.request.urlopen(req, timeout=180) as resp:
            return resp.status, resp.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        return exc.code, None
    except Exception as exc:  # noqa: BLE001
        return None, type(exc).__name__


def one(cid, case, rep):
    questions = bundle(cid)
    body = {"model": J.DEFAULT_MODEL, "state": J.build_state(ANCHOR, {"id": case["item_id"], "title": "", "text": case["text"]}),
            "questions": questions}
    data = J.encode_body(body)
    cor = f"ANC-022-v4:{cid}:{case['id']}:r{rep}"
    attempts = []
    for attempt in (1, 2):  # one retry of a recoverable failure
        status, raw = post(data)
        rec = {"attempt": attempt, "http_status": status}
        if status == 200 and raw:
            rec["raw_response"] = raw
            rec["raw_response_sha256"] = sha(raw.encode("utf-8"))
            parsed = json.loads(raw)
            v = J.validate_response(parsed, questions)
            if isinstance(v, J.Ok):
                attempts.append(rec)
                answers = {k: {"choice": a["choice"], "confidence": a["confidence"]} for k, a in v.value["answers"].items()}
                verdict, _ = J.decide(v.value["answers"], ROLES, 0.0, "prohibition")
                prod, _ = J.decide(v.value["answers"], ROLES, PROD, "prohibition")
                return {"correlation_id": cor, "candidate_id": cid, "case_id": case["id"], "repeat": rep,
                        "canonical_request_sha256": sha(data), "canonical_request": data.decode("utf-8"),
                        "attempts": attempts, "answers": answers, "semantic_verdict": verdict,
                        "production_verdict": prod, "model": v.value.get("model") or parsed.get("model")}
            rec["problem"] = v.error.code
        else:
            rec["problem"] = raw or "http_error"
        attempts.append(rec)
    return {"correlation_id": cor, "candidate_id": cid, "case_id": case["id"], "repeat": rep,
            "canonical_request_sha256": sha(data), "canonical_request": data.decode("utf-8"), "attempts": attempts, "error": True}


jobs = [(cid, case, rep) for cid in only for case in ORACLE["cases"] for rep in (1, 2)]
with cf.ThreadPoolExecutor(max_workers=4) as ex:
    results = list(ex.map(lambda j: one(*j), jobs))
results.sort(key=lambda r: r["correlation_id"])
tag = "-".join(only) if only else "all"
with open(f"{D}/runs/calls-{tag}.jsonl", "w") as fh:
    for r in results:
        fh.write(json.dumps(r, sort_keys=True, ensure_ascii=False) + "\n")
print(len(results), sum(1 for r in results if r.get("error")), sum(1 for r in results if len(r["attempts"]) > 1))
