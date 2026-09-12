"""Shared provenance and sc-compose plumbing for report-producing runners."""

from __future__ import annotations

import json
from pathlib import Path
import re
import subprocess
import tempfile
from typing import Any, TypeVar


class ReportRuntimeError(RuntimeError):
    """A report could not be rendered or its provenance could not be read."""


E = TypeVar("E", bound=Exception)
GIT_REVISION = re.compile(r"^[0-9a-f]{40}$")


def source_revision(root: Path | None = None) -> str | None:
    """Return the exact checkout revision, or ``None`` when it is unresolved."""
    checkout = root or Path(__file__).resolve().parents[2]
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=checkout,
        capture_output=True, text=True, check=False,
    )
    revision = result.stdout.strip()
    return revision if result.returncode == 0 and GIT_REVISION.fullmatch(revision) else None


def compose(
    template: Path,
    variables: dict[str, Any],
    output: Path,
    root: Path | None = None,
    error_type: type[E] = ReportRuntimeError,  # type: ignore[assignment]
) -> None:
    """Render a checked-in template through the repository sc-compose binary."""
    checkout = root or Path(__file__).resolve().parents[2]
    output.parent.mkdir(parents=True, exist_ok=True)
    variables_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile("w", suffix=".json", encoding="utf-8", delete=False) as handle:
            json.dump(variables, handle, sort_keys=True)
            variables_path = Path(handle.name)
        completed = subprocess.run(
            ["sc-compose", "render", "--root", str(checkout), "--file", str(template),
             "--var-file", str(variables_path), "--output", str(output)],
            cwd=checkout, capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
        )
        if completed.returncode != 0:
            raise error_type(f"sc-compose render failed: {completed.stderr.strip() or completed.stdout.strip()}")
    finally:
        if variables_path is not None:
            variables_path.unlink(missing_ok=True)
