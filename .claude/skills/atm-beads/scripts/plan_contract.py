"""Frozen phase-contract constants shared by validate-plan, the orchestration templates and the dispatch view.

This module is the layer-1 interface. Every consumer imports these names;
no script redefines them. Changing a value here is a plan-contract change and needs a plan PR.
"""

from __future__ import annotations

# Finding priority follows severity so `bd ready` orders blocking work ahead of planned sprints,
# important work level with them, and minor work behind them (user ruling R12, 2026-09-26).
SEVERITY_PRIORITY: dict[str, int] = {"blocking": 1, "important": 2, "minor": 4}

# Planned sprint dev beads and their sanity beads sit at this priority.
PRIORITY_SPRINT: int = 2

# Difficulty of a sprint or finding bead and the model classes allowed to take it (user ruling R14).
# A roster `model` string matches a class when it contains the class name (e.g. "gpt-5.6-terra", "claude-opus-5-5").
DIFFICULTY_MODELS: dict[str, tuple[str, ...]] = {
    "hard": ("fable", "opus", "astra"),
    "normal": ("terra", "opus", "sonnet"),
    "fast": ("luna",),
}
DIFFICULTIES: tuple[str, ...] = tuple(DIFFICULTY_MODELS)

# Legacy `metadata.model_class` values and the difficulty each migrates to.
MODEL_CLASS_TO_DIFFICULTY: dict[str, str] = {"astra": "hard", "terra": "normal", "luna": "fast"}

# Labels and dependency relation names shared by orchestration scripts.  Keep
# these here so a graph rule cannot silently diverge from the plan contract.
DEV_LABEL: str = "stage:dev"
SPRINT_LABEL: str = "stage:sprint"
SANITY_LABEL: str = "stage:dev-sanity"
FINDING_LABEL: str = "stage:finding"
FIX_LABEL: str = "stage:fix"
BLOCKS_RELATION: str = "blocks"
PARENT_CHILD_RELATION: str = "parent-child"

# validate-plan exit codes and output contract.
EXIT_VALID: int = 0
EXIT_CANNOT_RUN: int = 2
EXIT_PROBLEMS: int = 5
PROBLEM_LINE: str = "{bead}: {message}"  # one per stdout line

# Legacy in-memory fixture keys. Persisted phase plans use sprints.jsonl tuples.
INDEX_OPTIONAL_KEYS: tuple[str, ...] = ("integration_branch", "review_artifacts", "policy")
POLICY_KEYS: tuple[str, ...] = ("human_gates", "waivers")
# policy.waivers[{bead, check, reason}]: a past state violation the user accepted in the plan PR; the validator
# prints it as a warning instead of a problem. `check` names the state invariant.
WAIVABLE_CHECKS: tuple[str, ...] = ("reopened_after_pass", "started_before_blocker", "pass_without_qa")


def model_matches(model: str | None, difficulty: str) -> bool:
    """True when a roster model string may take work of the given difficulty."""
    if not model or difficulty not in DIFFICULTY_MODELS:
        return False
    m = model.lower()
    return any(cls in m for cls in DIFFICULTY_MODELS[difficulty])


def priority_for_severity(severity: str) -> int:
    """Bead priority for a finding severity; raises KeyError on an unknown severity."""
    return SEVERITY_PRIORITY[severity]
