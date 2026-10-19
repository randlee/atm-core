#!/usr/bin/env python3
"""Triage bj-1 / ANC-022: replay the canonical review request byte-identically (2 repeats, parallel)."""
import concurrent.futures as cf, hashlib, json, os, sys, urllib.request, urllib.error
W = "/Users/randlee/github/atm-core-worktrees/plan/phase-bj"
T = W + "/docs/plans/phase-bj/anchor-runs/tuning/ANC-022/triage-bj-1"
sys.path.insert(0, W + "/.claude/skills/plan-anchoring/scripts")
import jev_judge as J  # noqa: E402
REQ = json.load(open(T + "/request.json"))
BASE = json.load(open(W + "/.claude/skills/plan-anchoring/assets/questions/plan.json"))
ROLES = BASE["roles"]
KEY = os.environ["TYPESAFE_API_KEY"].strip()
body = REQ["review"]["canonical_request"]
data = J.encode_body(body)
sha = lambda b: hashlib.sha256(b).hexdigest()

def post():
    r = urllib.request.Request(J.JEV_URL, data=data, method="POST", headers={"Authorization": f"Bearer {KEY}", "Content-Type": "application/json", "User-Agent": "plan-anchoring/anchor-tuning"})
    try:
        with urllib.request.urlopen(r, timeout=120) as resp:
            return resp.status, resp.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, None
    except Exception as e:  # noqa: BLE001
        return None, type(e).__name__

def one(rep):
    attempts = []
    for a in (1, 2):
        st, raw = post(); rec = {"attempt": a, "http_status": st}
        if st == 200 and raw:
            rec["raw_response"] = raw; rec["raw_response_sha256"] = sha(raw.encode())
            v = J.validate_response(json.loads(raw), body["questions"])
            if isinstance(v, J.Ok):
                attempts.append(rec)
                ans = {k: {"choice": x["choice"], "confidence": x["confidence"]} for k, x in v.value["answers"].items()}
                return {"correlation_id": f"triage-bj-1:ANC-022:sealed:atm-bj-1:r{rep}", "repeat": rep, "canonical_request_sha256": sha(data),
                        "attempts": attempts, "answers": ans,
                        "semantic_verdict": J.decide(v.value["answers"], ROLES, 0.0, "prohibition")[0],
                        "production_verdict_t0_6": J.decide(v.value["answers"], ROLES, 0.6, "prohibition")[0]}
            rec["problem"] = v.error.code
        else:
            rec["problem"] = raw or "http_error"
        attempts.append(rec)
    return {"correlation_id": f"triage-bj-1:ANC-022:sealed:atm-bj-1:r{rep}", "repeat": rep, "canonical_request_sha256": sha(data), "attempts": attempts, "error": True}

with cf.ThreadPoolExecutor(2) as ex:
    out = list(ex.map(one, (1, 2)))
open(T + "/runs/replay-canonical.jsonl", "w").write("".join(json.dumps(o, sort_keys=True) + "\n" for o in out))
open(T + "/runs/canonical-request.json", "wb").write(data)
print(json.dumps([{k: o.get(k) for k in ("repeat", "answers", "semantic_verdict", "production_verdict_t0_6", "error", "canonical_request_sha256")} for o in out], indent=1))
