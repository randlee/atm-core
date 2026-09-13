"""Cross-domain provenance and rendering plumbing for report-producing runners.

This module is shared by smoke, benchmark, and fuzz report producers. Keeping
one owner avoids each evidence family growing a subtly different renderer.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import datetime, timezone
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
    checkout = root or Path(__file__).resolve().parents[1]
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=checkout,
        capture_output=True, text=True, check=False,
    )
    revision = result.stdout.strip()
    return revision if result.returncode == 0 and GIT_REVISION.fullmatch(revision) else None


def resolve_procedure_revision(
    revisions: list[dict[str, Any]],
    revision: str,
    *,
    root: Path,
    generated_at: str | None = None,
) -> dict[str, Any] | None:
    """Select the exact page, newest ancestor, or dated fallback entry.

    Report indexes and live report writers must agree on this decision.  Git
    ancestry is authoritative when available; a missing Git checkout is an
    expected condition for archived/publication-only consumers and therefore
    falls back to the newest entry already in effect at the report timestamp.
    """
    if not GIT_REVISION.fullmatch(revision):
        raise ReportRuntimeError(f"invalid source revision: {revision!r}")
    selected = next((item for item in revisions if item.get("rev") == revision), None)
    if selected is not None:
        return selected
    repository = subprocess.run(
        ["git", "rev-parse", "--git-dir"], cwd=root, capture_output=True, check=False
    )
    git_unavailable = repository.returncode != 0
    ancestors: list[dict[str, Any]] = []
    if not git_unavailable:
        for item in revisions:
            candidate = item.get("rev")
            if not isinstance(candidate, str) or not GIT_REVISION.fullmatch(candidate):
                continue
            result = subprocess.run(
                ["git", "merge-base", "--is-ancestor", candidate, revision],
                cwd=root,
                capture_output=True,
                check=False,
            )
            if result.returncode == 0:
                ancestors.append(item)
            elif result.returncode != 1:
                git_unavailable = True
                break
    if not git_unavailable:
        return max(ancestors, key=lambda item: item.get("date", ""), default=None)
    cutoff = (generated_at or datetime.now(timezone.utc).date().isoformat())[:10]
    dated = [
        item for item in revisions
        if isinstance(item.get("date"), str) and item["date"] <= cutoff
    ]
    return max(dated, key=lambda item: item["date"], default=None)


def resolve_procedure_page(
    procedure: str,
    revision: str | None,
    root: Path | None = None,
    generated_at: str | None = None,
    error_type: type[E] = ReportRuntimeError,  # type: ignore[assignment]
) -> ProcedurePage | None:
    """Select the exact page or newest manifest ancestor for ``revision``."""
    checkout = root or Path(__file__).resolve().parents[1]
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
    try:
        selected = resolve_procedure_revision(
            revisions, revision, root=checkout, generated_at=generated_at,
        )
    except ReportRuntimeError as error:
        raise error_type(str(error)) from error
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
    checkout = root or Path(__file__).resolve().parents[1]
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
