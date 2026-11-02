# pyright: strict
"""Plan-scoped, live-bead execution DAG for sprint-report --dag."""
from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from graphlib import TopologicalSorter, CycleError
from html import escape
import json
from collections.abc import Sequence
from pathlib import Path
import re
import shutil
import subprocess
from string import Template
import sys
import xml.etree.ElementTree as ET

from sprint_index_common import IndexSprint, JsonValue, NormalizedIndex, is_json_object, is_object_list, run_json as _run_json, index_bead_pairs, validate_index
from sprint_qa import choose_round, qa_icon, qa_verdict, findings_summary
from phase_beads import phase_beads
from operation_models import (
    AtmTask, AtmTaskEvent, BeadRecord, is_atm_tasks, is_atm_task_events,
    is_bead_records, is_object, matches_typeddict,
)
from typing import NotRequired, TypedDict

RENDERER = Path(__file__).resolve().parents[1] / 'renderer'
NS = 'http://www.w3.org/2000/svg'
ET.register_namespace('', NS)


def run_json(repo: Path, *args: str) -> JsonValue:
    return _run_json(repo, *args)


def record_payload(repo: Path, key: str, *args: str) -> object:
    value = run_json(repo, *args)
    if is_json_object(value):
        value = value.get(key)
    return value


def run_bead_records(repo: Path, key: str, *args: str) -> list[BeadRecord]:
    value = record_payload(repo, key, *args)
    if not is_bead_records(value):
        raise RuntimeError(f"{' '.join(args)} did not return validated bd bead records")
    return value


def run_atm_tasks(repo: Path, *args: str) -> list[AtmTask]:
    value = record_payload(repo, "tasks", *args)
    if not is_atm_tasks(value):
        raise RuntimeError(f"{' '.join(args)} did not return validated ATM task records")
    return value


def run_atm_task_events(repo: Path, *args: str) -> list[AtmTaskEvent]:
    value = record_payload(repo, "events", *args)
    if not is_atm_task_events(value):
        raise RuntimeError(f"{' '.join(args)} did not return validated ATM task-event records")
    return value


class Graph(TypedDict):
    nodes: list[str]
    edges: list[list[str]]
    gate_pairs: dict[str, str]
    edge_meaning: str
    render_direction: str


class Snapshot(TypedDict):
    beads: dict[str, BeadRecord]
    tasks: dict[str, AtmTask]
    blocked: dict[str, list[str]]
    events: dict[str, list[AtmTaskEvent] | None]
    iterations: dict[str, int | None]
    errors: dict[str, str]
    captured_at: str


class NodeState(TypedDict):
    state: str
    sanity_iterations: int | str | None
    evidence: str
    qa: NotRequired["QaState"]


class QaState(TypedDict):
    icon: str
    findings: str | None
    verdict: str | None
    qa_beads: list[str]
    open_findings: NotRequired[list[str]]


def normalized_index(value: object) -> NormalizedIndex:
    validate_index(value)
    if not is_object(value):
        raise RuntimeError('phase index does not match the normalized index schema')
    root_bead_id = value.get('root_bead_id')
    if not isinstance(root_bead_id, str):
        raise RuntimeError('phase index does not match the normalized index schema')
    source_rows = value.get('sprints')
    if not is_object_list(source_rows):
        raise RuntimeError('phase index sprints must be a list')
    rows: list[IndexSprint] = []
    for source_row in source_rows:
        if not is_object(source_row):
            raise RuntimeError('phase index sprint must be an object')
        dev = source_row.get('dev_bead_id')
        sanity = source_row.get('sanity_bead_id')
        dependencies = source_row.get('depends_on_sanity_bead_ids', [])
        sprint = source_row.get('sprint_bead_id', dev)
        if (not isinstance(dev, str) or not isinstance(sanity, str) or not isinstance(sprint, str)
                or not is_object_list(dependencies) or not all(isinstance(item, str) for item in dependencies)):
            raise RuntimeError('phase index sprint does not match the normalized index schema')
        dependency_ids: list[str] = [item for item in dependencies if isinstance(item, str)]
        rows.append({'sprint_bead_id': sprint, 'dev_bead_id': dev, 'sanity_bead_id': sanity,
                     'depends_on_sanity_bead_ids': dependency_ids})
    return {'root_bead_id': root_bead_id, 'sprints': rows}


def bead_map(value: object) -> dict[str, BeadRecord]:
    if not is_object(value):
        raise RuntimeError('bead map must be a mapping')
    result: dict[str, BeadRecord] = {}
    for key, bead in value.items():
        if not matches_typeddict(bead, BeadRecord):
            raise RuntimeError('bead map contains an invalid record')
        result[key] = bead
    return result


def node_state_map(value: object) -> dict[str, NodeState]:
    if not is_object(value):
        raise ValueError('node state map must be a mapping')
    result: dict[str, NodeState] = {}
    for key, state in value.items():
        if not matches_typeddict(state, NodeState):
            raise ValueError('node state map contains an invalid record')
        result[key] = state
    return result


def qa_state_map(value: object) -> dict[str, QaState]:
    if not is_object(value):
        raise ValueError('QA state map must be a mapping')
    result: dict[str, QaState] = {}
    for key, state in value.items():
        if not matches_typeddict(state, QaState):
            raise ValueError('QA state map contains an invalid record')
        result[key] = state
    return result


def tag(name: str) -> str:
    return '{' + NS + '}' + name


def prerequisites(bead: BeadRecord) -> list[str]:
    targets: list[str] = []
    for edge in bead.get('dependencies', []):
        if edge.get('type', edge.get('dependency_type')) != 'blocks':
            continue
        target = edge.get('depends_on_id') or edge.get('id')
        if target:
            targets.append(target)
    return targets


def build_graph(index: object, beads: object) -> Graph:
    """Render only the declared phase graph; Beads contribute live state."""
    index = normalized_index(index)
    beads = bead_map(beads)
    devs = set(index_bead_pairs(index))
    missing = devs - beads.keys()
    if missing:
        raise RuntimeError('Missing planned beads: ' + ', '.join(sorted(missing)))
    pairs = index_bead_pairs(index)
    for dev in sorted(devs):
        gates = [key for key, bead in beads.items()
                 if 'stage:dev-sanity' in (bead.get('labels') or [])
                 and dev in prerequisites(bead)]
        if len(gates) != 1:
            raise RuntimeError(f'{dev}: expected one live sanity gate, found {len(gates)}')
        if pairs[dev] != gates[0]:
            raise RuntimeError(f'{dev}: indexed sanity bead {pairs[dev]} does not match live gate {gates[0]}')
    nodes = devs | set(pairs.values())
    edges: list[tuple[str, str]] = []
    for row in index['sprints']:
        dev, sanity = row['dev_bead_id'], row['sanity_bead_id']
        # The sanity gate waits on its dev bead.  Each listed prerequisite
        # sprint gates this dev directly through its own sanity bead.
        edges.append((sanity, dev))
        edges.extend((dev, prerequisite) for prerequisite in row.get('depends_on_sanity_bead_ids', []))
    edges = sorted(edges)
    try:
        tuple(TopologicalSorter({key: [dep for src, dep in edges if src == key]
                                 for key in nodes}).static_order())
    except CycleError as exc:
        raise RuntimeError(f'Bead blocking dependencies contain a cycle: {exc}') from exc
    return {'nodes': sorted(nodes), 'edges': [list(edge) for edge in edges],
            'gate_pairs': pairs, 'edge_meaning': 'source depends on target',
            'render_direction': 'prerequisite to dependent'}


def collect_state(repo: Path, graph: Graph, beads: dict[str, BeadRecord],
                  counts: dict[str, int | None], phase: str) -> Snapshot:
    errors: dict[str, str] = {}
    def history(key: str) -> tuple[str, list[AtmTaskEvent] | str]:
        try:
            return key, run_atm_task_events(repo, 'atm', 'task', 'events', key, '--all', '--json')
        except (OSError, RuntimeError, KeyError, subprocess.CalledProcessError) as exc:
            return key, str(exc)
    with ThreadPoolExecutor(max_workers=4) as pool:
        histories: dict[str, list[AtmTaskEvent] | str | None] = dict(pool.map(history, graph['nodes']))
    typed_histories: dict[str, list[AtmTaskEvent] | None] = {}
    for key, value in histories.items():
        if isinstance(value, list):
            typed_histories[key] = value
        else:
            errors[key] = value if isinstance(value, str) else "event history was not available"
            typed_histories[key] = None
    try:
        tasks = {t['task_id']: t for t in run_atm_tasks(repo, 'atm', 'task', 'list', '--all', '--json')
                 if t['task_id'] in graph['nodes']}
    except (OSError, RuntimeError, subprocess.CalledProcessError) as exc:
        tasks = {}
        errors['task_list'] = str(exc)
    blocked = {b['id']: b.get('blocked_by', [])
               for b in run_bead_records(repo, 'beads', 'bd', 'blocked', '-l', f'phase-{phase}',
                                 '--label-any', 'stage:dev,stage:dev-sanity', '--json')}
    return {'beads': dict(beads), 'tasks': tasks, 'blocked': blocked, 'events': typed_histories,
            'iterations': counts, 'errors': errors,
            'captured_at': datetime.now(timezone.utc).isoformat()}


def states(graph: Graph, snapshot: Snapshot, index: object) -> dict[str, NodeState]:
    beads, events = snapshot['beads'], snapshot['events']
    pairs = graph['gate_pairs']
    gates: dict[str, str] = {gate: dev for dev, gate in pairs.items()}
    def completed(key: str) -> list[AtmTaskEvent]:
        return [e for e in (events.get(key) or []) if e.get('event') == 'completed']
    def latest(key: str) -> str:
        return max((e.get('at', '') for e in completed(key)), default='')
    def open_findings(key: str) -> list[str]:
        dev = gates.get(key)
        return [b['id'] for b in beads.values() if b.get('status') != 'closed' and any(
            edge.get('type') in ('discovered-from', 'parent-child')
            and (edge.get('depends_on_id') == key or (
                edge.get('depends_on_id') == dev
                and any('sanity' in label for label in b.get('labels', []))))
            for edge in b.get('dependencies', []))]
    def verdict(key: str) -> tuple[str, str]:
        bead = beads[key]
        reason = bead.get('close_reason') or ''
        if 'overridden' in reason.lower():
            return 'override', reason
        task = snapshot['tasks'].get(key)
        task_state = task.get('state') if task else None
        history = events.get(key) or []
        history_state = history[-1].get('to_state') if history else None
        if task_state == 'active' or history_state == 'active' or bead.get('status') == 'in_progress':
            return 'active', 'Work in progress'
        if key in snapshot['blocked'] or bead.get('status') == 'blocked':
            return 'blocked', 'Blocked by unfinished prerequisites'
        if task_state in ('assigned', 'queued') or history_state == 'assigned':
            return 'waiting', 'Assigned; completion not recorded'
        if key in gates:
            findings = open_findings(key)
            if findings:
                return 'findings', 'Open sanity findings: ' + ', '.join(findings)
            meta = bead.get('metadata') or {}
            passed = str(meta.get('verdict', '')).upper() == 'PASS' or re.match(r'^PASS\b', reason, re.I)
            if bead.get('status') == 'closed' and passed:
                dev = gates[key]
                if events.get(key) is None or events.get(dev) is None or 'task_list' in snapshot.get('errors', {}):
                    return 'uncertain', reason + '; ATM history unavailable'
                if not latest(key) or not latest(dev):
                    return 'uncertain', reason + '; completion chronology unavailable'
                if latest(dev) > latest(key):
                    return 'uncertain', reason + '; PASS predates latest dev completion'
                return 'done', reason
            if bead.get('status') == 'closed' or re.match(r'^FAIL\b', reason, re.I):
                return 'findings', reason or 'No explicit sanity PASS'
        elif key in pairs and bead.get('status') == 'closed':
            if not completed(key) or 'task_list' in snapshot.get('errors', {}):
                return 'uncertain', reason + '; dev completion history unavailable'
            gate_state, detail = verdict(pairs[key])
            if gate_state == 'done':
                return 'done', reason + '; sanity PASS recorded'
            if gate_state in ('uncertain', 'findings'):
                return gate_state, reason + '; ' + detail
            return 'waiting', reason + '; awaiting sanity completion'
        return 'waiting', 'Not started / awaiting completion'
    result: dict[str, NodeState] = {}
    for key in graph['nodes']:
        state, detail = verdict(key)
        count = None
        if key in gates:
            logged = snapshot.get('iterations', {}).get(key)
            count = max(logged or 0, len(completed(key))) if logged is not None or events.get(key) is not None else '?'
        result[key] = {'state': state, 'sanity_iterations': count, 'evidence': detail}
    return result


def is_qa_of(bead: BeadRecord, dev_id: str) -> bool:
    """A QA round of dev_id: a stage:qa child of it (phase contract; bd keeps one edge type per pair) or a
    pre-contract round that `validates` it."""
    edges = bead.get('dependencies') or []
    if any(edge.get('type') == 'validates' and edge.get('depends_on_id') == dev_id for edge in edges):
        return True
    if 'stage:qa' not in (bead.get('labels') or []):
        return False
    metadata = bead.get('metadata') or {}
    return bead.get('parent') == dev_id or metadata.get('checked_bead') == dev_id or any(
        edge.get('type') == 'parent-child' and edge.get('depends_on_id') == dev_id for edge in edges)


def qa_states(graph: Graph, beads: object, index: object) -> dict[str, QaState]:
    """Share table QA semantics, counting open findings across every round."""
    beads = bead_map(beads)
    result: dict[str, QaState] = {}
    for key in graph['nodes']:
        rounds = [bead for bead in beads.values() if is_qa_of(bead, key)]
        selected = choose_round(rounds)
        round_ids = {bead['id'] for bead in rounds}
        findings = [bead for bead in beads.values() if any(
            edge.get('type') in ('discovered-from', 'parent-child')
            and edge.get('depends_on_id') in round_ids
            for edge in bead.get('dependencies') or [])]
        icon = qa_icon(selected, findings)
        counts = findings_summary(findings) if icon == '🚩' else None
        result[key] = {'icon': icon, 'findings': counts,
                       'verdict': qa_verdict(selected) if selected else None,
                       'qa_beads': sorted(round_ids),
                       'open_findings': sorted(b['id'] for b in findings if b.get('status') != 'closed')}
    return result


def dot_source(graph: Graph, phase: str) -> str:
    def quote(text: str) -> str:
        return json.dumps(text, ensure_ascii=False)
    phase = str(phase).removeprefix('phase-').upper()
    lines = ['digraph execution {',
             'graph [rankdir=LR, compound=true, bgcolor="white", pad="0.4", nodesep="0.22", ranksep="0.5", splines=spline, fontname="Arial", fontsize=20, labelloc=t, label=' + quote(f'Phase {phase} — execution order\nSprint completes → its gate passes → downstream sprint starts') + '];',
             'node [shape=box, style="rounded,filled", fontname="Arial", fontsize=13, margin="0.12,0.10", color="#9caec2", fillcolor="#edf3fb", fontcolor="#172b43"];',
             'edge [color="#647a94", penwidth=1.15, arrowsize=0.75];']
    pairs = graph['gate_pairs']
    def order(key: str) -> list[int | str]:
        return [int(part) if part.isdigit() else part for part in re.split(r'(\d+)', key)]
    for dev in sorted(pairs, key=order):
        gate = pairs[dev]
        lines.append(f'subgraph {quote("cluster_" + dev)} {{ label=""; style=invis;')
        lines.append(f'{quote(dev)} [label={quote(dev)}];')
        label = quote(dev + '\nsanity gate')
        lines.append(f'{quote(gate)} [label={label}, shape=hexagon, fillcolor="#e6f1e9", color="#a0b8a8"];')
        lines.append('}')
    for key in sorted(set(graph['nodes']) - set(pairs) - set(pairs.values())):
        lines.append(f'{quote(key)} [label={quote(key)}, shape=hexagon, fillcolor="#f0e9fa", color="#b6a7cd"];')
    lines.extend(f'{quote(dep)} -> {quote(src)};' for src, dep in graph['edges'])
    return '\n'.join(lines + ['}']) + '\n'


colors={'done':'#15803d','blocked':'#b91c1c','waiting':'#64748b','active':'#2563eb','uncertain':'#b45309','findings':'#b91c1c','override':'#7e22ce'}
symbols={'blocked':'!','waiting':'○','active':'↻','uncertain':'?','findings':'!','override':'↷'}
def badge(parent: ET.Element[str], x: float, y: float, state: str,
          count: int | str | None = None) -> None:
 w=38 if count is not None else 20
 g=ET.SubElement(parent,tag('g'),{'class':'state-badge','transform':f'translate({x-w/2:g} {y-9:g})'})
 ET.SubElement(g,tag('rect'),{'width':str(w),'height':'18','rx':'6','fill':'white','stroke':colors[state],'stroke-width':'1.2'})
 if state=='done':ET.SubElement(g,tag('path'),{'d':'M 5 9 L 8 12 L 14 5','fill':'none','stroke':colors[state],'stroke-width':'2.5','stroke-linecap':'round','stroke-linejoin':'round'})
 else:
  t=ET.SubElement(g,tag('text'),{'x':'10','y':'13','text-anchor':'middle','font-family':'Arial','font-size':'15','font-weight':'bold','fill':colors[state]});t.text=symbols[state]
 if count is not None:
  t=ET.SubElement(g,tag('text'),{'x':'27','y':'13','text-anchor':'middle','font-family':'Arial','font-size':'12','font-weight':'bold','fill':colors[state]});t.text=str(count)


def qa_badge(parent: ET.Element[str], x: float, y: float, qa: QaState) -> None:
    icon, counts = qa['icon'], qa['findings']
    if not icon:
        return
    if counts is not None:
        width = max(38, len(counts) * 7 + 12)
        group = ET.SubElement(parent, tag('g'), {'class': 'qa-badge',
            'transform': f'translate({x-width/2:g} {y-9:g})'})
        ET.SubElement(group, tag('rect'), {'width': str(width), 'height': '18',
            'rx': '6', 'fill': 'white', 'stroke': colors['findings'], 'stroke-width': '1.2'})
        text = ET.SubElement(group, tag('text'), {'x': str(width/2), 'y': '13',
            'text-anchor': 'middle', 'font-family': 'Arial', 'font-size': '12',
            'font-weight': 'bold', 'fill': colors['findings']})
        text.text = counts
    else:
        state = {'✅': 'done', '📥': 'waiting', '🌀': 'active'}[icon]
        badge(parent, x, y, state)
        group = parent[-1]
        group.set('class', 'qa-badge')
        if icon == '📥':
            text_element = group.find(tag('text'))
            if text_element is None:
                raise ValueError('QA badge has no text element')
            group.remove(text_element)
            ET.SubElement(group, tag('path'), {'d': 'M4 10V14H16V10 M10 3V10 M7 7L10 10L13 7',
                'fill': 'none', 'stroke': colors['waiting'], 'stroke-width': '1.5'})
    title = ET.SubElement(group, tag('title'))
    title_text = 'QA ' + (qa['verdict'] or icon)
    if counts is not None:
        title_text += ': ' + counts + ' open blocking:important:minor'
    if qa['qa_beads']:
        title_text += '; ' + ', '.join(qa['qa_beads'])
    title.text = title_text


def overlay(layout: str, icons: object, captured_at: str,
            qa: object | None = None) -> str:
    icons = node_state_map(icons)
    qa = qa_state_map(qa) if qa is not None else None
    root = ET.fromstring(layout)
    graph = root.find(tag('g'))
    if graph is None:
        raise ValueError('SVG layout has no graph element')
    # Put badges over edges, preserving every original node/edge coordinate.
    badges = ET.Element(tag('g'), {'class': 'state-badges'})
    for node in graph.findall(tag('g')):
        if node.get('class') != 'node':
            continue
        title = node.find(tag('title'))
        if title is None or title.text is None:
            raise ValueError('SVG node has no title')
        key = title.text
        icon = icons[key]
        path = node.find(tag('path'))
        if path is None or path.get('d') is None:
            raise ValueError(f'SVG node {key} has no path')
        path_data = path.get('d')
        assert path_data is not None
        numbers = [float(n) for n in re.findall(r'-?\d+(?:\.\d+)?', path_data)]
        y = min(numbers[1::2])
        label = node.find(tag('text'))
        if label is None or label.get('x') is None:
            raise ValueError(f'SVG node {key} has no label position')
        x_position = label.get('x')
        assert x_position is not None
        x = float(x_position)
        title_text = key + ' — ' + icon['evidence']
        if icon['sanity_iterations'] is not None:
            title_text += f"; completed sanity iterations: {icon['sanity_iterations']}"
        title.text = title_text
        badge(badges, x, y, icon['state'], icon['sanity_iterations'])
        if qa and key in qa:
            qa_badge(badges, x, max(numbers[1::2]), qa[key])
    graph.append(badges)
    view_box = root.get('viewBox')
    if view_box is None:
        raise ValueError('SVG layout has no viewBox')
    view = list(map(float, view_box.split()))
    root.set('viewBox', ' '.join(map(str, [*view[:3], view[3] + 88])))
    root.set('height', str(view[3] + 88) + 'pt')
    footer = ET.SubElement(root, tag('g'), {'class': 'state-legend', 'font-family': 'Arial', 'font-size': '14', 'fill': '#334155'})
    ET.SubElement(footer, tag('rect'), {'x': '0', 'y': str(view[3]), 'width': str(view[2]), 'height': '88', 'fill': 'white'})
    labels = [('done', 'Complete / sanity PASS'), ('waiting', 'Pending'),
              ('blocked', 'Blocked'), ('active', 'Active'), ('findings', 'Findings'),
              ('uncertain', 'Unconfirmed'), ('override', 'User override')]
    x = 40
    for state, label in labels:
        badge(footer, x, view[3] + 18, state)
        text = ET.SubElement(footer, tag('text'), {'x': str(x + 16), 'y': str(view[3] + 23)})
        text.text = label
        x += len(label) * 7 + 48
    text = ET.SubElement(footer, tag('text'), {'x': '30', 'y': str(view[3] + 52), 'font-size': '12'})
    text.text = ('Top = dev/sanity; gate number = completed sanity iterations; ? = unavailable. '
                 'Dev check requires sanity PASS. Snapshot: ' + captured_at)
    text = ET.SubElement(footer, tag('text'), {'x': '30', 'y': str(view[3] + 74), 'font-size': '12'})
    text.text = 'Bottom = QA: assigned / active / pass, or open blocking:important:minor findings after FAIL. Blank = undispatched.'
    # Allow a small graph to fit its legend; never move its nodes or arrows.
    root.set('viewBox', ' '.join(map(str, [view[0], view[1], max(view[2], x + 25), view[3] + 88])))
    root.set('width', str(max(view[2], x + 25)) + 'pt')
    return ET.tostring(root, encoding='unicode')


def render(mode: str, source: Path, target: Path) -> None:
    subprocess.run(['node', str(RENDERER / 'render.cjs'), mode, str(source), str(target)], check=True)


def html_view(svg: str, phase: str, root_bead_id: str) -> str:
    template = (RENDERER.parent / 'dag-view.html').read_text()
    return Template(template).substitute(
        title=escape(f'Sprint review · Phase {phase.upper()}'), svg=svg,
        root_bead_id=escape(root_bead_id, quote=True))


def open_wyvern(artifact: Path) -> bool:
    """Leave the viewer running until the user closes it, without holding the CLI."""
    log_path = artifact.with_suffix('.wyvern.log')
    executable = shutil.which('wyvern')
    if executable is None:
        print('Wyvern unavailable; artifact saved without opening a viewer.', file=sys.stderr)
        return False
    try:
        with log_path.open('w') as log:
            process = subprocess.Popen(
                [executable, str(artifact), '--viewer', 'embedded'],
                stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                start_new_session=True,
            )
    except OSError as exc:
        print(f'Wyvern unavailable: {exc}; artifact remains saved.', file=sys.stderr)
        return False
    try:
        result = process.wait(timeout=1)
    except subprocess.TimeoutExpired:
        print(f'Wyvern launched (PID {process.pid}); log: {log_path}')
        return True
    if result:
        print(f'Wyvern could not open the saved artifact; see {log_path}', file=sys.stderr)
        return False
    return True


def generate(repo: Path, index: object, counts: dict[str, int | None], phase: str,
             output: Path | None = None, open_image: bool = False, open_view: bool = False,
             plan_html: Path | None = None, beads: Sequence[BeadRecord] | None = None) -> int:
    index = normalized_index(index)
    if not (RENDERER / 'node_modules/@viz-js/viz').exists():
        raise RuntimeError(f'DAG renderer dependencies missing; run npm ci --prefix {RENDERER}')
    phase = str(phase).removeprefix('phase-')
    output = output or repo / 'scratchpad' / f'phase-{phase}-dag' / f'phase-{phase}-dag'
    output = output if output.is_absolute() else repo / output
    output.parent.mkdir(parents=True, exist_ok=True)
    def path(suffix: str) -> Path:
        return output.parent / (output.name + suffix)
    bead_map = {b['id']: b for b in (beads if beads is not None else phase_beads(repo, phase))}
    graph = build_graph(index, bead_map)
    snapshot = collect_state(repo, graph, bead_map, counts, phase)
    icons = states(graph, snapshot, index)
    qa = qa_states(graph, bead_map, index)
    for key in icons:
        icons[key]['qa'] = qa[key]
    source = dot_source(graph, phase)
    # Reuse approved geometry when its DOT is unchanged.
    reuse = path('-layout.svg').exists() and path('.dot').exists() and path('.dot').read_text().strip() == source.strip()
    path('.dot').write_text(source)
    if not reuse:
        render('layout', path('.dot'), path('-layout.svg'))
    svg = overlay(path('-layout.svg').read_text(), icons, snapshot['captured_at'], qa)
    path('.svg').write_text(svg)
    path('.html').write_text(html_view(svg, phase, index['root_bead_id']))
    render('png', path('.svg'), path('.png'))
    for suffix, data in [('-data.json', graph), ('-state.json', snapshot), ('-icons.json', icons)]:
        path(suffix).write_text(json.dumps(data, indent=2) + '\n')
    if snapshot['errors']:
        print('sprint-report: some ATM evidence unavailable; affected states are unconfirmed (see state JSON)', file=sys.stderr)
    for suffix in ('.svg', '.html', '.png', '-state.json'):
        print(path(suffix))
    if plan_html is not None:
        # Written next to the plan; the planner commits it with the plan, never this script.
        plan_html.parent.mkdir(parents=True, exist_ok=True)
        plan_html.write_text(path('.html').read_text())
        print(plan_html)
    if open_image:
        if sys.platform == 'darwin':
            subprocess.run(['open', '-a', 'Preview', str(path('.png'))], check=True)
        else:
            subprocess.run(['xdg-open', str(path('.png'))], check=True)
    if open_view:
        open_wyvern(path('.html'))
    return 0
