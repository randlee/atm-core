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

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from pathlib import Path

try:
    from rdflib import Graph, URIRef, Namespace
    from rdflib.namespace import RDF
except ImportError:
    print("ERROR: rdflib not installed. Run: pip3 install rdflib", file=sys.stderr)
    raise SystemExit(1)


TRIAGE_BASE = "urn:atm:triage:"
TRIAGE = Namespace(TRIAGE_BASE)
SCRIPT_DIR = Path(__file__).resolve().parent


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


def _load_graph(
    findings_dir: Path,
    structure: Path | None,
    events: Path | None,
    finding_id_pattern: re.Pattern[str] | None,
) -> tuple[Graph, dict[str, Path], set[URIRef], list[str], int]:
    graph = Graph()
    finding_files: dict[str, Path] = {}
    diagnostics: list[str] = []

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
        return graph, finding_files, known_sprints, diagnostics
    if not findings_dir.is_dir():
        diagnostics.append(f"#error: {findings_dir}: findings path is not a directory")
        return graph, finding_files, known_sprints, diagnostics

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
        for finding in selected:
            finding_files[str(finding)] = path
            for triple in parsed.triples((finding, None, None)):
                finding_graph.add(triple)

    for triple in finding_graph:
        graph.add(triple)
    return graph, finding_files, known_sprints, diagnostics, parsed_files


def _run_query(graph: Graph, script_dir: Path) -> list:
    query_path = script_dir / "validate-findings.sparql"
    try:
        return list(graph.query(query_path.read_text()))
    except Exception as exc:  # noqa: BLE001 - present an actionable diagnostic
        print(f"#error: {query_path}: SPARQL query failed ({exc})", file=sys.stderr)
        raise SystemExit(1)


def _diagnostic_line(row, finding_files: dict[str, Path], known_sprints: set[URIRef]) -> str:
    level, finding, field, detail = (str(value) for value in row)
    source = finding_files.get(finding)
    location = f"{source}: " if source else ""
    return f"{level}: {location}{finding} missing {field} — {detail}"


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
    args = parser.parse_args(argv)
    if args.max_results < 0:
        parser.error("--max-results must be >= 0")

    try:
        finding_id_pattern = (
            re.compile(args.finding_id_regex) if args.finding_id_regex else None
        )
    except re.error as exc:
        parser.error(f"invalid --finding-id-regex: {exc}")

    graph, finding_files, known_sprints, diagnostics, parsed_files = _load_graph(
        args.findings_dir, args.structure, args.events, finding_id_pattern
    )
    rows = _run_query(graph, args.script_dir)

    # A foundIn value that points outside the supplied phase graph is an error,
    # not merely an out-of-scope finding.  Without this check, a typo would
    # look identical to an intentionally unscoped record.
    if known_sprints:
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

    detail_lines = [
        _diagnostic_line(row, finding_files, known_sprints)
        for row in rows
    ]
    diagnostics.extend(detail_lines)

    counts = Counter(line.split(":", 1)[0] for line in diagnostics)
    files = parsed_files
    findings = len(finding_files)
    scope = "selected " if finding_id_pattern else ""
    print(
        f"validated {files} file(s), {scope}{findings} finding(s): "
        f"{counts.get('#error', 0)} error(s), {counts.get('#warning', 0)} warning(s)"
    )

    limit = args.max_results or len(diagnostics)
    for line in diagnostics[:limit]:
        print(line)
    suppressed = len(diagnostics) - min(limit, len(diagnostics))
    if suppressed:
        print(f"… {suppressed} diagnostic line(s) truncated; use --max-results 0 for all")

    return 1 if counts.get("#error", 0) else 0


if __name__ == "__main__":
    raise SystemExit(main())
