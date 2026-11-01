#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# ///
"""atm-bd-lite gate: hold one hook file to its hooks/*.md spec (fenix-brief.md §B).

usage: atm-bd-lite-gate.py <hook.py> <hooks/spec.md> [--out run.json]

Deterministic checks run first and fail before Jev: rows == Transition arms (ANC-052), at most one
beads write and no create/claim/release (ANC-064), stack_bead only read (ANC-065), no subprocess/bd/gh
(ANC-066), <= 100 lines (ANC-070). Then one Jev request with two question sets: every Transitions row
is implemented by exactly one arm with no additional check or write (ANC-053); every arm and reply site
is listed as a row or an Errors entry (ANC-054). Writes the run JSON; exits 1 on any failure or "no",
2 when Jev cannot be asked.
"""
import ast, http.client, json, os, re, sys, time
from pathlib import Path

MODEL, HOST, MAX_REQUEST_BYTES, ATTEMPTS = "jev-1.13.0", "api.typesafe.ai", 96000, 4
WRITES, FORBIDDEN_WRITES = {"update", "create", "claim", "release"}, {"create", "claim", "release"}
READS = {("beads", "show"), ("cache", "get")}
SUBPROCESS = re.compile(r"subprocess|BdCli|GitCli|AtmCli|\bgh |\bbd ")
REPLIES = {"refused", "pass_through", "not_applicable", "handled", "from_result"}
MAX_LINES = 100


def table(spec: str, heading: str) -> list[list[str]]:
    section = spec.split(f"## {heading}", 1)[1].split("\n## ", 1)[0] if f"## {heading}" in spec else ""
    rows = [[c.strip() for c in line.strip().strip("|").split("|")] for line in section.splitlines() if line.startswith("|")]
    return rows[2:]  # drop header and separator


def call_name(node: ast.Call) -> tuple[str, str] | None:
    f = node.func
    return (f.value.id, f.attr) if isinstance(f, ast.Attribute) and isinstance(f.value, ast.Name) else None


def facts(src: str) -> dict:
    tree = ast.parse(src)
    parents = {child: node for node in ast.walk(tree) for child in ast.iter_child_nodes(node)}
    arms = [(ast.unparse(c.pattern), c.pattern.lineno) for m in ast.walk(tree) if isinstance(m, ast.Match)
            for c in m.cases]
    transition_arms = [(p, line) for p, line in arms if "Transition." in p]
    calls = [(n, call_name(n)) for n in ast.walk(tree) if isinstance(n, ast.Call)]
    beads_calls = [(name[1], n.lineno) for n, name in calls if name and name[0] == "beads"]
    replies = [(ast.unparse(n), n.lineno) for n, name in calls if name and name[0] == "HookReply" and name[1] in REPLIES]

    def only_read(node: ast.AST) -> bool:
        while (node := parents.get(node)) is not None:
            if isinstance(node, ast.Call) and call_name(node) in READS:
                return True
        return False

    stack_uses = [n.lineno for n in ast.walk(tree)
                  if (getattr(n, "id", None) == "stack_bead" or getattr(n, "attr", None) == "stack_bead") and not only_read(n)]
    return {"arms": arms, "transition_arms": transition_arms, "beads_calls": beads_calls, "replies": replies,
            "stack_bead_writes": stack_uses}


def deterministic(src: str, f: dict, rows: list[list[str]]) -> list[str]:
    names = sorted(r[1] for r in rows)
    armed = sorted(p.removeprefix("Transition.") for p, _ in f["transition_arms"])
    writes = [c for c in f["beads_calls"] if c[0] in WRITES]
    fails = []
    if names != armed:
        fails.append(f"ANC-052 rows != arms: spec rows {names}, Transition arms {armed}")
    if len(writes) > 1 or any(c[0] in FORBIDDEN_WRITES for c in writes):
        fails.append(f"ANC-064 writes: {writes} (at most one beads.update; no create/claim/release)")
    if f["stack_bead_writes"]:
        fails.append(f"ANC-065 stack_bead used outside beads.show(/cache.get( at lines {f['stack_bead_writes']}")
    if hits := [f"{i}: {line.strip()}" for i, line in enumerate(src.splitlines(), 1) if SUBPROCESS.search(line)]:
        fails.append(f"ANC-066 subprocess/bd/gh: {hits}")
    if (n := len(src.splitlines())) > MAX_LINES:
        fails.append(f"ANC-070 {n} lines > {MAX_LINES}")
    return fails


def questions(f: dict, rows: list[list[str]]) -> dict:
    yes_no = lambda q: {"type": "choice", "instructions": q, "criteria": {"yes": "yes", "no": "no"}}
    qs = {f"row:{r[0]}": yes_no(f"Spec Transitions row {r[0]} ({r[1]}). Is this row implemented by exactly one "
                                "match arm of state.hook, with no check or write beyond the row's 'hook checks' and "
                                "'write' cells?") for r in rows}
    for text, line in f["arms"] + f["replies"]:
        qs[f"site:L{line}:{text[:60]}"] = yes_no(f"state.hook line {line}: `{text}`. Is this arm or reply listed in "
                                                  "state.spec as a Transitions row or an Errors entry?")
    return qs


def ask_jev(state: dict, qs: dict) -> dict:
    key = os.environ.get("TYPESAFE_API_KEY", "").strip()
    if not key:
        raise RuntimeError("TYPESAFE_API_KEY is missing; no Jev evaluation ran")
    body = json.dumps({"model": MODEL, "state": state, "questions": qs}).encode()
    if len(body) > MAX_REQUEST_BYTES:
        raise RuntimeError(f"Jev request is {len(body)} bytes > {MAX_REQUEST_BYTES}")
    for attempt in range(ATTEMPTS):
        conn = http.client.HTTPSConnection(HOST, timeout=60)
        conn.request("POST", "/v1/systemone", body=body,
                     headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"})
        resp = conn.getresponse()
        raw, status = resp.read(), resp.status
        conn.close()
        if status in (429, 502, 529) and attempt + 1 < ATTEMPTS:
            time.sleep(min(30.0, float(resp.getheader("Retry-After") or 1)))
            continue
        if status != 200:
            raise RuntimeError(f"Jev HTTP {status}")
        answers = json.loads(raw).get("answers", {})
        if set(answers) != set(qs):
            raise RuntimeError("Jev answers do not match the questions")
        return answers
    raise RuntimeError("Jev busy; attempts exhausted")


def main(argv: list[str]) -> int:
    if len(argv) not in (2, 4) or (len(argv) == 4 and argv[2] != "--out"):
        print(__doc__, file=sys.stderr)
        return 2
    hook, spec = Path(argv[0]), Path(argv[1])
    out = Path(argv[3]) if len(argv) == 4 else spec.parent.parent / "anchor-runs" / "hooks" / f"{hook.stem}.json"
    src, spec_md = hook.read_text(), spec.read_text()
    rows = table(spec_md, "Transitions")
    f = facts(src)
    run = {"hook": str(hook), "spec": str(spec), "lines": len(src.splitlines()), "rows": [r[:2] for r in rows],
           "facts": f, "deterministic_failures": deterministic(src, f, rows), "jev": None}
    if not run["deterministic_failures"]:
        try:
            answers = ask_jev({"hook": src, "spec": spec_md}, questions(f, rows))
        except (RuntimeError, OSError, ValueError) as exc:
            run["jev_error"] = str(exc)
        else:
            run["jev"] = {"model": MODEL, "answers": answers,
                          "no": sorted(k for k, a in answers.items() if a.get("choice") != "yes")}
    run["pass"] = not run["deterministic_failures"] and run["jev"] is not None and not run["jev"]["no"]
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(run, indent=2) + "\n")
    print(json.dumps({"pass": run["pass"], "out": str(out), "deterministic_failures": run["deterministic_failures"],
                      "jev_no": run["jev"] and run["jev"]["no"], "jev_error": run.get("jev_error")}, indent=2))
    return 0 if run["pass"] else (2 if "jev_error" in run else 1)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
