#!/usr/bin/env python3
"""Phase contract check: the authored sprints.json against the live (or to-be-imported) beads.

Run by validate-plan after check-plan.jq. Prints one problem per line as `<bead-id>: <message>`
(warnings as `warning: <message>`) and exits 0 (valid), 5 (problems) or 2 (could not run).

  phase_contract_check.py --root <id> [--index <sprints.json>] [--scope <bead-id>]   live beads
  phase_contract_check.py --plan <plan.jsonl> [--index <sprints.json>]                 import shape, before beads exist
  phase_contract_check.py --fixture <fixture.json>                                      committed test fixture

Checks (bead atm-bo-10 deliverables 1-4):
  membership   the children of the phase root are exactly the pairs listed in sprints.json
               (plus plan-gate beads stage:plan*, gate beads, QA beads that `validates` a phase bead and findings
               `caused-by` one: bd keeps one edge type per pair, so the edge is the membership); each pair is linked
  schema       every listed dev bead is a stage:sprint with N >= 1 numbered deliverables, acceptance
               criteria covering exactly 1..N, non-empty owned_paths, a valid difficulty; its sanity bead too
  graph        the pr_target sprint's sanity bead is in the dev bead's blocker closure; relation matches the
               edges; owned paths are disjoint across sprints; handoffs sit inside both fences
  state        (live only) no PASS sanity reopened; blockers were closed before a dev bead started; a PASS
               has a QA bead; at most one fix round (second FAIL = ROUND_CAP); open PR base == pr_target;
               finding priority follows severity; sprint beads are P2; difficulty present where required;
               dev-sanity base/commit types; human gates listed in policy.human_gates; a blocking finding's
               sanity bead gates not-started downstream work (R16). policy.waivers turns an accepted past
               state violation into a warning
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Callable, Iterable

sys.path.insert(0, str(Path(__file__).resolve().parent))
import plan_contract as C  # noqa: E402
from sprint_index_common import index_bead_pairs, validate_index  # noqa: E402

PLAN_GATE_LABELS = ("stage:plan", "stage:plan-review", "stage:plan-fix")
QA_LABEL = "stage:qa"
QA_DISPATCH_WINDOW = timedelta(minutes=30)
SHA40 = re.compile(r"^[0-9a-f]{40}$")
DELIVERABLE_LINE = re.compile(r"^\s*(\d+)[.)]\s+\S")
# acceptance keys: at the start of a checkbox line ("- [ ] #2–3:", "- [ ] Deliverables 1–3:", "- [ ] D4:")
# or in parentheses anywhere on the line ("(D1–D3)", "(#1/#3)", "(#2)"). A bare "#88" in prose is an issue number.
AC_LINE_KEY = re.compile(r"^\s*(?:-\s*\[.\]\s*)?(?:#|Deliverables?\s*|D)(\d+(?:\s*(?:[-–—/,]|and)\s*(?:#|D)?\d+)*)\s*[:.)]", re.IGNORECASE)
AC_PAREN_KEY = re.compile(r"\(`?(?:[a-z][a-z0-9]*(?:-[a-z0-9]+)*)?(?:#|D)(\d+(?:\s*(?:[-–—/,]|and)\s*(?:#|D)?\d+)*)`?\)")
FOLDED = re.compile(r"^\s*folded into\s+\S+", re.IGNORECASE)
Problems = list[str]


# ---------------------------------------------------------------------------------------------- helpers

def meta(b: dict[str, Any]) -> dict[str, Any]:
    m = b.get("metadata")
    return m if isinstance(m, dict) else {}


def labels(b: dict[str, Any]) -> list[str]:
    return [str(x) for x in (b.get("labels") or [])]


def has_label(b: dict[str, Any], label: str) -> bool:
    return label in labels(b)


def stage(b: dict[str, Any]) -> str:
    return next((l for l in labels(b) if l.startswith("stage:")), "")


def severity(b: dict[str, Any]) -> str:
    for l in labels(b):
        if l.startswith("severity:"):
            return l.split(":", 1)[1]
    return str(meta(b).get("severity") or "")


def deps(b: dict[str, Any], kind: str) -> list[str]:
    out = []
    for d in b.get("dependencies") or []:
        t = d.get("type") or d.get("dependency_type") or ""
        if t == kind:
            out.append(str(d.get("depends_on_id") or d.get("id") or ""))
    return out


def parent_of(b: dict[str, Any]) -> str:
    p = b.get("parent")
    if isinstance(p, str) and p:
        return p
    pc = deps(b, "parent-child")
    return pc[0] if pc else ""


def ts(value: Any) -> datetime | None:
    if not isinstance(value, str) or not value:
        return None
    v = value.replace("Z", "+00:00")
    try:
        d = datetime.fromisoformat(v)
    except ValueError:
        try:
            d = datetime.strptime(value[:19], "%Y-%m-%d %H:%M:%S")
        except ValueError:
            return None
    return d if d.tzinfo else d.replace(tzinfo=timezone.utc)


def is_pass(b: dict[str, Any]) -> bool:
    return b.get("status") == "closed" and str(b.get("close_reason") or "").upper().startswith("PASS")


def is_fail_reason(reason: Any) -> bool:
    return str(reason or "").upper().startswith("FAIL")


def glob_prefix(path: str) -> tuple[str, bool]:
    """('crates/x/src', True) for 'crates/x/src/**'; ('a/b.rs', False) for a plain path."""
    p = path.strip().rstrip("/")
    for suffix in ("/**", "/*"):
        if p.endswith(suffix):
            return p[: -len(suffix)], True
    if "*" in p:
        return p.split("*", 1)[0].rstrip("/"), True
    return p, False


def paths_overlap(a: str, b: str) -> bool:
    pa, ga = glob_prefix(a)
    pb, gb = glob_prefix(b)
    if pa == pb:
        return True
    if ga and (pb.startswith(pa + "/") or pa == ""):
        return True
    if gb and (pa.startswith(pb + "/") or pb == ""):
        return True
    return False


def path_inside(path: str, fence: Iterable[str]) -> bool:
    for f in fence:
        pf, g = glob_prefix(f)
        if path == pf or (g and path.startswith(pf + "/")):
            return True
    return False


def parse_deliverables(description: str) -> list[int]:
    """Numbers of the items in the '## Deliverables' numbered list, in order."""
    nums: list[int] = []
    in_section = False
    for line in (description or "").splitlines():
        if line.startswith("## "):
            in_section = line.strip().lower().startswith("## deliverables")
            continue
        if in_section:
            m = DELIVERABLE_LINE.match(line)
            if m:
                nums.append(int(m.group(1)))
    return nums


def _expand_keys(spec: str) -> set[int]:
    """'1–3' -> {1,2,3}; '1/3' -> {1,3}; '2, 4 and 5' -> {2,4,5}."""
    out: set[int] = set()
    for part in re.split(r"\s*(?:[/,]|and)\s*", spec):
        nums = [int(x) for x in re.findall(r"\d+", part)]
        if not nums:
            continue
        if len(nums) >= 2 and re.search(r"[-–—]", part):
            lo, hi = min(nums[0], nums[1]), max(nums[0], nums[1])
            out.update(range(lo, hi + 1))
        else:
            out.update(nums)
    return out


def acceptance_refs(text: str) -> set[int]:
    refs: set[int] = set()
    for line in (text or "").splitlines():
        m = AC_LINE_KEY.match(line)
        if m:
            refs |= _expand_keys(m.group(1))
        for m in AC_PAREN_KEY.finditer(line):
            refs |= _expand_keys(m.group(1))
    return refs


def is_folded(b: dict[str, Any]) -> bool:
    return b.get("status") == "closed" and bool(FOLDED.match(str(b.get("close_reason") or "")))


def history_transitions(entries: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Ordered snapshots {at, status, close_reason} from `bd history --json`."""
    out = []
    for e in entries:
        iss = e.get("Issue", e.get("issue"))
        if isinstance(iss, str):
            try:
                iss = json.loads(iss)
            except json.JSONDecodeError:
                continue
        if not isinstance(iss, dict):
            continue
        at = ts(e.get("CommitDate") or e.get("commit_date") or iss.get("updated_at"))
        out.append({"at": at, "status": iss.get("status"), "close_reason": iss.get("close_reason") or ""})
    out.sort(key=lambda s: s["at"] or datetime.min.replace(tzinfo=timezone.utc))
    return out


# ---------------------------------------------------------------------------------------------- the check

class PhaseCheck:
    def __init__(self, root_id: str, index: dict[str, Any] | None, beads: list[dict[str, Any]], *,
                 live: bool, history: Callable[[str], list[dict[str, Any]]] | None = None,
                 prs: list[dict[str, Any]] | None = None, now: datetime | None = None) -> None:
        self.root_id = root_id
        self.index = index
        self.live = live
        self.history = history or (lambda _id: [])
        self.prs = prs or []
        self.now = now or datetime.now(timezone.utc)
        self.by: dict[str, dict[str, Any]] = {str(b["id"]): b for b in beads if b.get("id")}
        self.root = self.by.get(root_id, {})
        self.problems: Problems = []
        self.warnings: list[str] = []
        self.pairs: dict[str, str] = index_bead_pairs(index) if index else {}
        pol = (index or {}).get("policy") or {}
        self.waivers: list[dict[str, Any]] = [w for w in (pol.get("waivers") or []) if isinstance(w, dict)] if isinstance(pol, dict) else []
        self.trunk = ""
        if index and isinstance(index.get("integration_branch"), str):
            self.trunk = index["integration_branch"]
        if not self.trunk:
            self.trunk = str(meta(self.root).get("integration_branch") or "")
        # phase set: root and every descendant through parent links
        self.phase_ids: set[str] = {root_id}
        changed = True
        while changed:
            changed = False
            for bid, b in self.by.items():
                if bid not in self.phase_ids and parent_of(b) in self.phase_ids:
                    self.phase_ids.add(bid)
                    changed = True

    # -- reporting
    def problem(self, bead: str, msg: str, check: str = "") -> None:
        """Record a problem; a state check named in policy.waivers for this bead becomes a warning."""
        if check:
            for w in self.waivers:
                if w.get("bead") == bead and w.get("check") == check:
                    self.warn(f"waived {check} on {bead}: {msg} [{w.get('reason')}]")
                    return
        self.problems.append(C.PROBLEM_LINE.format(bead=bead, message=msg))

    def warn(self, msg: str) -> None:
        self.warnings.append(C.WARNING_PREFIX + msg)

    # -- derived
    def devs(self) -> list[str]:
        if self.pairs:
            return list(self.pairs)
        return sorted(b for b, x in self.by.items() if has_label(x, C.SPRINT_LABEL) or
                      (has_label(x, "stage:dev") and parent_of(x) == self.root_id))

    def sanity_of(self, dev: str) -> str:
        if dev in self.pairs:
            return self.pairs[dev]
        for bid, b in self.by.items():
            if has_label(b, C.SANITY_LABEL) and (dev in deps(b, "blocks") or str(meta(b).get("dev_bead") or "") == dev):
                return bid
        return ""

    def branch_to_dev(self) -> dict[str, str]:
        return {str(meta(self.by[d]).get("branch") or ""): d for d in self.devs() if d in self.by}

    def blocker_closure(self, dev: str) -> set[str]:
        """Every bead reachable through blocks edges, following a sanity bead to its dev bead."""
        seen: set[str] = set()
        stack = [dev]
        while stack:
            cur = stack.pop()
            for blk in deps(self.by.get(cur, {}), "blocks"):
                if blk in seen:
                    continue
                seen.add(blk)
                stack.append(blk)
                if has_label(self.by.get(blk, {}), C.SANITY_LABEL):
                    dev_of = str(meta(self.by.get(blk, {})).get("dev_bead") or "")
                    if dev_of and dev_of not in seen:
                        seen.add(dev_of)
                        stack.append(dev_of)
        return seen

    def edge_sprint(self, bid: str) -> str:
        """The phase bead a QA bead `validates` or a finding is `caused-by`, when that bead is in the phase."""
        b = self.by.get(bid, {})
        kinds = ("validates",) if has_label(b, QA_LABEL) else ("caused-by",) if stage(b) == C.FINDING_LABEL else ()
        for kind in kinds:
            for t in deps(b, kind):
                if t in self.phase_ids and t != self.root_id:
                    return t
        return ""

    def sprint_of(self, bid: str) -> str:
        """The listed sprint dev bead a finding/fix/QA bead belongs to (metadata.sprint_bead, edges or ancestry)."""
        devs = set(self.devs())
        sb = str(meta(self.by.get(bid, {})).get("sprint_bead") or "")
        if sb in devs:
            return sb
        es = self.edge_sprint(bid)
        if es in devs:
            return es
        if es and es != bid:
            up = self.sprint_of(es)
            if up:
                return up
        cur, seen = parent_of(self.by.get(bid, {})), set()
        while cur and cur not in seen:
            if cur in devs:
                return cur
            seen.add(cur)
            cur = parent_of(self.by.get(cur, {}))
        return ""

    def downstream_devs(self, sprint: str) -> list[str]:
        san = self.sanity_of(sprint)
        return [d for d in self.devs() if d != sprint and d in self.by and san and san in self.blocker_closure(d)]

    def downstream_targets(self, sprint: str, exclude: set[str] = frozenset()) -> list[str]:
        """Not-started downstream dev beads plus open, unclaimed findings/fixes on those sprints, minus `exclude`
        (the gate's own blocker closure: a gate cannot block what it waits on)."""
        out = []
        downstream = [d for d in self.downstream_devs(sprint) if d not in exclude]
        for d in downstream:
            if self.by[d].get("status") == "open":
                out.append(d)
        for bid, b in self.by.items():
            if bid in self.phase_ids and stage(b) in (C.FINDING_LABEL, C.FIX_LABEL) and b.get("status") == "open" \
                    and not b.get("assignee") and bid not in exclude and self.sprint_of(bid) in downstream:
                out.append(bid)
        return out

    # -- checks
    def check_membership(self) -> None:
        if not self.index:
            self.warn("no sprints.json given; membership not checked")
            return
        if str(self.index.get("root_bead_id")) != self.root_id:
            self.problem(self.root_id, f"sprints.json root_bead_id {self.index.get('root_bead_id')!r} is not {self.root_id}")
        listed: set[str] = set(self.pairs) | set(self.pairs.values())
        for dev, san in self.pairs.items():
            for bid in (dev, san):
                if bid not in self.by:
                    self.problem(bid, "listed in sprints.json but not in beads")
                elif parent_of(self.by[bid]) != self.root_id:
                    self.problem(bid, f"listed in sprints.json but its parent is {parent_of(self.by[bid]) or 'none'}, not {self.root_id}")
            if dev in self.by and san in self.by:
                if not has_label(self.by[san], C.SANITY_LABEL):
                    self.problem(san, f"paired sanity bead of {dev} is not labelled {C.SANITY_LABEL}")
                if dev not in deps(self.by[san], "blocks"):
                    self.problem(san, f"paired sanity bead does not block on its dev bead {dev}")
                if str(meta(self.by[san]).get("dev_bead") or "") != dev:
                    self.problem(san, f"metadata.dev_bead is not {dev}")
        for bid, b in self.by.items():
            if parent_of(b) != self.root_id or bid in listed:
                continue
            if any(has_label(b, l) for l in PLAN_GATE_LABELS):
                continue
            if b.get("issue_type") == "gate":
                continue
            if is_folded(b) or (has_label(b, C.SANITY_LABEL) and b.get("status") == "closed"
                                and is_folded(self.by.get(str(meta(b).get("dev_bead") or ""), {}))):
                continue  # absorbed sprint and its sanity bead: history, not membership
            st = stage(b) or "unlabelled"
            hint = "parent it under its sprint dev bead"
            es = self.edge_sprint(bid)
            if es:  # bd keeps one edge type per pair: the validates/caused-by edge must go before the reparent
                kind = "validates" if has_label(b, QA_LABEL) else "caused-by"
                hint = f"bd dep remove {bid} {es} ({kind}), then bd update {bid} --parent {es}"
            self.problem(bid, f"{st} bead is a child of {self.root_id} but not a listed sprint pair; {hint}")

    def check_hierarchy(self) -> None:
        """Top level holds epics only (user ruling 2026-09-26: several phases share one repo, so beads at the
        top level get mixed up). The phase root is an epic, or a feature whose ancestors are all epics."""
        cur, seen = self.root_id, set()
        while cur and cur not in seen:
            seen.add(cur)
            b = self.by.get(cur)
            if not b:
                self.problem(cur, "phase ancestor is not in beads")
                return
            kind = str(b.get("issue_type") or "")
            parent = parent_of(b)
            if cur == self.root_id and kind not in ("epic", "feature"):
                self.problem(cur, f"phase root is a {kind or 'bead without issue_type'}; a phase root is an epic or a feature under an epic")
            elif cur != self.root_id and kind != "epic":
                self.problem(cur, f"phase ancestor is a {kind or 'bead without issue_type'}, not an epic; only epics may hold phases")
            if not parent and kind != "epic":
                self.problem(cur, f"{kind or 'bead'} at the top level; only epics live at the top level, parent it under its epic")
            cur = parent
        if self.live:
            stray = sorted(bid for bid, b in self.by.items() if not parent_of(b) and str(b.get("issue_type") or "") != "epic")
            if stray:
                self.warn(f"{len(stray)} top-level beads are not epics (only epics live at the top level): {' '.join(stray)}")

    def check_schema(self) -> None:
        for dev in self.devs():
            b = self.by.get(dev)
            if not b:
                continue
            m = meta(b)
            if not has_label(b, C.SPRINT_LABEL):
                self.problem(dev, f"planned sprint bead lacks label {C.SPRINT_LABEL}")
            nums = parse_deliverables(str(b.get("description") or ""))
            if not nums:
                self.problem(dev, "no '## Deliverables' numbered list (deliverables are an array of numbered items)")
            else:
                n = len(nums)
                if nums != list(range(1, n + 1)):
                    self.problem(dev, f"deliverables are numbered {nums}, not 1..{n}")
                refs = acceptance_refs(str(b.get("acceptance_criteria") or ""))
                if not refs:
                    self.problem(dev, "acceptance criteria reference no deliverable (key each line with #n or Dn)")
                else:
                    missing = sorted(set(range(1, n + 1)) - refs)
                    extra = sorted(r for r in refs if r > n or r < 1)
                    if missing:
                        self.problem(dev, f"acceptance criteria cover no deliverable {', '.join(f'#{x}' for x in missing)}")
                    if extra:
                        self.problem(dev, f"acceptance criteria reference deliverable {', '.join(f'#{x}' for x in extra)} but there are {n}")
            owned = m.get("owned_paths")
            if not isinstance(owned, list) or not owned or not all(isinstance(p, str) and p for p in owned):
                self.problem(dev, "metadata.owned_paths must be a non-empty list of paths")
            self.check_difficulty(dev, b, required=True)
            san = self.sanity_of(dev)
            if san and san in self.by:
                self.check_difficulty(san, self.by[san], required=True)
                if not self.pairs and not has_label(self.by[san], C.SANITY_LABEL):
                    self.problem(san, f"sanity bead is not labelled {C.SANITY_LABEL}")
            elif not san:
                self.problem(dev, "has no paired stage:dev-sanity bead blocking on it")

    def check_difficulty(self, bid: str, b: dict[str, Any], *, required: bool) -> None:
        d = meta(b).get("difficulty")
        if d is None or d == "":
            if required:
                self.problem(bid, f"metadata.difficulty missing; one of {', '.join(C.DIFFICULTIES)}")
            return
        if d not in C.DIFFICULTY_MODELS:
            self.problem(bid, f"metadata.difficulty {d!r} is not one of {', '.join(C.DIFFICULTIES)}")

    def check_graph(self) -> None:
        devs = [d for d in self.devs() if d in self.by]
        br2dev = self.branch_to_dev()
        for dev in devs:
            m = meta(self.by[dev])
            pt = str(m.get("pr_target") or "")
            if pt and pt != self.trunk and not (self.trunk == "" and pt.startswith("integrate/")):
                below = br2dev.get(pt)
                if below is None:
                    self.problem(dev, f"pr_target {pt} is neither the integration branch nor a listed sprint's branch")
                else:
                    san = self.sanity_of(below)
                    if san and san not in self.blocker_closure(dev):
                        self.problem(dev, f"pr_target sprint {below}'s sanity bead {san} is not in its blocker closure (add `bd dep add {dev} {san}`)")
            sanity_blockers = [x for x in deps(self.by[dev], "blocks") if has_label(self.by.get(x, {}), C.SANITY_LABEL) or x.endswith("-sanity")]
            rel = str(m.get("relation") or "")
            if rel == "root" and sanity_blockers:
                self.problem(dev, f"relation root but blocked by {', '.join(sanity_blockers)}")
            if rel in ("must_follow", "parallel_safe") and not sanity_blockers and pt != self.trunk:
                self.problem(dev, f"relation {rel} but blocked by no sanity bead")
            for h in m.get("handoffs") or []:
                if not isinstance(h, dict) or not h.get("path") or not h.get("consumer"):
                    self.problem(dev, f"handoff {h!r} needs path and consumer")
                    continue
                path, consumer = str(h["path"]), str(h["consumer"])
                if not path_inside(path, m.get("owned_paths") or []):
                    self.problem(dev, f"handoff {path} is outside its own owned_paths")
                cdev = consumer if consumer in self.by else next((d for d in devs if meta(self.by[d]).get("sprint") == consumer), "")
                if not cdev:
                    self.problem(dev, f"handoff consumer {consumer} is not a listed sprint")
                elif not path_inside(path, meta(self.by[cdev]).get("owned_paths") or []):
                    self.problem(cdev, f"consumes handoff {path} from {dev} but its owned_paths do not include it")
        for i, a in enumerate(devs):
            for b in devs[i + 1:]:
                if b in self.blocker_closure(a) or a in self.blocker_closure(b):
                    continue  # ordered sprints may share paths
                for pa in meta(self.by[a]).get("owned_paths") or []:
                    for pb in meta(self.by[b]).get("owned_paths") or []:
                        if isinstance(pa, str) and isinstance(pb, str) and paths_overlap(pa, pb):
                            self.problem(a, f"owned path {pa} overlaps {b}'s {pb}")

    def check_state(self) -> None:
        if not self.live:
            return
        devs = [d for d in self.devs() if d in self.by]
        sanities = [self.sanity_of(d) for d in devs]
        # history is needed only for sanity beads (reopen after PASS); dev start times come from started_at
        need_history = [x for x in sanities if x in self.by]
        with ThreadPoolExecutor(max_workers=16) as pool:
            hist = dict(zip(need_history, pool.map(lambda i: history_transitions(self.history(i)), need_history)))
        # sanity: never reopened after PASS
        for san in sanities:
            prev = None
            for snap in hist.get(san, []):
                if snap["status"] == "open" and prev and prev["status"] == "closed" and str(prev["close_reason"]).upper().startswith("PASS"):
                    when = snap["at"].isoformat(timespec="minutes") if snap["at"] else "?"
                    self.problem(san, f"reopened at {when} after a PASS; a passed sprint is frozen, later work is a fix bead with its own sanity", check="reopened_after_pass")
                    break
                if snap["status"] != (prev or {}).get("status"):
                    prev = snap
        # dev: blockers closed before it started; open blocker while in progress
        for dev in devs:
            b = self.by[dev]
            if b.get("status") not in ("in_progress", "closed"):
                continue
            started = ts(b.get("started_at")) or next((s["at"] for s in hist.get(dev, []) if s["status"] == "in_progress"), None)
            for blk in deps(b, "blocks"):
                blocker = self.by.get(blk)
                if not blocker:
                    continue
                if blocker.get("status") != "closed":
                    if b.get("status") == "in_progress":
                        self.problem(dev, f"in progress while blocker {blk} is {blocker.get('status')}")
                    continue
                closed_at = ts(blocker.get("closed_at"))
                if started and closed_at and closed_at > started:
                    self.problem(dev, f"started {started.isoformat(timespec='minutes')} before blocker {blk} closed {closed_at.isoformat(timespec='minutes')}", check="started_before_blocker")
        # sanity PASS -> QA bead
        qa_by_dev: dict[str, list[dict[str, Any]]] = {}
        for bid, x in self.by.items():
            if has_label(x, QA_LABEL) and bid in self.phase_ids:
                target = str(meta(x).get("checked_bead") or "") or next((d for d in devs if bid.startswith(d + "-qa")), "")
                if target:
                    qa_by_dev.setdefault(target, []).append(x)
        for dev, san in zip(devs, sanities):
            s = self.by.get(san)
            if not s or not is_pass(s):
                continue
            passed = ts(s.get("closed_at"))
            if not qa_by_dev.get(dev) and passed and self.now - passed > QA_DISPATCH_WINDOW:
                self.problem(dev, f"sanity passed at {passed.isoformat(timespec='minutes')} with no QA bead (create {dev}-qa from qa-bead.json.j2 and dispatch)", check="pass_without_qa")
            fails = [q for q in qa_by_dev.get(dev, []) if q.get("status") == "closed" and is_fail_reason(q.get("close_reason"))]
            if len(fails) >= 2:
                self.problem(dev, f"{len(fails)} QA rounds failed (ROUND_CAP): stop, determine the root cause from the data before any dispatch")
            rounds = [int(str(meta(q).get("round") or 1)) for q in qa_by_dev.get(dev, []) if str(meta(q).get("round") or "1").isdigit()]
            if rounds and max(rounds) > 2:
                self.problem(dev, f"QA round {max(rounds)} exists; at most one fix round is allowed")
        # open PR base == pr_target
        by_head = {str(p.get("headRefName")): p for p in self.prs}
        for dev in devs:
            m = meta(self.by[dev])
            pr = by_head.get(str(m.get("branch") or ""))
            if pr and m.get("pr_target") and pr.get("baseRefName") != m.get("pr_target"):
                self.problem(dev, f"PR #{pr.get('number')} base is {pr.get('baseRefName')}, not pr_target {m.get('pr_target')}")
        # priority and difficulty over the phase set
        for bid in sorted(self.phase_ids):
            x = self.by.get(bid)
            if not x or bid == self.root_id:
                continue
            st = stage(x)
            if bid in devs or bid in sanities:
                if x.get("priority") != C.PRIORITY_SPRINT:
                    self.problem(bid, f"priority P{x.get('priority')} but planned sprint beads are P{C.PRIORITY_SPRINT}")
            elif st == C.FINDING_LABEL:
                sev = severity(x)
                if x.get("status") == "closed":
                    pass  # closed findings are history: priority and severity no longer order any work
                elif sev not in C.SEVERITY_PRIORITY:
                    self.problem(bid, f"finding has no severity label (severity:{'|'.join(C.SEVERITY_PRIORITY)})")
                elif x.get("priority") != C.SEVERITY_PRIORITY[sev]:
                    self.problem(bid, f"{sev} finding is P{x.get('priority')}, the severity map says P{C.SEVERITY_PRIORITY[sev]}")
                self.check_difficulty(bid, x, required=x.get("status") != "closed")
            elif st == C.FIX_LABEL:
                self.check_difficulty(bid, x, required=x.get("status") != "closed")
        # R16: a blocking finding's sanity bead gates every not-started downstream dev bead and every open,
        # unclaimed finding on a downstream sprint; started work is left alone (warned once)
        for fid, f in sorted(self.by.items()):
            if fid not in self.phase_ids or stage(f) != C.FINDING_LABEL or f.get("status") == "closed" or severity(f) != "blocking":
                continue
            sprint = self.sprint_of(fid)
            if not sprint:
                self.problem(fid, "blocking finding has no sprint (metadata.sprint_bead or a sprint dev bead as ancestor)")
                continue
            fsan = self.sanity_of(fid)
            if not fsan:
                self.problem(fid, "blocking finding has no stage:dev-sanity bead; its fix cannot gate downstream work")
                continue
            if fid not in deps(self.by.get(fsan, {}), "blocks"):
                self.problem(fsan, f"sanity gate of blocking finding {fid} does not block on it, so it is ready before the fix lands (bd dep add {fsan} {fid})")
            for target in self.downstream_targets(sprint, self.blocker_closure(fid) | self.blocker_closure(fsan)):
                if fsan not in deps(self.by[target], "blocks"):
                    self.problem(target, f"not blocked by blocking finding {fid}'s sanity bead {fsan} (bd dep add {target} {fsan})")
            for d in self.downstream_devs(sprint):
                if self.by[d].get("status") == "in_progress":
                    self.warn(f"{d} is in progress while blocking finding {fid} on {sprint} is open; rebase on the fix when it lands")
        # dev-sanity template variable types recorded on sanity beads
        for san in sanities:
            m = meta(self.by.get(san, {}))
            base, commit = m.get("base"), m.get("commit")
            if isinstance(base, str) and base and SHA40.match(base):
                self.problem(san, "metadata.base is a SHA; the sanity base must be a branch name")
            if isinstance(commit, str) and commit and not SHA40.match(commit):
                self.problem(san, f"metadata.commit {commit!r} is not a 40-hex SHA")
        # human gates
        allowed = set()
        pol = (self.index or {}).get("policy") or {}
        if isinstance(pol, dict):
            allowed = {str(g) for g in pol.get("human_gates") or []}
        for bid, x in self.by.items():
            gtype = str(x.get("gate_type") or meta(x).get("gate_type") or "")
            if x.get("issue_type") != "gate" and not gtype:
                continue
            gtype = gtype or ("human" if "human" in str(x.get("title") or "").lower() else "human")
            blocked = [t for t, y in self.by.items() if bid in deps(y, "blocks") and t in self.phase_ids]
            if gtype == "human" and blocked and bid not in allowed and x.get("status") != "closed":
                self.problem(bid, f"human gate blocks {', '.join(blocked)} but is not listed in sprints.json policy.human_gates (a phase runs unattended)")

    def run(self) -> tuple[Problems, list[str]]:
        try:
            if self.index:
                validate_index(self.index)
        except RuntimeError as exc:
            self.problem(self.root_id, f"sprints.json: {exc}")
            return self.problems, self.warnings
        if not self.root:
            self.problem(self.root_id, "phase root bead not found")
            return self.problems, self.warnings
        self.check_hierarchy()
        self.check_membership()
        self.check_schema()
        self.check_graph()
        self.check_state()
        return self.problems, self.warnings


# ---------------------------------------------------------------------------------------------- sources

def sh_json(*args: str) -> Any:
    r = subprocess.run(list(args), capture_output=True, text=True)
    if r.returncode:
        raise RuntimeError(f"{' '.join(args)}: {r.stderr.strip() or r.stdout.strip()}")
    try:
        return json.loads(r.stdout or "null")
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"{' '.join(args)} returned invalid JSON: {exc}") from exc


def live_beads(root: str) -> list[dict[str, Any]]:
    """The root, everything under it (transitively), and every bead that blocks one of them."""
    beads = sh_json("bd", "list", "--json", "--all", "--limit", "5000") or []
    return beads if isinstance(beads, list) else []


def live_history(bead: str) -> list[dict[str, Any]]:
    try:
        h = sh_json("bd", "history", bead, "--json")
    except RuntimeError:
        return []
    if isinstance(h, list):
        return h
    if isinstance(h, dict):
        return h.get("entries") or h.get("history") or []
    return []


def live_prs() -> list[dict[str, Any]]:
    try:
        prs = sh_json("gh", "pr", "list", "--state", "open", "--limit", "200", "--json", "number,headRefName,baseRefName,headRefOid")
    except RuntimeError as exc:
        print(C.WARNING_PREFIX + f"gh pr list failed; PR base not checked: {exc}", file=sys.stderr)
        return []
    return prs if isinstance(prs, list) else []


def load_plan(path: Path) -> list[dict[str, Any]]:
    beads = []
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            beads.append(json.loads(line))
        except json.JSONDecodeError as exc:
            raise RuntimeError(f"{path}:{n}: {exc}") from exc
    return beads


def run_fixture(path: Path) -> tuple[Problems, list[str]]:
    fx = json.loads(path.read_text(encoding="utf-8"))
    hist = fx.get("history") or {}
    now = ts(fx.get("now")) or datetime.now(timezone.utc)
    check = PhaseCheck(fx["root"], fx.get("index"), fx["beads"], live=bool(fx.get("live", True)),
                       history=lambda i: hist.get(i, []), prs=fx.get("prs") or [], now=now)
    return check.run()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", help="phase root bead id (live mode)")
    ap.add_argument("--index", type=Path, help="authored sprints.json (default: docs/plans/phase-<p>/sprints.json)")
    ap.add_argument("--plan", type=Path, help="import-shaped plan.jsonl to check before beads exist")
    ap.add_argument("--fixture", type=Path, help="committed fixture JSON (tests)")
    ap.add_argument("--scope", help="accepted for interface parity; doc checks are scoped by validate-plan")
    a = ap.parse_args()
    try:
        if a.fixture:
            problems, warnings = run_fixture(a.fixture)
        elif a.plan:
            beads = load_plan(a.plan)
            roots = [b for b in beads if b.get("issue_type") in ("epic", "feature")]
            root = a.root or (str(roots[0]["id"]) if roots else "")
            if not root:
                print("phase_contract_check: pass --root when the plan file has no root bead", file=sys.stderr)
                return C.EXIT_CANNOT_RUN
            index = json.loads(a.index.read_text(encoding="utf-8")) if a.index and a.index.exists() else None
            problems, warnings = PhaseCheck(root, index, beads, live=False).run()
        elif a.root:
            beads = live_beads(a.root)
            index_path = a.index
            if index_path is None:
                here = Path(__file__).resolve().parent
                out = subprocess.run([str(here / "phase-index-path"), "--root", a.root], capture_output=True, text=True)
                if out.returncode:
                    print(f"phase_contract_check: {out.stderr.strip()}", file=sys.stderr)
                    return C.EXIT_CANNOT_RUN
                index_path = Path(out.stdout.strip())
            if not index_path.exists():
                print(C.PROBLEM_LINE.format(bead=a.root, message=f"authored phase file {index_path} is missing; the planner commits it with the plan PR"))
                return C.EXIT_PROBLEMS
            index = json.loads(index_path.read_text(encoding="utf-8"))
            problems, warnings = PhaseCheck(a.root, index, beads, live=True, history=live_history, prs=live_prs()).run()
        else:
            ap.error("pass --root, --plan or --fixture")
            return C.EXIT_CANNOT_RUN
    except (OSError, RuntimeError, ValueError, KeyError) as exc:
        print(f"phase_contract_check: {exc}", file=sys.stderr)
        return C.EXIT_CANNOT_RUN
    for w in warnings:
        print(w)
    for p in problems:
        print(p)
    return C.EXIT_PROBLEMS if problems else C.EXIT_VALID


if __name__ == "__main__":
    raise SystemExit(main())
