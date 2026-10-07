"""Formula lookup and the small `bd` wrapper shared by `sc-compose-pour-mock` and `bead-groups`.

A formula is two files: `<name>.formula.toml.j2` (the sc-compose source) and
`<name>.relations.json` (bead-groups data: attach ref and post-pour edges).
Each file is looked up on `search_path(repo)`, first match wins:

1. `<repo>/.atm-bd/formula/` - the repository override (tracked in git; the
   rest of `.atm-bd/` is not);
2. `skills/atm-bd-orchestration/formulas/` - the package default.
"""
from __future__ import annotations

import json
import subprocess
from pathlib import Path

PACKAGE_FORMULAS = Path(__file__).resolve().parents[1] / "formulas"
OVERRIDE_DIR = Path(".atm-bd") / "formula"
TEMPLATE_SUFFIX = ".formula.toml.j2"
RELATIONS_SUFFIX = ".relations.json"
# Poured beads record where they came from, so a re-run can tell its own bead from a conflicting one.
PROVENANCE_KEY = "sc_compose_attach"


def search_path(repo: Path | None) -> list[Path]:
    """Directories searched for formula files, highest precedence first."""
    return ([Path(repo) / OVERRIDE_DIR] if repo is not None else []) + [PACKAGE_FORMULAS]


def find(name: str, suffix: str, repo: Path | None) -> Path:
    """The first `<name><suffix>` on the search path; FileNotFoundError when none exists."""
    for directory in search_path(repo):
        candidate = directory / f"{name}{suffix}"
        if candidate.is_file():
            return candidate
    raise FileNotFoundError(f"no {name}{suffix} in {[str(d) for d in search_path(repo)]}")


def template(name: str, repo: Path | None) -> Path:
    return find(name, TEMPLATE_SUFFIX, repo)


def relations(name: str, repo: Path | None) -> Path:
    return find(name, RELATIONS_SUFFIX, repo)


def attach_id(parent: str, ref: str, step: str) -> str:
    """Stable id of a step poured directly under `parent`: <parent>.<ref>-<step>.

    One dotted level only. `bd mol bond --ref` makes <parent>.<ref>.<step> under an
    intermediate <parent>.<ref> molecule; bd 1.3.0 refuses a parent-child edge from
    <parent>.<ref>.<step> straight to <parent> ("already a child ... via hierarchy"),
    and that implicit id hierarchy does not stop `bd close <parent>`.
    """
    return f"{parent}.{ref}-{step}"


class BdError(RuntimeError):
    def __init__(self, argv: list[str], returncode: int, stdout: str, stderr: str):
        detail = (stderr or "").strip() or (stdout or "").strip()
        super().__init__(f"{' '.join(argv[:3])} exited {returncode}: {detail}")
        self.argv, self.returncode, self.stdout, self.stderr = argv, returncode, stdout, stderr


class Bd:
    """Runs one `bd` executable; every call is logged in `self.log` as (argv, returncode)."""

    def __init__(self, executable: str = "bd"):
        self.executable = executable
        self.log: list[tuple[list[str], int]] = []

    def run(self, *args: str, check: bool = True) -> subprocess.CompletedProcess:
        argv = [self.executable, *args]
        proc = subprocess.run(argv, text=True, capture_output=True)
        self.log.append((argv, proc.returncode))
        if check and proc.returncode:
            raise BdError(argv, proc.returncode, proc.stdout, proc.stderr)
        return proc

    def show(self, bead: str) -> dict | None:
        """The bead as `bd show --json` reports it, or None when it does not exist."""
        proc = self.run("show", bead, "--json", check=False)
        if proc.returncode:
            text = (proc.stderr + proc.stdout).lower()
            if "not found" in text or "no issue" in text:
                return None
            raise BdError([self.executable, "show", bead], proc.returncode, proc.stdout, proc.stderr)
        rows = json.loads(proc.stdout)
        return rows[0] if rows else None

    def deps(self, bead: str) -> dict[str, str]:
        """{depends_on_id: dependency type} for the edges `bead` holds."""
        rows = json.loads(self.run("dep", "list", bead, "--json").stdout or "[]")
        return {row["id"]: row.get("dependency_type") or row.get("type") for row in rows}

    def dep_add(self, bead: str, depends_on: str, dep_type: str) -> None:
        self.run("dep", "add", bead, depends_on, "--type", dep_type)
