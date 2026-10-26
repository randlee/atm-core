#!/usr/bin/env python3
"""Freeze the ANC-022 v4 oracle (planning gate) before any Jev call. Expectations come only from ANC-022 v4 / INTENT v2."""
import hashlib
import json

W = "/Users/randlee/github/atm-core-worktrees/plan/phase-bj"
T = W + "/docs/plans/phase-bj/anchor-runs/tuning"
D = T + "/ANC-022-v4"
req = json.load(open(D + "/request.json"))
a = req["authority"]["anchors"]["records"][0]
bj1 = json.load(open(T + "/ANC-022/triage-bj-1/runs/canonical-request.json"))["state"]["item"]["text"]

S, NO = {"relevant": "yes", "contradicted": "no"}, {"relevant": "no", "contradicted": "no"}
C = {"relevant": "yes", "contradicted": "yes"}
cases = [
    ("fixture_pass", "approved_fixture_pass", a["fixtures"]["pass"][0], S, "satisfied",
     "ANC-022 v4 fixtures.pass[0] verbatim."),
    ("fixture_fail", "approved_fixture_fail", a["fixtures"]["fail"][0], C, "contradicted",
     "ANC-022 v4 fixtures.fail[0] verbatim; summary 'dropped, not aliased'."),
    ("compliant_removal", "minimal_compliant",
     "Remove the atm-bd-console entry from [project.scripts] in pyproject.toml and change every README and cron example that ran atm-bd-console to run `atm-bd console` instead.",
     S, "satisfied", "Edits the command name and its callers and does exactly what the v4 statement requires (replaced by the `atm-bd console` subcommand)."),
    ("subcommand_doc", "boundary_subcommand", "The README's operator section tells the lead to open the dashboard with `atm-bd console --phase bj`.",
     S, "satisfied", "A document invoking the console is a caller (scope); it invokes the replacement subcommand the statement requires, so no conflict."),
    ("subcommand_script", "boundary_subcommand", "bead_queues.py starts the dashboard by running `atm-bd console` as a subprocess.",
     S, "satisfied", "A script calling the console (scope); calls `atm-bd console`, which the statement names as the replacement."),
    ("subcommand_feature", "topical_boundary", "`atm-bd console` gains a sanity runs tab and a quality runs tab, newest run on top.",
     S, "satisfied", "Changes the console a caller runs (scope: the console and its callers); names no atm-bd-console command."),
    ("exception_directory_module", "exception",
     "Keep the Textual app in the directory apps/atm-bd-console and import it as the atm_bd_console module from the `atm-bd console` subcommand.",
     S, "satisfied", "v4 quote 'atm-bd console would logically be in atm-bd-console module' and check 'outside apps/atm-bd-console and atm_bd_console': directory and module are excepted."),
    ("conflict_entry_point", "direct_conflict",
     "Keep atm-bd-console as a [project.scripts] entry point so existing cron jobs can keep running atm-bd-console --phase bj.",
     C, "contradicted", "v4 check: 'command -v atm-bd-console fails'; an installed command plus cron callers is not replaced."),
    ("conflict_forwarding_alias", "boundary_conflict", "Add an atm-bd-console console script that forwards to `atm-bd console`, as a compatibility alias.",
     C, "contradicted", "v4 summary 'atm-bd-console is dropped, not aliased'."),
    ("conflict_test", "direct_conflict", "Add a test that runs atm-bd-console --help and checks that the sanity tab is listed.",
     C, "contradicted", "v4 check: no atm-bd-console invocation outside the directory and module."),
    ("conflict_doc", "direct_conflict", "The README quick start keeps the line `atm-bd-console --phase bj` for users who have not switched yet.",
     C, "contradicted", "v4 check: no atm-bd-console invocation; v4 statement: replaced."),
    ("silent_install_rename", "silent_in_scope", "Rename scripts/install.py to scripts/install_skill.py and update the README install instructions to the new name.",
     S, "satisfied", "Edits an install script (scope 'install scripts'); silent about the console, and silence is no conflict for a prohibition."),
    ("silent_install_config", "silent_in_scope",
     "`install.py` renders the new repository configuration key `max_sanity_rounds` from the consumer registry when present, else 5.",
     S, "satisfied", "Edits an install script (scope 'install scripts'); silent about the console. Same pattern as triage-bj-1 Deliverable 9."),
    ("silent_install_verify", "silent_in_scope",
     "During install, `atm-bd skill` verifies every installed file against the release manifest and refuses mismatches with the path list.",
     S, "satisfied", "Edits the install path / installer (scope 'install scripts'); names no atm-bd-console command."),
    ("irrelevant_template", "irrelevant", "Change the sanity-fail message template so it names the failing deliverable number.",
     NO, "not_applicable", "A message-template change touches no console command name, install script or console caller (scope)."),
    ("irrelevant_model", "irrelevant",
     "Add a pydantic model per bead kind and make validate-plan reject a phase whose beads lack branch, worktree, pr_target or difficulty.",
     NO, "not_applicable", "A data model and validation rule; no console command name, install script or console caller (scope)."),
    ("topical_only_outofscope", "topical_only",
     "The phase plan's out-of-scope list notes that the small console icon fixes close in phase-bi.",
     NO, "not_applicable", "Mentions the console topic but edits no command name, no install script and no caller of the console (scope)."),
    ("known_bj1_item", "known_example", bj1, S, "satisfied",
     "Disputed production item (triage-bj-1, run plan-20261019T013526Z-atm-bj-1): Deliverable 9 edits install.py (scope 'install scripts'); no deliverable keeps, adds, aliases or runs atm-bd-console. Expectation from ANC-022 v4 scope and check, adopted by the independent triage."),
]
oracle = {
    "schema": "plan-anchor/tuning-oracle/1", "anchor_id": "ANC-022", "anchor_version": a["version"],
    "anchor_statement": a["statement"], "anchor_scope": a["scope"], "anchor_kind": a["kind"], "gate": "planning",
    "roles": {"relevance": "relevant", "compliance": "addressed", "conflict": "contradicted"},
    "graded_roles": ["relevant", "contradicted"],
    "ungraded_roles_note": "ANC-022 is a prohibition: jev_judge.decide ignores the compliance role, so `addressed` is recorded but not graded.",
    "semantic_mapping": "jev_judge.decide(answers, roles, 0.0, 'prohibition')", "repeat_count": 2,
    "frozen_before_first_jev_call": True,
    "authority": [f"ANC-022 v{a['version']} record sha256 {hashlib.sha256(json.dumps(a, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()).hexdigest()}",
                  f"INTENT phase-bj v{req['authority']['intent']['version']} sha256 {req['authority']['intent']['sha256']}"],
    "cases": [{"id": i, "item_id": f"item-{n:02d}", "category": cat, "text": t, "expected": e, "verdict": v, "why": w}
              for n, (i, cat, t, e, v, w) in enumerate(cases, 1)],
}
json.dump(oracle, open(D + "/oracle.json", "w"), indent=1, ensure_ascii=False)
print(len(oracle["cases"]))
