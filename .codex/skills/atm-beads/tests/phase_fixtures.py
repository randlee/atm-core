"""Phase settings and installed assignment files for cross-skill tests."""
from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path

TEMPLATE_DIRECTORY = Path(".claude/skills/atm-bd-orchestration/templates")
TEMPLATE_NAMES = {
    "dev": "dev-template.xml.j2",
    "fix": "fix-assignment.xml.j2",
    "sanity": "dev-sanity-template.xml.j2",
    "qa": "qa-template.xml.j2",
    "plan_review": "plan-review-template.xml.j2",
    "review": "review-template.xml.j2",
}


def run_subprocess(argv, *, on_timeout=None, **kwargs):
    try:
        return subprocess.run(argv, timeout=60, **kwargs)
    except subprocess.TimeoutExpired as error:
        diagnostic = on_timeout() if on_timeout else ""
        raise AssertionError(f"{argv!r} timed out; stdout: {error.stdout!r}; stderr: {error.stderr!r}{diagnostic}") from error


def phase_data(phase="t", prefix="x", plans_dir="plans", lead="phase-lead"):
    return {
        "plan": f"{plans_dir}/phase-{phase}.jsonl",
        "root": f"{prefix}-phase-{phase}",
        "integration_branch": f"integrate/phase-{phase}",
        "lead": lead,
        "templates": {key: str(TEMPLATE_DIRECTORY / name) for key, name in TEMPLATE_NAMES.items()},
    }


def phase_toml(data):
    scalars = "".join(f"{key} = {json.dumps(value)}\n" for key, value in data.items() if key != "templates")
    if "templates" not in data:
        return scalars
    return scalars + "\n[templates]\n" + "".join(
        f"{key} = {json.dumps(value)}\n" for key, value in data["templates"].items())


def install_templates(repo):
    source = Path(__file__).resolve().parents[2] / "atm-bd-orchestration/templates"
    target = Path(repo) / TEMPLATE_DIRECTORY
    shutil.copytree(source, target, dirs_exist_ok=True)


def write_phase(repo, phase="t", prefix="x", plans_dir="plans", lead="phase-lead"):
    install_templates(repo)
    path = Path(repo) / ".atm-bd" / f"phase-{phase}.toml"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(phase_toml(phase_data(phase, prefix, plans_dir, lead)), encoding="utf-8")
    return path
