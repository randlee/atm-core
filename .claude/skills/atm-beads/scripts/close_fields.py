"""Close fields carried by every poured dev, fix, sanity and QA bead, and the close vars built from one bead (#557).

A pour stamps `POURED` into the bead's metadata, so a validated open bead holds everything its close needs
that is known at pour time. `close_vars` turns that one bead into the `--vars` object for
`atm-task-close.py --role <role>`: the poured fields, the task ids (equal to the bead ids) and the two
host paths, projected onto the role's fields in `task-close-vars.j2`. It reads no other bead.
"""

from __future__ import annotations

from pathlib import Path

import yaml

CONTRACT = Path(__file__).resolve().parents[2] / "atm-bd-orchestration" / "templates" / "task-close-vars.j2"
ROLE_BY_STAGE = {"stage:dev": "dev", "stage:fix": "fix", "stage:dev-sanity": "sanity", "stage:qa": "qa"}
# Stamped at pour on every open stage bead; bead_schema.CloseMetadata enforces them.
POURED = ("role", "phase", "sprint", "sprint_bead", "stack", "stack_bead", "branch", "worktree", "pr_target", "base",
          "checked_bead", "checked_title", "sanity_bead", "qa_bead", "phase_feature", "lint_command", "policy_path",
          "max_sanity_rounds")
# A QA bead's reviewers_round1 is planned on that bead (bead_schema.QaMetadata); a sanity close takes it from its qa_bead.
# Host-local: never stored on a bead.
HOST = ("primary_checkout", "worktree_path")


class CloseFieldsError(ValueError):
    def __init__(self, code: str, message: str, fix: str):
        super().__init__(message)
        self.code, self.fix = code, fix

    def as_json(self) -> dict:
        return {"code": self.code, "message": str(self), "fix": self.fix}


def contract() -> dict:
    return yaml.safe_load(CONTRACT.read_text(encoding="utf-8").split("---", 2)[1])


def stamp(*, role: str, phase: str, sprint: str, sprint_bead: str, stack: str, stack_bead: str, branch: str,
          worktree: str, pr_target: str, checked_bead: str, checked_title: str, sanity_bead: str, qa_bead: str,
          phase_feature: str, config: dict) -> dict:
    """The POURED metadata of one bead; `config` is the repository configuration (repo_config.load)."""
    return {"role": role, "phase": phase, "sprint": sprint, "sprint_bead": sprint_bead, "stack": stack,
            "stack_bead": stack_bead, "branch": branch, "worktree": worktree, "pr_target": pr_target, "base": pr_target,
            "checked_bead": checked_bead, "checked_title": checked_title, "sanity_bead": sanity_bead, "qa_bead": qa_bead,
            "phase_feature": phase_feature, "lint_command": config["lint_command"], "policy_path": config["policy_path"],
            "max_sanity_rounds": config["max_sanity_rounds"]}


def role_of(row: dict) -> str:
    stages = [label for label in row.get("labels") or [] if label in ROLE_BY_STAGE]
    if len(stages) != 1:
        raise CloseFieldsError("CLOSE_VARS.NOT_A_TASK_BEAD", f"{row.get('id')}: labels {row.get('labels')} name no single "
                               "stage:dev, stage:fix, stage:dev-sanity or stage:qa", "Pass the id of a poured dev, fix, sanity or QA bead.")
    return ROLE_BY_STAGE[stages[0]]


def close_vars(row: dict, repo: Path) -> tuple[str, dict, list[str]]:
    """(role, vars, fields the closing agent still supplies) for one bead; refuses a bead missing a poured field."""
    role = role_of(row)
    metadata = row.get("metadata") or {}
    missing = [name for name in POURED if metadata.get(name) in (None, "", [])]
    if missing:
        raise CloseFieldsError("CLOSE_VARS.NOT_POURED", f"{row['id']}: metadata lacks {', '.join(missing)}",
                               f"The pour or backfill did not stamp {row['id']}: ask the phase lead to run "
                               f"close_fields_backfill.py for its phase; never assemble these fields from other beads.")
    if metadata["role"] != role:
        raise CloseFieldsError("CLOSE_VARS.ROLE_MISMATCH", f"{row['id']}: metadata.role {metadata['role']!r} but its stage is {role}",
                               f"Correct metadata.role on {row['id']} to {role!r}.")
    worktree = Path(metadata["worktree"])
    available = {**{name: metadata[name] for name in POURED if name not in ("role", "worktree")},
                 "task_id": row["id"], "dev_task_id": metadata["checked_bead"], "sanity_task_id": metadata["sanity_bead"],
                 "qa_task_id": metadata["qa_bead"], "primary_checkout": str(repo),
                 "worktree_path": str(worktree if worktree.is_absolute() else (repo / worktree).resolve())}
    terms = contract()
    required = terms["required_variables_by_role"][role]
    values = {name: available[name] for name in required if name in available}
    remaining = [name for name in required if name not in values]
    return role, values, remaining


# The checked bead records these at its dev or fix close (atm-task-close.py); a QA assignment takes them from it.
RECORDED = ("commit", "branch", "pr_number")


def qa_assignment_vars(qa_row: dict, checked_row: dict, repo: Path) -> dict:
    """The `templates.qa` vars of one QA bead: its poured fields plus the PR its checked bead recorded.

    The PR, commit and branch come from the checked bead, never from hand-assembled vars, so a QA assignment
    cannot render without its PR (#2214). A value the QA bead also holds must agree with the checked bead.
    """
    role, values, _ = close_vars(qa_row, repo)
    if role != "qa":
        raise CloseFieldsError("ASSIGN_VARS.NOT_QA", f"{qa_row['id']} is a {role} bead",
                               "Pass the QA bead; only QA assignments are built here.")
    metadata = qa_row.get("metadata") or {}
    checked = checked_row.get("metadata") or {}
    missing = [name for name in RECORDED if checked.get(name) in (None, "")]
    if missing:
        raise CloseFieldsError("ASSIGN_VARS.NOT_RECORDED", f"{checked_row['id']}: metadata lacks {', '.join(missing)}",
                               f"{checked_row['id']} has no recorded close: finish its dev or fix close "
                               f"(atm-task-close.py records commit, branch and pr_number), then rebuild these vars.")
    conflicts = [f"{name} {metadata[name]!r} != {checked[name]!r}" for name in RECORDED
                 if metadata.get(name) not in (None, "") and str(metadata[name]) != str(checked[name])]
    if conflicts:
        raise CloseFieldsError("ASSIGN_VARS.CONFLICT", f"{qa_row['id']} disagrees with {checked_row['id']}: {'; '.join(conflicts)}",
                               f"Correct the stale value on {qa_row['id']} to the checked bead's recorded close.")
    for name in ("layer", "round", "reviewers_round1"):
        if metadata.get(name) in (None, "", []):
            raise CloseFieldsError("ASSIGN_VARS.NOT_PLANNED", f"{qa_row['id']}: metadata lacks {name}",
                                   f"Set {name} on {qa_row['id']} (layer when its PR is linked, round and reviewers at pour).")
    is_fix = checked.get("role") == "fix" or "stage:fix" in (checked_row.get("labels") or [])
    return {"task_id": qa_row["id"], "qa_task_id": qa_row["id"], "checked_bead": values["checked_bead"],
            "dev_task_id": values["checked_bead"], "sanity_task_id": metadata.get("sanity_bead") or "",
            "phase": metadata["phase"], "phase_feature": metadata["phase_feature"], "sprint": metadata["sprint"],
            "sprint_bead": metadata["sprint_bead"], "stack": metadata["stack"], "layer": metadata["layer"],
            "round": metadata["round"], "base": metadata["base"], "worktree_path": values["worktree_path"],
            "policy_path": metadata["policy_path"], "reviewers_round1": metadata["reviewers_round1"],
            "commit": str(checked["commit"]), "branch": str(checked["branch"]), "pr_number": str(checked["pr_number"]),
            "carry_forward": checked_row["id"] if is_fix else ""}
