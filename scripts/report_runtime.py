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
import shutil
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


def copy_procedure_page(page: ProcedurePage | None, *, linking_page: Path, copy_dir: Path, root: Path | None = None, error_type: type[E] = ReportRuntimeError) -> str | None:
    """Copy the resolved procedure into a report directory and return a local href."""
    if page is None:
        return None
    checkout = root or Path(__file__).resolve().parents[1]
    source = checkout / "site/reports" / page.html
    try:
        relative = copy_dir.relative_to(linking_page.parent)
    except ValueError as error:
        raise error_type("procedure copy directory must be the linking page directory or a descendant") from error
    destination = copy_dir / "procedure.html"
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, destination)
    return Path(relative, "procedure.html").as_posix()


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
    if not git_unavailable:
        shallow = subprocess.run(
            ["git", "rev-parse", "--is-shallow-repository"],
            cwd=root, capture_output=True, text=True, check=False,
        )
        if shallow.stdout.strip() == "true":
            # A shallow clone lacks procedure commits, so ancestry would silently
            # degrade to the dated fallback and select different pages.
            raise ReportRuntimeError(
                f"{root}: shallow git checkout; report generation needs full history "
                "(actions/checkout fetch-depth: 0, or git fetch --unshallow)"
            )
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


def _manifest_revisions(procedure: str, root: Path, error_type: type[E]) -> list[dict[str, Any]]:
    manifest_path = root / "site/reports/procedures/manifest.json"
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        procedures = manifest["procedures"]
        entry = next(item for item in procedures if item.get("procedure") == procedure)
        revisions = entry["revisions"]
    except (OSError, UnicodeError, json.JSONDecodeError, KeyError, StopIteration, TypeError) as error:
        raise error_type(f"unable to resolve procedure {procedure} from {manifest_path}: {error}") from error
    if not isinstance(revisions, list):
        raise error_type(f"procedure {procedure} has a malformed revision list")
    return revisions


def _page_for(procedure: str, selected: dict[str, Any], root: Path, error_type: type[E]) -> ProcedurePage:
    selected_revision = selected.get("rev")
    selected_html = selected.get("html")
    if not isinstance(selected_revision, str) or not isinstance(selected_html, str):
        raise error_type(f"procedure {procedure} has a malformed revision entry")
    if not (root / "site/reports" / selected_html).is_file():
        raise error_type(f"procedure {procedure} page does not exist: {selected_html}")
    return ProcedurePage(revision=selected_revision, html=selected_html)


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
    revisions = _manifest_revisions(procedure, checkout, error_type)
    try:
        selected = resolve_procedure_revision(
            revisions, revision, root=checkout, generated_at=generated_at,
        )
    except ReportRuntimeError as error:
        raise error_type(str(error)) from error
    if selected is None:
        raise error_type(f"procedure {procedure} has no page for revision {revision}")
    return _page_for(procedure, selected, checkout, error_type)


def select_procedure_page(
    procedure: str,
    revision: str | None,
    *,
    root: Path,
    generated_at: str,
    error_type: type[E] = ReportRuntimeError,  # type: ignore[assignment]
) -> tuple[ProcedurePage, bool]:
    """The procedure page a run at ``revision`` executed, and whether that choice was inferred.

    An exact manifest match or the newest Git ancestor is proven.  Without a
    source revision, or when no listed revision precedes it, the newest
    revision already in effect on the run date is selected and reported as
    inferred.  The report index and every report renderer share this one
    decision so a page and its index row never disagree.
    """
    revisions = _manifest_revisions(procedure, root, error_type)
    selected = None
    if revision is not None:
        if not GIT_REVISION.fullmatch(revision):
            raise error_type(f"invalid source revision for procedure {procedure}: {revision!r}")
        try:
            selected = resolve_procedure_revision(revisions, revision, root=root, generated_at=generated_at)
        except ReportRuntimeError as error:
            raise error_type(str(error)) from error
    inferred = selected is None
    if inferred:
        cutoff = generated_at[:10]
        dated = [item for item in revisions if isinstance(item.get("date"), str) and item["date"] <= cutoff]
        selected = max(dated, key=lambda item: item["date"], default=None)
    if selected is None:
        raise error_type(
            f"procedure {procedure} has no page for revision {revision or 'none'} (run date {generated_at})"
        )
    return _page_for(procedure, selected, root, error_type), inferred


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
