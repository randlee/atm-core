"""Cross-domain provenance and rendering plumbing for report-producing runners.

This module stays under ``scripts/smoke`` because smoke owns the public report
layout and all current consumers render into that layout, including the fuzz
adapter.  Keeping one owner avoids a second top-level report utility.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
import re
import subprocess
import tempfile
from typing import Any, TypeVar


class ReportRuntimeError(RuntimeError):
    """A report could not be rendered or its provenance could not be read."""


E = TypeVar("E", bound=Exception)
GIT_REVISION = re.compile(r"^[0-9a-f]{40}$")


@dataclass(frozen=True)
class ProcedurePage:
    """A manifest-backed procedure page selected for one source revision."""

    revision: str
    html: str


def source_revision(root: Path | None = None) -> str | None:
    """Return the exact checkout revision, or ``None`` when it is unresolved."""
    checkout = root or Path(__file__).resolve().parents[2]
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=checkout,
        capture_output=True, text=True, check=False,
    )
    revision = result.stdout.strip()
    return revision if result.returncode == 0 and GIT_REVISION.fullmatch(revision) else None


def resolve_procedure_page(
    procedure: str,
    revision: str | None,
    root: Path | None = None,
    error_type: type[E] = ReportRuntimeError,  # type: ignore[assignment]
) -> ProcedurePage | None:
    """Select the exact page or newest manifest ancestor for ``revision``.

    A missing source revision has no exact provenance and therefore returns
    ``None`` so callers can link the procedure's stable index.  A resolved
    revision must never degrade to a guessed or nonexistent page.
    """
    checkout = root or Path(__file__).resolve().parents[2]
    if revision is None:
        return None
    if not GIT_REVISION.fullmatch(revision):
        raise error_type(f"invalid source revision for procedure {procedure}: {revision!r}")
    manifest_path = checkout / "site/reports/procedures/manifest.json"
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        procedures = manifest["procedures"]
        entry = next(item for item in procedures if item.get("procedure") == procedure)
        revisions = entry["revisions"]
    except (OSError, UnicodeError, json.JSONDecodeError, KeyError, StopIteration, TypeError) as error:
        raise error_type(f"unable to resolve procedure {procedure} from {manifest_path}: {error}") from error
    selected = next((item for item in revisions if item.get("rev") == revision), None)
    if selected is None:
        for item in revisions:
            candidate = item.get("rev")
            if not isinstance(candidate, str) or not GIT_REVISION.fullmatch(candidate):
                continue
            result = subprocess.run(
                ["git", "merge-base", "--is-ancestor", candidate, revision],
                cwd=checkout,
                capture_output=True,
                text=True,
                check=False,
            )
            if result.returncode == 0:
                selected = item
                break
            if result.returncode != 1:
                detail = result.stderr.strip() or result.stdout.strip() or "git ancestry check failed"
                raise error_type(f"unable to resolve procedure {procedure}: {detail}")
    if selected is None:
        raise error_type(f"procedure {procedure} has no page for revision {revision}")
    selected_revision = selected.get("rev")
    selected_html = selected.get("html")
    if not isinstance(selected_revision, str) or not isinstance(selected_html, str):
        raise error_type(f"procedure {procedure} has a malformed revision entry")
    if not (checkout / "site/reports" / selected_html).is_file():
        raise error_type(f"procedure {procedure} page does not exist: {selected_html}")
    return ProcedurePage(revision=selected_revision, html=selected_html)


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
