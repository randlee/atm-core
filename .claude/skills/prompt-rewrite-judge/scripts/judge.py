#!/usr/bin/env python3
"""Judge prompt rewrites with Jev: per-requirement keep/drop plus clarity.

judge.py --key key.json --original original.txt --out DIR [--client .claude/skills/atm-bd-orchestration/scripts/jev_client.py] CAND.txt...
Writes DIR/<cand>.req.json and DIR/<cand>.json (Jev answers) and prints one row per
candidate; each value is the choice and Jev's probability for it. A failed row carries the
client's exit code, JSON error, unparsed stdout and stderr verbatim.
"""
import argparse
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

MODEL = "jev-1.13.0"
MAX_REQUEST_BYTES = 96000  # jev_client.py refuses larger requests
KEEP = ("Requirement: {req}\nCompare state.candidate with state.original. Does state.candidate still "
        "explicitly require this, at least as clearly as state.original? Answer no if the candidate "
        "omits it, weakens it, or makes it optional.")
EXTRA = {
    "changes_rule": ("Does state.candidate add a rule, or change the order or meaning of an action, compared with state.original?", ["no", "yes"]),
    "unnecessary": ("Does state.candidate contain words an agent does not need to carry out state.requirements?", ["none", "some", "mostly"]),
    "ambiguous": ("Could an agent misread what state.candidate requires or in what order?", ["no", "yes"]),
    "clearer_than_original": ("Is state.candidate clearer for an agent than state.original?", ["yes", "same", "no"]),
}
HEADER = "cand | words | dropped (keep=no) | changes_rule | unnecessary | ambiguous | clearer"


def request(key, original, cand):
    q = {f"keep_{i}": {"type": "choice", "instructions": KEEP.format(req=r), "criteria": {"yes": None, "no": None}}
         for i, r in enumerate(key, 1)}
    for k, (text, opts) in EXTRA.items():
        q[k] = {"type": "choice", "instructions": text, "criteria": {o: None for o in opts}}
    return {"model": MODEL, "state": {"requirements": key, "original": original, "candidate": cand}, "questions": q}


def reduce(answers):
    """Jev answers -> {'dropped': [(n, p_no)], question: (choice, p_choice)}."""
    pick = lambda a: (a["choice"], a["probabilities"][a["choice"]])
    keeps = sorted((int(k.split("_")[1]), a) for k, a in answers.items() if k.startswith("keep_"))
    out = {"dropped": [(n, a["probabilities"]["no"]) for n, a in keeps if a["choice"] == "no"]}
    out.update({k: pick(answers[k]) for k in EXTRA})
    return out


def row(name, words, verdict, error=None):
    if verdict is None:
        return f"{name} | {words} | JEV FAILED {json.dumps(error)}"
    cell = lambda k: f"{verdict[k][0]}({verdict[k][1]:.2f})"
    drops = " ".join(f"R{n}({p:.2f})" for n, p in verdict["dropped"]) or "-"
    return " | ".join([name, str(words), drops] + [cell(k) for k in EXTRA])


def run(client, path):
    """-> (answers, None) or (None, the last attempt's verbatim error)."""
    for _ in range(2):  # one rerun on failure; never answer for Jev
        p = subprocess.run([sys.executable, str(client), "--request", str(path)], capture_output=True, text=True)
        try:
            d = json.loads(p.stdout)
        except json.JSONDecodeError:
            d = {}
        if d.get("success"):
            return d["data"]["answers"], None
        error = {"exit": p.returncode, "error": d.get("error"), "stdout": None if d else p.stdout, "stderr": p.stderr}
    return None, error


def main(argv=None):
    a = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    a.add_argument("--key", required=True, type=Path)
    a.add_argument("--original", required=True, type=Path)
    a.add_argument("--out", required=True, type=Path)
    a.add_argument("--client", type=Path, default=Path(__file__).resolve().parents[2] / "atm-bd-orchestration/scripts/jev_client.py")
    a.add_argument("cands", nargs="+", type=Path)
    o = a.parse_args(argv)
    key, orig = json.loads(o.key.read_text()), o.original.read_text()
    if not (isinstance(key, list) and key and all(isinstance(r, str) and r for r in key)):
        sys.exit("key.json must be a nonempty JSON list of requirement strings")
    o.out.mkdir(parents=True, exist_ok=True)
    jobs = []
    for c in o.cands:
        text = c.read_text()
        body = json.dumps(request(key, orig, text))
        if len(body.encode()) > MAX_REQUEST_BYTES:
            sys.exit(f"{c.stem}: request {len(body.encode())} bytes > {MAX_REQUEST_BYTES}")
        rp = o.out / f"{c.stem}.req.json"
        rp.write_text(body)
        jobs.append((c.stem, rp, len(text.split())))
    with ThreadPoolExecutor(8) as ex:
        answers = list(ex.map(lambda j: run(o.client, j[1]), jobs))
    print(HEADER)
    for (name, _, words), (ans, error) in zip(jobs, answers):
        if ans is not None:
            (o.out / f"{name}.json").write_text(json.dumps(ans, indent=1))
        print(row(name, words, None if ans is None else reduce(ans), error))
    return 0 if all(ans for ans, _ in answers) else 2


if __name__ == "__main__":
    sys.exit(main())
