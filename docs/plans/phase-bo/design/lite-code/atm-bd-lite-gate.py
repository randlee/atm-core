#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["jev-check"]
# [tool.uv.sources]
# jev-check = { path = "../packages/jev-check", editable = true }  # bo-12: skills/atm-bd-lite/scripts -> atm-bd packages/jev-check
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
import ast, json, sys
from pathlib import Path
from jev_check import JevCheckError, Question, check, load_question_set

QUESTIONS = Path(__file__).with_name("gate-questions.json")
WRITES, FORBIDDEN_WRITES = {"update", "create", "claim", "release"}, {"create", "claim", "release"}
READS = {("beads", "show"), ("cache", "get")}
CLI_ADAPTERS, OS_SPAWN, CLI_TOOLS = {"BdCli", "GitCli", "AtmCli"}, ("system", "popen", "exec", "spawn"), {"gh", "bd", "git"}
REPLIES = {"pass_through", "not_applicable", "handled"}  # plus refused(Refusal(ErrorId.X, ...)) (§B.1)
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
    def typed_refusal(n: ast.Call) -> bool:
        arg = n.args[0] if n.args else None
        return (isinstance(arg, ast.Call) and getattr(arg.func, "id", None) == "Refusal" and bool(arg.args)
                and ast.unparse(arg.args[0]).startswith("ErrorId."))

    replies = [(ast.unparse(n), n.lineno) for n, name in calls if name and name[0] == "HookReply"
               and (name[1] in REPLIES or (name[1] == "refused" and typed_refusal(n)))]

    def only_read(node: ast.AST) -> bool:
        while (node := parents.get(node)) is not None:
            if isinstance(node, ast.Call) and call_name(node) in READS:
                return True
        return False

    def runs_cli(n: ast.Call) -> bool:
        first = n.args[0] if n.args else None
        if isinstance(first, (ast.List, ast.Tuple)) and first.elts:
            first = first.elts[0]
        return isinstance(first, ast.Constant) and isinstance(first.value, str) and first.value.split(" ")[0] in CLI_TOOLS

    subprocess_sites = sorted(
        [(f"import {a.name}", n.lineno) for n in ast.walk(tree) if isinstance(n, ast.Import) for a in n.names
         if a.name.split(".")[0] == "subprocess"]
        + [(f"from {n.module} import", n.lineno) for n in ast.walk(tree)
           if isinstance(n, ast.ImportFrom) and (n.module or "").split(".")[0] == "subprocess"]
        + [(getattr(n, "id", None) or n.attr, n.lineno) for n in ast.walk(tree)
           if getattr(n, "id", None) in CLI_ADAPTERS or getattr(n, "attr", None) in CLI_ADAPTERS]
        + [(f"import {a.name}", n.lineno) for n in ast.walk(tree) if isinstance(n, ast.ImportFrom)
           for a in n.names if a.name in CLI_ADAPTERS]
        + [(ast.unparse(n)[:60], n.lineno) for n, name in calls
           if (name and name[0] == "os" and name[1].startswith(OS_SPAWN)) or runs_cli(n)], key=lambda x: x[1])
    stack_uses = [n.lineno for n in ast.walk(tree)
                  if (getattr(n, "id", None) == "stack_bead" or getattr(n, "attr", None) == "stack_bead") and not only_read(n)]
    return {"arms": arms, "transition_arms": transition_arms, "beads_calls": beads_calls, "replies": replies,
            "stack_bead_writes": stack_uses,
            "subprocess_sites": subprocess_sites}


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
    if f["subprocess_sites"]:
        fails.append(f"ANC-066 subprocess/CLI adapter/bd/gh/git: {f['subprocess_sites']}")
    if (n := len(src.splitlines())) > MAX_LINES:
        fails.append(f"ANC-070 {n} lines > {MAX_LINES}")
    return fails


def questions(f: dict, rows: list[list[str]]) -> dict[str, Question]:
    qs = load_question_set(QUESTIONS)
    return (qs.expand("implemented", {r[0]: f"Spec Transitions row {r[0]} ({r[1]})." for r in rows})
            | qs.expand("listed", {f"L{line}": f"state.hook line {line}: `{text}`." for text, line in f["arms"] + f["replies"]}))


def findings(answers: dict, f: dict) -> list[str]:
    """jev-check result -> gate findings (proposal deliverable 6)."""
    sites = {f"L{line}": text for text, line in f["arms"] + f["replies"]}
    out = []
    for key, a in answers.items():
        kind, subject = key.split(":", 1)
        if a["choice"] != "yes":
            out.append(f"implement {subject}" if kind == "implemented"
                       else f"add a row for {subject} `{sites.get(subject, '')}`, or remove the branch")
    return out


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
            answers = check({"hook": src, "spec": spec_md}, questions(f, rows))
        except JevCheckError as exc:
            run["jev_error"] = str(exc)
        else:
            run["jev"] = {"answers": answers, "findings": findings(answers, f),
                          "no": sorted(k for k, a in answers.items() if a.get("choice") != "yes")}
    run["pass"] = not run["deterministic_failures"] and run["jev"] is not None and not run["jev"]["no"]
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(run, indent=2) + "\n")
    print(json.dumps({"pass": run["pass"], "out": str(out), "deterministic_failures": run["deterministic_failures"],
                      "jev_no": run["jev"] and run["jev"]["no"], "jev_error": run.get("jev_error")}, indent=2))
    return 0 if run["pass"] else (2 if "jev_error" in run else 1)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
