#!/usr/bin/env python3
"""Enforce Python-package dependency policy for the public atm_graft wheel."""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import sys
import tomllib

BOUNDARY = Path("boundaries/atm-graft-python/hermes-graft-binding.toml")
PACKAGE = Path("crates/atm-graft-python/pyproject.toml")
EXPECTED = {"pydantic"}

@dataclass(frozen=True)
class Violation:
    location: str
    message: str

def _name(spec: str) -> str:
    return spec.split(";", 1)[0].split("[", 1)[0].split("=", 1)[0].split("<", 1)[0].split(">", 1)[0].strip()

def collect_violations(root: Path) -> list[Violation]:
    boundary = tomllib.loads((root / BOUNDARY).read_text(encoding="utf-8"))
    package = tomllib.loads((root / PACKAGE).read_text(encoding="utf-8"))
    declared = set(boundary.get("dependencies", {}).get("allowed_dependencies", []))
    dependencies = {_name(value) for value in package.get("project", {}).get("dependencies", [])}
    findings: list[Violation] = []
    if dependencies != EXPECTED:
        findings.append(Violation(str(PACKAGE), f"Python dependencies must be exactly {sorted(EXPECTED)}"))
    if not dependencies <= declared:
        findings.append(Violation(str(BOUNDARY), "allowed_dependencies must include every Python package dependency"))
    contracts = boundary.get("contracts", {})
    required = {"AtmSendRequest", "AtmReadRequest", "AtmListRequest", "AtmSendResult", "AtmReadResult", "AtmListResult"}
    present = set(contracts.get("request_types", [])) | set(contracts.get("response_types", []))
    if not required <= present:
        findings.append(Violation(str(BOUNDARY), "contracts must name every native tool request/result type"))
    return findings

if __name__ == "__main__":
    root = Path(__file__).resolve().parents[1]
    findings = collect_violations(root)
    for item in findings:
        print(f"{item.location}: {item.message}")
    raise SystemExit(bool(findings))
