#!/usr/bin/env python3
"""One-call status table for a `gh stack`.

Combines exactly three data sources into one table:

1. ``gh stack view --json``  - local stack tracking: layer order, head/base
   SHAs, needsRebase, PR number.
2. ``git rev-parse origin/<branch>`` (after one ``git fetch``) - what is
   actually pushed.
3. One batched GraphQL query - per-PR ``mergeable``, ``mergeStateStatus``,
   ``baseRefName``, ``headRefOid``, ``isDraft`` and CI rollup.

Coherence checks (the two metrics conventional per-branch calls never show):

* ``base ok``  - each layer's base == the layer below's head (bottom == trunk).
* ``origin ok`` - local head == origin head == PR head.
* ``needsRebase`` straight from gh stack, plus GitHub's ``mergeable`` /
  ``mergeStateStatus`` which reveal CONFLICTING / BEHIND / DIRTY layers.

Read-only. Never runs ``gh stack sync`` or ``gh stack rebase``.
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor


def run(cmd: list[str], *, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, capture_output=True, check=check)


def short(sha: str | None) -> str:
    return (sha or "")[:9] or "-"


def stack_json_at(path: str) -> dict | None:
    proc = subprocess.run(["gh", "stack", "view", "--json"], cwd=path, text=True, capture_output=True)
    if proc.returncode != 0:
        return None
    try:
        data = json.loads(proc.stdout)
    except json.JSONDecodeError:
        return None
    return data if data.get("branches") else None


def worktree_paths() -> list[str]:
    """Every worktree of this repo that is checked out on a branch (cwd first)."""
    out = run(["git", "worktree", "list", "--porcelain"]).stdout
    paths: list[str] = []
    cur: str | None = None
    for line in out.splitlines():
        if line.startswith("worktree "):
            cur = line[len("worktree "):]
        elif line.startswith("branch ") and cur:
            paths.append(cur)
            cur = None
        elif line == "" :
            cur = None
    cwd = run(["git", "rev-parse", "--show-toplevel"]).stdout.strip()
    paths.sort(key=lambda p: p != cwd)
    return paths


def discover_stacks(trunk_filter: str | None, *, include_merged: bool) -> list[dict]:
    """Run `gh stack view --json` once per worktree, dedupe by branch set.

    Works from develop/main (not part of any stack): every stack that has at
    least one worktree checked out is found. Concurrent, read-only.
    """
    paths = worktree_paths()
    with ThreadPoolExecutor(max_workers=8) as pool:
        results = list(pool.map(stack_json_at, paths))
    stacks: list[dict] = []
    for path, data in zip(paths, results):
        if not data:
            continue
        if trunk_filter and data["trunk"] != trunk_filter:
            continue
        if not include_merged and all(b.get("isMerged") for b in data["branches"]):
            continue
        data["worktree"] = path
        stacks.append(data)
    # Each worktree only knows the layers linked from it; the same stack seen
    # from a lower layer is a prefix of the view from the top. Keep the longest.
    def names(d: dict) -> tuple[str, ...]:
        return tuple(b["name"] for b in d["branches"])
    stacks = [d for d in stacks if not any(
        o is not d and len(names(o)) > len(names(d)) and names(o)[: len(names(d))] == names(d)
        for o in stacks)]
    uniq: dict[tuple[str, ...], dict] = {}
    for d in stacks:
        uniq.setdefault(names(d), d)
    stacks = list(uniq.values())
    stacks.sort(key=lambda d: (not d["trunk"].startswith("integrate/"), d["trunk"], d["branches"][0]["name"]))
    return stacks


def origin_sha(ref: str) -> str | None:
    proc = run(["git", "rev-parse", "--verify", "--quiet", f"origin/{ref}"], check=False)
    return proc.stdout.strip() or None


def pr_details(numbers: list[int]) -> dict[int, dict]:
    """One GraphQL round-trip for every PR in the stack."""
    if not numbers:
        return {}
    remote = run(["gh", "repo", "view", "--json", "owner,name"]).stdout
    repo = json.loads(remote)
    owner, name = repo["owner"]["login"], repo["name"]
    fields = (
        "number isDraft mergeable mergeStateStatus baseRefName headRefOid "
        "reviewDecision state "
        "commits(last:1){nodes{commit{statusCheckRollup{state}}}}"
    )
    aliases = " ".join(f"pr{n}: pullRequest(number:{n}) {{ {fields} }}" for n in numbers)
    query = f'query($owner:String!,$name:String!){{ repository(owner:$owner,name:$name) {{ {aliases} }} }}'
    proc = run(["gh", "api", "graphql", "-f", f"query={query}", "-F", f"owner={owner}", "-F", f"name={name}"])
    data = json.loads(proc.stdout)["data"]["repository"]
    out: dict[int, dict] = {}
    for n in numbers:
        pr = data.get(f"pr{n}") or {}
        nodes = ((pr.get("commits") or {}).get("nodes") or [{}])
        rollup = ((nodes[0].get("commit") or {}).get("statusCheckRollup") or {}).get("state")
        pr["ci"] = rollup or "NONE"
        out[n] = pr
    return out


def is_ancestor(older: str, newer: str) -> bool:
    return run(["git", "merge-base", "--is-ancestor", older, newer], check=False).returncode == 0


def build_rows(stack: dict, prs: dict[int, dict], *, fetched: bool) -> tuple[list[dict], list[str], list[str]]:
    trunk = stack["trunk"]
    trunk_origin = origin_sha(trunk) if fetched else None
    rows: list[dict] = []
    problems: list[str] = []
    notes: list[str] = []
    expected_base = trunk_origin
    for idx, br in enumerate(stack["branches"], start=1):
        name = br["name"]
        pr = prs.get((br.get("pr") or {}).get("number") or -1, {})
        if br.get("isMerged"):
            rows.append({"layer": idx, "branch": name, "pr": (br.get("pr") or {}).get("number"),
                         "head": br.get("head"), "base": br.get("base"), "merged": True, "queued": False,
                         "draft": False, "mergeable": None, "merge_state": "MERGED", "ci": pr.get("ci"),
                         "base_ok": None, "origin_ok": None, "needs_rebase": False, "origin": None,
                         "pr_head": pr.get("headRefOid"), "expected_base": expected_base, "pr_base": pr.get("baseRefName")})
            expected_base = br.get("head") or expected_base
            continue
        origin = origin_sha(name) if fetched else None
        base_ok = (br.get("base") == expected_base) if (expected_base and br.get("base")) else None
        origin_ok = None
        if origin:
            origin_ok = br.get("head") == origin and (not pr or pr.get("headRefOid") == origin)
        row = {
            "layer": idx,
            "branch": name,
            "pr": (br.get("pr") or {}).get("number"),
            "head": br.get("head"),
            "origin": origin,
            "pr_head": pr.get("headRefOid"),
            "base": br.get("base"),
            "expected_base": expected_base,
            "base_ok": base_ok,
            "origin_ok": origin_ok,
            "needs_rebase": br.get("needsRebase"),
            "merged": br.get("isMerged"),
            "queued": br.get("isQueued"),
            "draft": pr.get("isDraft"),
            "mergeable": pr.get("mergeable"),
            "merge_state": pr.get("mergeStateStatus"),
            "ci": pr.get("ci"),
            "pr_base": pr.get("baseRefName"),
        }
        parent = trunk if idx == 1 else stack["branches"][idx - 2]["name"]
        if pr and pr.get("baseRefName") not in (None, parent):
            problems.append(f"L{idx} {name}: PR #{row['pr']} base is {pr['baseRefName']}, expected {parent}")
        if base_ok is False:
            if idx == 1 and is_ancestor(br.get("base", ""), expected_base):
                notes.append(f"L1 {name}: behind trunk ({short(br['base'])} < {short(expected_base)}); fine unless CONFLICTING, do not restart CI just to catch up")
            else:
                problems.append(f"L{idx} {name}: base {short(br['base'])} != parent head {short(expected_base)} -> needs rebase")
        if origin_ok is False:
            problems.append(
                f"L{idx} {name}: local {short(br['head'])} / origin {short(origin)} / PR {short(pr.get('headRefOid'))} differ"
                " -> local tracking stale or unpushed; owner must fetch+reset or push"
            )
        if br.get("needsRebase"):
            problems.append(f"L{idx} {name}: gh stack reports needsRebase")
        if pr.get("mergeable") == "CONFLICTING":
            problems.append(f"L{idx} {name}: PR #{row['pr']} CONFLICTING")
        if pr.get("isDraft"):
            problems.append(f"L{idx} {name}: PR #{row['pr']} is DRAFT (blocks stack merge)")
        rows.append(row)
        # The next layer must be based on THIS layer's pushed head (fall back to local).
        expected_base = origin or br.get("head")
    return rows, problems, notes


ICON_SYNC = {"ok": "✅", "stale": "🔄", "rebase": "⚠️", "unknown": "❓"}
ICON_MERGE = {"MERGED": "\U0001f3c1", "OK": "✅", "BLOCKED": "\U0001f6a7"}
ICON_CI = {"SUCCESS": "✅", "FAILURE": "⛔", "ERROR": "⛔", "PENDING": "\U0001f300",
           "EXPECTED": "\U0001f300", "NONE": "—"}


def sync_icon(r: dict) -> str:
    if r["merged"]:
        return ICON_MERGE["MERGED"]
    if r["origin_ok"] is False:
        return ICON_SYNC["stale"]
    if r["base_ok"] is False or r["needs_rebase"]:
        return ICON_SYNC["rebase"]
    if r["base_ok"] is None or r["origin_ok"] is None:
        return ICON_SYNC["unknown"]
    return ICON_SYNC["ok"]


def merge_icon(r: dict) -> str:
    """✅ only when GitHub says the PR can merge now; anything else is 🚧."""
    if r["merged"]:
        return ICON_MERGE["MERGED"]
    if r["draft"] or r["queued"] or r["mergeable"] != "MERGEABLE":
        return ICON_MERGE["BLOCKED"]
    return ICON_MERGE["OK"] if r["merge_state"] in ("CLEAN", "HAS_HOOKS", "UNSTABLE") else ICON_MERGE["BLOCKED"]


def ci_icon(r: dict) -> str:
    return ICON_CI.get(r["ci"] or "NONE", ICON_CI["NONE"])


def render_table(stack: dict, rows: list[dict], problems: list[str], notes: list[str], *, trunk_origin: str | None) -> str:
    hdr = ["L", "PR", "rebase", "merge", "CI"]
    lines = [f"stack: {stack['branches'][-1]['name']} -> {stack['trunk']} @ {short(trunk_origin)}", ""]
    lines.append("| " + " | ".join(hdr) + " |")
    lines.append("|" + "|".join("---" for _ in hdr) + "|")
    for r in rows:
        pr = f"#{r['pr']}" if r["pr"] else "-"
        lines.append("| " + " | ".join([
            f"{r['layer']}/{len(rows)}", pr, sync_icon(r), merge_icon(r), ci_icon(r),
        ]) + " |")
    lines.append("")
    if problems:
        lines.append(f"VERDICT: ❌ NOT COHERENT ({len(problems)} issue(s))")
        lines.extend(f"- {p}" for p in problems)
    else:
        lines.append("VERDICT: ✅ COHERENT - every base == parent head, every head pushed and on its PR")
    lines.extend(f"- note: {n}" for n in notes)
    return "\n".join(lines)


def legend() -> str:
    lines = []
    lines.append("rebase: ✅ not needed (base==parent head, local==origin==PR)  ⚠️ needed  🔄 local tracking stale: fetch+reset before any sync  ❓ unknown (--no-fetch)  🏁 merged")
    lines.append("merge: ✅ mergeable now  🚧 blocked (conflicting, behind, draft, queued, required checks, or still computing)  🏁 merged")
    lines.append("CI: ✅ green  🌀 running  ⛔ failed, do not enter  — none")
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--trunk", help="only stacks whose trunk is this branch (e.g. integrate/phase-aw)")
    ap.add_argument("--phase", help="shorthand for --trunk integrate/phase-<PHASE>")
    ap.add_argument("--all", action="store_true", help="include stacks whose every layer is already merged")
    ap.add_argument("--no-fetch", action="store_true", help="skip `git fetch origin` and origin comparison")
    ap.add_argument("--no-pr", action="store_true", help="skip the GraphQL PR query (offline / local-only view)")
    ap.add_argument("--json", action="store_true", help="emit the merged rows as JSON instead of tables")
    args = ap.parse_args()
    trunk_filter = args.trunk or (f"integrate/phase-{args.phase.lower()}" if args.phase else None)

    stacks = discover_stacks(trunk_filter, include_merged=args.all)
    if not stacks:
        where = f" with trunk {trunk_filter}" if trunk_filter else ""
        sys.stderr.write(
            f"no gh stack found{where}. Stacks are discovered through `git worktree list`; a stack "
            "needs at least one of its layers checked out in a worktree (never `git checkout` in the main repo).\n"
        )
        return 2
    fetched = not args.no_fetch
    if fetched:
        run(["git", "fetch", "--quiet", "origin"], check=False)
    numbers = sorted({b["pr"]["number"] for st in stacks for b in st["branches"] if b.get("pr")})
    prs = {} if args.no_pr else pr_details(numbers)

    report: list[dict] = []
    blocks: list[str] = []
    any_problem = False
    for st in stacks:
        rows, problems, notes = build_rows(st, prs, fetched=fetched)
        trunk_origin = origin_sha(st["trunk"]) if fetched else None
        any_problem |= bool(problems)
        report.append({"trunk": st["trunk"], "trunk_origin": trunk_origin, "worktree": st["worktree"],
                       "rows": rows, "problems": problems, "notes": notes, "coherent": not problems})
        blocks.append(render_table(st, rows, problems, notes, trunk_origin=trunk_origin))
    if args.json:
        print(json.dumps({"stacks": report, "coherent": not any_problem}, indent=2))
    else:
        print("\n\n".join(blocks))
        print()
        print(legend())
    return 1 if any_problem else 0


if __name__ == "__main__":
    sys.exit(main())
