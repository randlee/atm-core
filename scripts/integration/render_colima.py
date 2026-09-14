#!/usr/bin/env python3
"""Render one colima integration run from its step payloads.

A run is one directory ``site/reports/integration/colima/<run>/`` holding one
``steps/NN-<name>/`` directory per step.  Each step directory carries the step
runner's payload byte-for-byte as ``step.json`` (plus any text sidecars the
runner wrote) and gets one self-contained XHTML ``panel.xhtml``.  The run gets
``integration.json`` (the aggregate verdict), ``index.html`` (the panels played
in order) and a sibling ``<run>.envelope.json`` the report index discovers.

Every artifact here is derived from the payloads on disk; payloads are never
edited.  The live driver (BB.8.2) and the historical backfill use this one
code path so the pages are the same shape whichever wrote them.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import html
import json
from pathlib import Path
import re
import shutil
import sys
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.report_runtime import ProcedurePage, ReportRuntimeError, compose, select_procedure_page  # noqa: E402

REPORTS = Path("site/reports")
TEMPLATES = Path("templates/integration-report")
# The colima fixture the sequence runs in; the container name inside it is recorded per step.
HOST_LABEL = "hermes-testbed"
SEQUENCE_PROCEDURE = "colima-integration"
STEP_DIR = re.compile(r"^(\d{2})-([a-z][a-z0-9-]*)$")
# Step name -> the procedure that runs it.  The payload's ``feature`` names the
# historical runner; the step name is what the run directory shows.
STEP_PROCEDURES = {
    "hermes-skills": "colima-hermes-skills",
    "task-start": "colima-task-start",
    "assignment": "colima-assignment",
    "prompt-handoffs": "colima-prompt-handoffs",
}
FEATURE_STEPS = {
    "colima-hermes-skills": "hermes-skills",
    "bb4-task-start": "task-start",
    "bb5-assignment": "assignment",
    "bb6-prompt-handoffs": "prompt-handoffs",
}
COMMAND_KEYS = frozenset({"command", "exit_code", "stdout", "stderr"})
# Payload keys shown in the panel header rather than as sections.
HEADER_KEYS = frozenset({
    "feature", "generated_at", "source_revision", "container", "image", "image_sha256",
    "container_image", "atm_version", "status", "cases", "host", "platform", "run_id",
})
CASE_HEADER_KEYS = frozenset({"name", "status", "detail"})
VERDICTS = frozenset({"PASS", "FAIL"})


class RenderError(RuntimeError):
    """A run directory or payload cannot be rendered."""


@dataclass(frozen=True)
class Step:
    order: int
    name: str
    directory: Path
    payload: dict[str, Any]

    @property
    def procedure(self) -> str:
        return STEP_PROCEDURES[self.name]

    @property
    def status(self) -> str:
        return str(self.payload["status"])

    @property
    def source_revision(self) -> str | None:
        value = self.payload.get("source_revision")
        return value if isinstance(value, str) else None

    @property
    def generated_at(self) -> str:
        value = self.payload.get("generated_at")
        if isinstance(value, str):
            return value
        run_id = self.payload.get("run_id")
        match = re.fullmatch(r"(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z", str(run_id))
        if match is None:
            raise RenderError(f"{self.directory}: payload has neither generated_at nor a run_id timestamp")
        year, month, day, hour, minute, second = match.groups()
        return f"{year}-{month}-{day}T{hour}:{minute}:{second}Z"

    @property
    def container(self) -> str:
        value = self.payload.get("container") or self.payload.get("host")
        return str(value) if value else "unknown"

    @property
    def image(self) -> dict[str, Any]:
        image = self.payload.get("image")
        if isinstance(image, dict):
            tags = image.get("image_repo_tags")
            return {
                "tag": image.get("image_tag") or (tags[0] if isinstance(tags, list) and tags else None),
                "id": image.get("image_id"),
                "sha256": image.get("image_sha256"),
                "architecture": image.get("image_architecture"),
            }
        digest = self.payload.get("image_sha256") or self.payload.get("container_image")
        return {"tag": None, "id": None, "sha256": digest if isinstance(digest, str) else None, "architecture": None}

    @property
    def atm_version(self) -> str | None:
        value = self.payload.get("atm_version")
        if isinstance(value, dict):
            return str(value.get("stdout", "")).strip() or None
        return str(value).strip() if isinstance(value, str) else None


# --- payload to panel -------------------------------------------------------

def _is_command(value: Any) -> bool:
    return isinstance(value, dict) and COMMAND_KEYS <= value.keys()


def _commands_table(commands: list[dict[str, Any]]) -> str:
    rows = []
    for item in commands:
        stderr = str(item.get("stderr", ""))
        rows.append(
            "<tr>"
            f"<td><code>{html.escape(str(item.get('command', '')))}</code></td>"
            f"<td>{html.escape(str(item.get('exit_code', '')))}</td>"
            f"<td><pre>{html.escape(str(item.get('stdout', '')))}</pre>"
            + (f"<h4>stderr</h4><pre>{html.escape(stderr)}</pre>" if stderr.strip() else "")
            + "</td></tr>"
        )
    return "<table><thead><tr><th>command</th><th>exit</th><th>stdout</th></tr></thead><tbody>" + "".join(rows) + "</tbody></table>"


def _is_scalar(value: Any) -> bool:
    """A value that reads as one line: number, flag, short single-line text, or a short list of those."""
    if isinstance(value, str):
        return "\n" not in value and len(value) <= 160
    if isinstance(value, list):
        return len(value) <= 8 and all(_is_scalar(item) and item is not None and not isinstance(item, list) for item in value)
    return value is None or isinstance(value, (int, float, bool))


def _scalar_text(value: Any) -> str:
    if isinstance(value, list):
        return ", ".join(str(item) for item in value)
    if isinstance(value, bool):
        return "yes" if value else "no"
    return "—" if value is None else str(value)


def _facts_table(facts: dict[str, Any]) -> str:
    rows = "".join(
        f"<tr><th>{html.escape(str(key))}</th><td>{html.escape(_scalar_text(value))}</td></tr>"
        for key, value in facts.items()
    )
    return f'<table class="facts"><tbody>{rows}</tbody></table>' if rows else ""


def _value_html(value: Any) -> str:
    if _is_command(value):
        return _commands_table([value])
    if isinstance(value, list) and value and all(_is_command(item) for item in value):
        return _commands_table(value)
    if isinstance(value, dict) and value and all(_is_command(item) for item in value.values()):
        return "".join(f"<h4>{html.escape(str(key))}</h4>{_commands_table([item])}" for key, item in value.items())
    if _is_scalar(value):
        return f"<p>{html.escape(_scalar_text(value))}</p>"
    if isinstance(value, str):
        return f"<pre>{html.escape(value)}</pre>"
    return f"<pre>{html.escape(json.dumps(value, indent=2, sort_keys=True))}</pre>"


def _verdict_html(status: Any) -> str:
    text = str(status)
    tone = text.lower() if text in VERDICTS else "other"
    return f'<span class="verdict {tone}">{html.escape(text)}</span>'


def _case_html(number: int, case: dict[str, Any]) -> str:
    """One case: heading with verdict, detail line, scalar facts in one table, then each structured field."""
    parts = [f"<h3>{number}. {html.escape(str(case.get('name', 'unnamed')))} {_verdict_html(case.get('status', '?'))}</h3>"]
    detail = case.get("detail")
    if detail:
        parts.append(f"<p>{html.escape(str(detail))}</p>")
    facts = {key: value for key, value in case.items() if key not in CASE_HEADER_KEYS and _is_scalar(value)}
    parts.append(_facts_table(facts))
    for key, value in case.items():
        if key in CASE_HEADER_KEYS or key in facts:
            continue
        parts.append(f"<h4>{html.escape(str(key))}</h4>{_value_html(value)}")
    return "".join(parts)


def _sidecar_sections(directory: Path) -> str:
    """Runner-written text beside the payload (skill reports, result line, doctor output)."""
    parts = []
    for path in sorted(directory.iterdir()):
        if path.name in {"step.json", "panel.xhtml"} or not path.is_file():
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeError:
            continue
        parts.append(f"<h2>{html.escape(path.name)}</h2><pre>{html.escape(text)}</pre>")
    return "".join(parts)


def panel_body(step: Step) -> str:
    cases = step.payload.get("cases")
    if not isinstance(cases, list):
        raise RenderError(f"{step.directory}: payload has no cases list")
    parts = ["<h2>Cases</h2>"]
    parts.extend(_case_html(number, case) for number, case in enumerate(cases, start=1) if isinstance(case, dict))
    for key, value in step.payload.items():
        if key in HEADER_KEYS:
            continue
        parts.append(f"<h2>{html.escape(str(key))}</h2>{_value_html(value)}")
    parts.append(_sidecar_sections(step.directory))
    return "".join(parts)


# --- run directory ----------------------------------------------------------

def _load_payload(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise RenderError(f"{path}: unreadable payload: {error}") from error
    if not isinstance(payload, dict) or payload.get("status") not in VERDICTS:
        raise RenderError(f"{path}: payload must be an object with a PASS/FAIL status")
    return payload


def read_steps(run_dir: Path) -> list[Step]:
    steps_dir = run_dir / "steps"
    steps: list[Step] = []
    for directory in sorted(steps_dir.iterdir()) if steps_dir.is_dir() else []:
        match = STEP_DIR.fullmatch(directory.name)
        if match is None or not directory.is_dir():
            raise RenderError(f"{directory}: step directories are named NN-<step>")
        order, name = int(match.group(1)), match.group(2)
        if name not in STEP_PROCEDURES:
            raise RenderError(f"{directory}: unknown step {name!r}; known: {', '.join(sorted(STEP_PROCEDURES))}")
        steps.append(Step(order, name, directory, _load_payload(directory / "step.json")))
    if not steps:
        raise RenderError(f"{run_dir}: no steps to render")
    if [step.order for step in steps] != list(range(1, len(steps) + 1)):
        raise RenderError(f"{run_dir}: step numbers must run 01..NN without gaps")
    return steps


def _procedure(step: Step, root: Path) -> tuple[ProcedurePage, bool]:
    try:
        return select_procedure_page(step.procedure, step.source_revision, root=root, generated_at=step.generated_at)
    except ReportRuntimeError as error:
        raise RenderError(f"{step.directory}: {error}") from error


def _up(from_page: Path, reports_root: Path) -> str:
    return "../" * len(from_page.parent.relative_to(reports_root).parts)


def render_panel(step: Step, page: ProcedurePage, *, root: Path) -> None:
    panel = step.directory / "panel.xhtml"
    variables = {
        "title": f"{step.name} · step {step.order:02d} · {step.status}",
        "step_label": f"Step {step.order:02d} · {step.name}",
        "status": step.status,
        "generated_at": step.generated_at,
        "procedure_label": f"{step.procedure} @ {page.revision[:8]}",
        "procedure_href": _up(panel, root / REPORTS) + page.html,
        "container": step.container,
        "image_digest": step.image["sha256"] or "unknown",
        "atm_version": step.atm_version or "unknown",
        "body_html": panel_body(step),
    }
    compose(root / TEMPLATES / "step-panel.xhtml.j2", variables, panel, root=root, error_type=RenderError)


def _step_record(step: Step, page: ProcedurePage, inferred: bool, run_dir: Path) -> dict[str, Any]:
    relative = step.directory.relative_to(run_dir).as_posix()
    return {
        "order": step.order,
        "name": step.name,
        "procedure": step.procedure,
        "procedure_revision": page.revision,
        "procedure_revision_inferred": inferred,
        "status": step.status,
        "generated_at": step.generated_at,
        "container": step.container,
        "payload": f"{relative}/step.json",
        "panel": f"{relative}/panel.xhtml",
    }


def _single(values: set[Any], what: str, run_dir: Path) -> Any:
    if len(values) > 1:
        raise RenderError(f"{run_dir}: steps disagree on {what}: {sorted(map(str, values))}")
    return next(iter(values), None)


def aggregate(
    steps: list[Step],
    records: list[dict[str, Any]],
    run_dir: Path,
    run_procedure: str,
    run_page: ProcedurePage,
    run_page_inferred: bool,
) -> dict[str, Any]:
    image = next((step.image for step in steps if step.image["sha256"]), steps[0].image)
    return {
        "schema_version": 1,
        "generated_at": max(step.generated_at for step in steps),
        "source_revision": _single({step.source_revision for step in steps}, "source_revision", run_dir),
        "image": image,
        "container": _single({step.container for step in steps}, "container", run_dir),
        "atm_version": _single({step.atm_version for step in steps if step.atm_version}, "atm_version", run_dir),
        "procedure": run_procedure,
        "procedure_revision": run_page.revision,
        "procedure_revision_inferred": run_page_inferred,
        "status": "PASS" if all(step.status == "PASS" for step in steps) else "FAIL",
        "steps": records,
    }


def _sections_html(summary: dict[str, Any]) -> str:
    return "".join(
        "<section>"
        f'<h2>Step {record["order"]:02d} · {html.escape(record["name"])}'
        f' <span class="verdict {html.escape(record["status"].lower())}">{html.escape(record["status"])}</span>'
        f'<a href="{html.escape(record["panel"], quote=True)}">open panel</a></h2>'
        f'<iframe src="{html.escape(record["panel"], quote=True)}" title="{html.escape(record["name"], quote=True)}"></iframe>'
        "</section>"
        for record in summary["steps"]
    )


def render_run_page(summary: dict[str, Any], run_dir: Path, page: ProcedurePage, *, root: Path) -> None:
    index = run_dir / "index.html"
    variables = {
        "title": f"Colima integration · {run_dir.name}",
        "generated_at": summary["generated_at"],
        "status": summary["status"],
        "source_revision": summary["source_revision"] or "unknown",
        "image_digest": summary["image"]["sha256"] or "unknown",
        "container": summary["container"],
        "procedure_label": f"{summary['procedure']} @ {summary['procedure_revision'][:8]}",
        "procedure_href": _up(index, root / REPORTS) + page.html,
        "procedure_note": " (inferred from run date)" if summary["procedure_revision_inferred"] else "",
        "sections_html": _sections_html(summary),
    }
    compose(root / TEMPLATES / "run.html.j2", variables, index, root=root, error_type=RenderError)


def _envelope(summary: dict[str, Any], run_dir: Path, reports_root: Path) -> dict[str, Any]:
    envelope = {
        "schema_version": 1,
        "report_type": "integration",
        "generated_at": summary["generated_at"],
        "host_label": HOST_LABEL,
        "report_html": (run_dir / "index.html").relative_to(reports_root).as_posix(),
        "procedure": summary["procedure"],
        "status": summary["status"],
    }
    if summary["source_revision"]:
        envelope["source_revision"] = summary["source_revision"]
    return envelope


def _write_json(path: Path, payload: dict[str, Any]) -> None:
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def render_run(run_dir: Path, *, root: Path = ROOT) -> dict[str, Any]:
    """Render every panel, the aggregate, the run page and the envelope for ``run_dir``."""
    root = root.resolve()
    run_dir = run_dir.resolve()
    reports_root = root / REPORTS
    try:
        run_dir.relative_to(reports_root)
    except ValueError as error:
        raise RenderError(f"{run_dir} is not under {reports_root}") from error
    steps = read_steps(run_dir)
    records = []
    pages: list[ProcedurePage] = []
    for step in steps:
        page, inferred = _procedure(step, root)
        render_panel(step, page, root=root)
        records.append(_step_record(step, page, inferred, run_dir))
        pages.append(page)
    if len({record["procedure"] for record in records}) == 1:
        run_procedure = records[0]["procedure"]
        run_page = pages[0]
        run_page_inferred = records[0]["procedure_revision_inferred"]
    else:
        run_procedure = SEQUENCE_PROCEDURE
        try:
            run_page, run_page_inferred = select_procedure_page(
                SEQUENCE_PROCEDURE,
                _single({step.source_revision for step in steps}, "source_revision", run_dir),
                root=root,
                generated_at=max(step.generated_at for step in steps),
            )
        except ReportRuntimeError as error:
            raise RenderError(f"{run_dir}: {error}") from error
    summary = aggregate(steps, records, run_dir, run_procedure, run_page, run_page_inferred)
    _write_json(run_dir / "integration.json", summary)
    render_run_page(summary, run_dir, run_page, root=root)
    _write_json(run_dir.parent / f"{run_dir.name}.envelope.json", _envelope(summary, run_dir, reports_root))
    return summary


def add_step(run_dir: Path, name: str, payload: Path, order: int | None = None) -> Path:
    """Place ``payload`` as the next step of ``run_dir`` (byte-for-byte) and return its directory."""
    if name not in STEP_PROCEDURES:
        raise RenderError(f"unknown step {name!r}; known: {', '.join(sorted(STEP_PROCEDURES))}")
    steps_dir = run_dir / "steps"
    existing = sorted(steps_dir.iterdir()) if steps_dir.is_dir() else []
    number = order if order is not None else len(existing) + 1
    directory = steps_dir / f"{number:02d}-{name}"
    target = directory / "step.json"
    if payload.resolve() != target.resolve():
        if target.exists():
            raise RenderError(f"{target} already exists")
        directory.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(payload, target)
    return directory


def step_for_feature(feature: str) -> str:
    try:
        return FEATURE_STEPS[feature]
    except KeyError as error:
        raise RenderError(f"no integration step for feature {feature!r}") from error


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Render a colima integration run from its step payloads.")
    parser.add_argument("--out", type=Path, required=True, help="run directory under site/reports/integration/colima/")
    parser.add_argument("--payload", type=Path, help="step payload to add before rendering (copied byte-for-byte)")
    parser.add_argument("--step", help="step name for --payload (default: from the payload's feature)")
    parser.add_argument("--order", type=int, help="step number for --payload (default: next)")
    parser.add_argument("--root", type=Path, default=ROOT, help="repository root")
    args = parser.parse_args(argv[1:])
    try:
        if args.payload is not None:
            name = args.step or step_for_feature(str(_load_payload(args.payload).get("feature")))
            add_step(args.out, name, args.payload, args.order)
        summary = render_run(args.out, root=args.root.resolve())
    except RenderError as error:
        print(f"render-colima: error: {error}", file=sys.stderr)
        return 1
    print(f"{args.out}: {summary['status']} ({len(summary['steps'])} step(s))")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
