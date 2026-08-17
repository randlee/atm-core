#!/usr/bin/env python3
"""Validate raw triage findings before graph-orchestration scoping.

``query_runner.py`` intentionally drops findings that do not point at a
declared sprint.  That is correct for cursor resolution, but it also means
invalid findings can disappear before they are reported.  This command parses
the raw findings directory first and emits one ``#error``/``#warning`` line
per missing (or invalid) field.

Usage::

    validate-findings.py \\
      --findings-dir .triage/phase-AI/findings \\
      [--structure .sprints/AICH/structure.ttl] \\
      [--events .sprints/AICH/events.ttl]

Exit status is non-zero when at least one ``#error`` is found.  Warnings do
not fail the command.  ``--max-results`` truncates printed detail without
changing validation or the exit status.
"""

import argparse
import json
import re
import sys
from collections import Counter
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Literal, Union

try:
    from rdflib import Graph, URIRef, Namespace
    from rdflib.namespace import RDF
except ImportError:
    print("ERROR: rdflib not installed. Run: pip3 install rdflib", file=sys.stderr)
    raise SystemExit(1)


TRIAGE_BASE = "urn:atm:triage:"
TRIAGE = Namespace(TRIAGE_BASE)
SCRIPT_DIR = Path(__file__).resolve().parent
_INVALID_REPOSITORY_PATH = re.compile(
    r"(^/|^[A-Za-z]:[/\\]|(^|[/\\])\.\.(?:[/\\]|$))"
)


@dataclass(frozen=True)
class ValidationSummary:
    """Counts produced by a validation run.

    The counts describe the input that was actually selected.  In particular,
    ``findings`` is the count after ``--finding-id-regex`` filtering.
    """

    files: int = 0
    findings: int = 0
    errors: int = 0
    warnings: int = 0
    scoped: bool = False


@dataclass(frozen=True)
class ValidationPass:
    """The validator ran successfully and found no error-level diagnostics."""

    kind: Literal["validation:pass"] = field(
        default="validation:pass", init=False
    )
    diagnostics: tuple[str, ...] = ()
    summary: ValidationSummary = field(default_factory=ValidationSummary)


@dataclass(frozen=True)
class ValidationFail:
    """The validator ran successfully, but the data failed validation."""

    kind: Literal["validation:fail"] = field(
        default="validation:fail", init=False
    )
    diagnostics: tuple[str, ...] = ()
    summary: ValidationSummary = field(default_factory=ValidationSummary)


@dataclass(frozen=True)
class ValidationError:
    """The validator could not complete (bad input, query, or configuration)."""

    kind: Literal["error"] = field(default="error", init=False)
    message: str = ""
    diagnostics: tuple[str, ...] = ()
    summary: ValidationSummary | None = None


ValidationResult = Union[ValidationPass, ValidationFail, ValidationError]


def _parse_file(path: Path, graph: Graph, *, errors: list[str]) -> dict:
    """Parse one Turtle file, recording its Finding subjects by URI."""
    parsed = Graph()
    try:
        parsed.parse(str(path), format="turtle")
    except Exception as exc:  # noqa: BLE001 - report all bad files together
        errors.append(f"#error: {path}: malformed Turtle ({exc})")
        return {}

    graph += parsed
    return {
        str(finding): path
        for finding in parsed.subjects(RDF.type, TRIAGE.Finding)
    }


def _is_invalid_persisted_path(value: object) -> bool:
    """Return whether a persisted path is absolute or escapes its repository."""

    return _INVALID_REPOSITORY_PATH.search(str(value)) is not None


def _load_graph(
    findings_dir: Path,
    structure: Path | None,
    events: Path | None,
    finding_id_pattern: re.Pattern[str] | None,
) -> tuple[
    Graph,
    dict[str, Path],
    set[URIRef],
    list[str],
    int,
    list[str],
]:
    graph = Graph()
    finding_files: dict[str, Path] = {}
    diagnostics: list[str] = []
    path_diagnostics: list[str] = []

    known_sprints: set[URIRef] = set()
    for path in (structure, events):
        if path is None:
            continue
        if not path.exists():
            diagnostics.append(f"#error: {path}: input file does not exist")
            continue
        try:
            graph.parse(str(path), format="turtle")
        except Exception as exc:  # noqa: BLE001 - report malformed input
            diagnostics.append(f"#error: {path}: malformed Turtle ({exc})")
    known_sprints.update(graph.subjects(RDF.type, TRIAGE.Sprint))

    if not findings_dir.exists():
        diagnostics.append(f"#error: {findings_dir}: findings directory does not exist")
        return graph, finding_files, known_sprints, diagnostics, 0, path_diagnostics
    if not findings_dir.is_dir():
        diagnostics.append(f"#error: {findings_dir}: findings path is not a directory")
        return graph, finding_files, known_sprints, diagnostics, 0, path_diagnostics

    parsed_files = 0
    finding_graph = Graph()
    for path in sorted(findings_dir.glob("*.ttl")):
        parsed = Graph()
        try:
            parsed.parse(str(path), format="turtle")
        except Exception as exc:  # noqa: BLE001 - report all bad files together
            diagnostics.append(f"#error: {path}: malformed Turtle ({exc})")
            continue
        parsed_files += 1
        file_findings = list(parsed.subjects(RDF.type, TRIAGE.Finding))
        selected = {
            finding
            for finding in file_findings
            if finding_id_pattern is None
            or any(
                finding_id_pattern.search(str(value))
                for value in parsed.objects(finding, TRIAGE.findingId)
            )
        }
        all_linked_records: set[object] = set()
        for finding in file_findings:
            for occurrence in parsed.objects(finding, TRIAGE.hasOccurrence):
                all_linked_records.add(occurrence)
                all_linked_records.update(
                    parsed.objects(occurrence, TRIAGE.occursIn)
                )
        for finding in selected:
            finding_files[str(finding)] = path
            linked_records: set[object] = set()
            for triple in parsed.triples((finding, None, None)):
                finding_graph.add(triple)
            # Include occurrence/worktree metadata reachable from this
            # finding. The validator's query intentionally scopes findings by
            # ID, but path invariants live on the linked records rather than
            # on the Finding subject itself.
            for occurrence in parsed.objects(finding, TRIAGE.hasOccurrence):
                linked_records.add(occurrence)
                for triple in parsed.triples((occurrence, None, None)):
                    finding_graph.add(triple)
                for worktree in parsed.objects(occurrence, TRIAGE.occursIn):
                    linked_records.add(worktree)
                    for triple in parsed.triples((worktree, None, None)):
                        finding_graph.add(triple)

            # The SPARQL query validates paths reachable through the canonical
            # Finding -> Occurrence -> WorktreeSnapshot edges above. Keep a
            # parse-time check for legacy/unlinked records in the same selected
            # TTL file so absolute paths cannot evade the gate.
            for subject, predicate, value in parsed:
                if predicate not in (TRIAGE.file, TRIAGE.path):
                    continue
                if (
                    subject in linked_records
                    or subject in all_linked_records
                    or not _is_invalid_persisted_path(value)
                ):
                    continue
                field_name = (
                    "triage:file" if predicate == TRIAGE.file else "triage:path"
                )
                path_diagnostics.append(
                    f"#error: {path}: {subject} invalid {field_name} — "
                    "persisted path must be repository-relative"
                )

    for triple in finding_graph:
        graph.add(triple)
    return (
        graph,
        finding_files,
        known_sprints,
        diagnostics,
        parsed_files,
        path_diagnostics,
    )


def _run_query(graph: Graph, script_dir: Path) -> list:
    query_path = script_dir / "validate-findings.sparql"
    try:
        return list(graph.query(query_path.read_text()))
    except Exception as exc:  # noqa: BLE001 - present an actionable diagnostic
        raise RuntimeError(f"{query_path}: SPARQL query failed ({exc})") from exc


def _diagnostic_line(row, finding_files: dict[str, Path]) -> str:
    level, finding, field, detail = (str(value) for value in row)
    source = finding_files.get(finding)
    location = f"{source}: " if source else ""
    # Most query rows represent an absent predicate.  The wrapper also adds a
    # row when ``foundIn`` is present but points at an undeclared sprint; call
    # that invalid rather than claiming the field is missing.
    verb = (
        "invalid"
        if "undeclared sprint" in detail
        or field in {"triage:file", "triage:path"}
        else "missing"
    )
    return f"{level}: {location}{finding} {verb} {field} — {detail}"


def _summary(
    files: int,
    findings: int,
    diagnostics: list[str] | tuple[str, ...],
    *,
    scoped: bool = False,
) -> ValidationSummary:
    counts = Counter(line.split(":", 1)[0] for line in diagnostics)
    return ValidationSummary(
        files=files,
        findings=findings,
        errors=counts.get("#error", 0),
        warnings=counts.get("#warning", 0),
        scoped=scoped,
    )


def run_validation(
    *,
    findings_dir: Path,
    structure: Path | None = None,
    events: Path | None = None,
    finding_id_regex: str | None = None,
    script_dir: Path = SCRIPT_DIR,
) -> ValidationResult:
    """Run the validator and return a discriminated result.

    ``ValidationPass`` and ``ValidationFail`` both mean the command completed
    normally.  A ``ValidationFail`` is the expected result when finding data
    has ``#error`` diagnostics; it is not an exception.  ``ValidationError``
    is reserved for operational failures such as malformed Turtle, missing
    paths, an invalid regex, or a broken SPARQL query.
    """

    try:
        if not isinstance(findings_dir, Path):
            findings_dir = Path(findings_dir)
        if structure is not None and not isinstance(structure, Path):
            structure = Path(structure)
        if events is not None and not isinstance(events, Path):
            events = Path(events)
        if not isinstance(script_dir, Path):
            script_dir = Path(script_dir)

        finding_id_pattern = (
            re.compile(finding_id_regex) if finding_id_regex else None
        )
    except (OSError, re.error, TypeError, ValueError) as exc:
        return ValidationError(message=f"invalid validator configuration: {exc}")

    try:
        (
            graph,
            finding_files,
            known_sprints,
            input_diagnostics,
            parsed_files,
            path_diagnostics,
        ) = _load_graph(findings_dir, structure, events, finding_id_pattern)
        # Input diagnostics are operational errors, not validation findings:
        # returning an Error keeps malformed files from being mistaken for a
        # successful run that merely found invalid records.
        if input_diagnostics:
            return ValidationError(
                message="validator input could not be loaded",
                diagnostics=tuple(input_diagnostics),
                summary=_summary(
                    parsed_files,
                    len(finding_files),
                    input_diagnostics,
                    scoped=bool(finding_id_pattern),
                ),
            )

        rows = _run_query(graph, script_dir)

        # A foundIn value that points outside the supplied phase graph is an
        # error, not merely an out-of-scope finding.  Without this check, a
        # typo would look identical to an intentionally unscoped record.
        # When a structure/events graph was supplied, every foundIn target
        # must resolve to a declared sprint.  Do not guard this on
        # ``known_sprints`` being non-empty: an empty (but valid) phase graph
        # is still authoritative, and otherwise every finding would silently
        # pass as if no scope had been requested.
        if structure is not None or events is not None:
            for finding in sorted(graph.subjects(RDF.type, TRIAGE.Finding), key=str):
                for sprint in graph.objects(finding, TRIAGE.foundIn):
                    if sprint not in known_sprints:
                        rows.append(
                            (
                                "#error",
                                finding,
                                "triage:foundIn",
                                "finding references an undeclared sprint",
                            )
                        )

        diagnostics = tuple(
            [_diagnostic_line(row, finding_files) for row in rows]
            + path_diagnostics
        )
        summary = _summary(
            parsed_files,
            len(finding_files),
            diagnostics,
            scoped=bool(finding_id_pattern),
        )
        if summary.errors:
            return ValidationFail(diagnostics=diagnostics, summary=summary)
        return ValidationPass(diagnostics=diagnostics, summary=summary)
    except Exception as exc:  # noqa: BLE001 - API boundary must not leak errors
        return ValidationError(
            message=f"validator execution failed: {type(exc).__name__}: {exc}"
        )


def _result_payload(result: ValidationResult) -> dict:
    """Convert a result union member to a stable JSON-compatible object."""

    payload = asdict(result)
    payload["diagnostics"] = list(result.diagnostics)
    if result.summary is not None:
        payload["summary"] = asdict(result.summary)
    return payload


def _print_result(
    result: ValidationResult,
    *,
    max_results: int = 0,
    as_json: bool = False,
) -> None:
    # ``main`` turns a negative value into an Error result.  Clamp it here so
    # error rendering never accidentally uses Python's negative slicing.
    if max_results < 0:
        max_results = 0
    diagnostics = list(result.diagnostics)
    limit = max_results or len(diagnostics)
    shown = diagnostics[:limit]
    suppressed = len(diagnostics) - len(shown)

    if as_json:
        payload = _result_payload(result)
        payload["diagnostics"] = shown
        payload["suppressed"] = suppressed
        print(json.dumps(payload, sort_keys=True))
        return

    if isinstance(result, ValidationError):
        print(f"error: {result.message}")
    else:
        summary = result.summary
        scope = "selected " if summary.scoped else ""
        print(
            f"validated {summary.files} file(s), {scope}{summary.findings} finding(s): "
            f"{summary.errors} error(s), {summary.warnings} warning(s)"
        )
    for line in shown:
        print(line)
    if suppressed:
        print(
            f"… {suppressed} diagnostic line(s) truncated; "
            "use --max-results 0 for all"
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--findings-dir", type=Path, required=True)
    parser.add_argument("--structure", type=Path)
    parser.add_argument("--events", type=Path)
    parser.add_argument(
        "--finding-id-regex",
        help="validate only findings whose triage:findingId matches this regex",
    )
    parser.add_argument(
        "--script-dir",
        type=Path,
        default=SCRIPT_DIR,
        help="directory containing validate-findings.sparql (default: script directory)",
    )
    parser.add_argument(
        "--max-results",
        type=int,
        default=0,
        help="print at most N detail lines (0 means all; exit status is unaffected)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="emit the discriminated result as one JSON object",
    )
    args = parser.parse_args(argv)
    if args.max_results < 0:
        # Keep this an Error result instead of argparse's SystemExit so callers
        # using the API and callers using the CLI observe the same contract.
        result: ValidationResult = ValidationError(
            message="invalid validator configuration: --max-results must be >= 0"
        )
    else:
        result = run_validation(
            findings_dir=args.findings_dir,
            structure=args.structure,
            events=args.events,
            finding_id_regex=args.finding_id_regex,
            script_dir=args.script_dir,
        )
    _print_result(result, max_results=args.max_results, as_json=args.json)
    if result.kind == "validation:pass":
        return 0
    if result.kind == "validation:fail":
        return 1
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
