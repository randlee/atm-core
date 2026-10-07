"""Plan-scoped, live-bead execution DAG for sprint-report --dag."""
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
from graphlib import TopologicalSorter, CycleError
from html import escape
import json
from pathlib import Path
import re
import shutil
import subprocess
from string import Template
import sys
import xml.etree.ElementTree as ET

from sprint_index_common import run_json, index_bead_pairs, validate_index
from sprint_qa import choose_round, qa_icon, qa_verdict, findings_summary

RENDERER = Path(__file__).resolve().parents[1] / 'renderer'
NS = 'http://www.w3.org/2000/svg'
ET.register_namespace('', NS)


def tag(name):
    return '{' + NS + '}' + name


def prerequisites(bead):
    return [edge['depends_on_id'] for edge in bead.get('dependencies', [])
            if edge.get('type') == 'blocks']


def build_graph(index, beads):
    """Render only the declared phase graph; Beads contribute live state."""
    validate_index(index)
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
    edges = []
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


def collect_state(repo, graph, beads, counts):
    errors = {}
    def history(key):
        try:
            return key, run_json(repo, 'atm', 'task', 'events', key, '--all', '--json')['events']
        except (OSError, RuntimeError, KeyError, subprocess.CalledProcessError) as exc:
            return key, str(exc)
    with ThreadPoolExecutor(max_workers=4) as pool:
        histories = dict(pool.map(history, graph['nodes']))
    for key, value in list(histories.items()):
        if not isinstance(value, list):
            errors[key] = value
            histories[key] = None
    try:
        tasks = {t['task_id']: t for t in run_json(repo, 'atm', 'task', 'list', '--all', '--json')
                 if t['task_id'] in graph['nodes']}
    except (OSError, RuntimeError, subprocess.CalledProcessError) as exc:
        tasks = {}
        errors['task_list'] = str(exc)
    blocked = {b['id']: b.get('blocked_by', [])
               for b in run_json(repo, 'bd', 'blocked', '--json')}
    return {'beads': beads, 'tasks': tasks, 'blocked': blocked, 'events': histories,
            'iterations': counts, 'errors': errors,
            'captured_at': datetime.now(timezone.utc).isoformat()}


def states(graph, snapshot, index):
    beads, events = snapshot['beads'], snapshot['events']
    pairs = graph['gate_pairs']
    gates = {gate: dev for dev, gate in pairs.items()}
    def completed(key):
        return [e for e in (events.get(key) or []) if e.get('event') == 'completed']
    def latest(key):
        return max((e['at'] for e in completed(key)), default='')
    def open_findings(key):
        dev = gates.get(key)
        return [b['id'] for b in beads.values() if b.get('status') != 'closed' and any(
            edge.get('type') in ('discovered-from', 'parent-child')
            and (edge.get('depends_on_id') == key or (
                edge.get('depends_on_id') == dev
                and any('sanity' in label for label in b.get('labels', []))))
            for edge in b.get('dependencies', []))]
    def verdict(key):
        bead = beads[key]
        reason = bead.get('close_reason') or ''
        if 'overridden' in reason.lower():
            return 'override', reason
        task_state = snapshot.get('tasks', {}).get(key, {}).get('state')
        history_state = (events.get(key) or [{}])[-1].get('to_state')
        if task_state == 'active' or history_state == 'active' or bead['status'] == 'in_progress':
            return 'active', 'Work in progress'
        if key in snapshot['blocked'] or bead['status'] == 'blocked':
            return 'blocked', 'Blocked by unfinished prerequisites'
        if task_state in ('assigned', 'queued') or history_state == 'assigned':
            return 'waiting', 'Assigned; completion not recorded'
        if key in gates:
            findings = open_findings(key)
            if findings:
                return 'findings', 'Open sanity findings: ' + ', '.join(findings)
            meta = bead.get('metadata') or {}
            passed = str(meta.get('verdict', '')).upper() == 'PASS' or re.match(r'^PASS\b', reason, re.I)
            if bead['status'] == 'closed' and passed:
                dev = gates[key]
                if events.get(key) is None or events.get(dev) is None or 'task_list' in snapshot.get('errors', {}):
                    return 'uncertain', reason + '; ATM history unavailable'
                if not latest(key) or not latest(dev):
                    return 'uncertain', reason + '; completion chronology unavailable'
                if latest(dev) > latest(key):
                    return 'uncertain', reason + '; PASS predates latest dev completion'
                return 'done', reason
            if bead['status'] == 'closed' or re.match(r'^FAIL\b', reason, re.I):
                return 'findings', reason or 'No explicit sanity PASS'
        elif key in pairs and bead['status'] == 'closed':
            if not completed(key) or 'task_list' in snapshot.get('errors', {}):
                return 'uncertain', reason + '; dev completion history unavailable'
            gate_state, detail = verdict(pairs[key])
            if gate_state == 'done':
                return 'done', reason + '; sanity PASS recorded'
            if gate_state in ('uncertain', 'findings'):
                return gate_state, reason + '; ' + detail
            return 'waiting', reason + '; awaiting sanity completion'
        return 'waiting', 'Not started / awaiting completion'
    result = {}
    for key in graph['nodes']:
        state, detail = verdict(key)
        count = None
        if key in gates:
            logged = snapshot.get('iterations', {}).get(key)
            count = max(logged or 0, len(completed(key))) if logged is not None or events.get(key) is not None else '?'
        result[key] = {'state': state, 'sanity_iterations': count, 'evidence': detail}
    return result


def is_qa_of(bead, dev_id):
    """A QA round of dev_id: a stage:qa child of it (phase contract; bd keeps one edge type per pair) or a
    pre-contract round that `validates` it."""
    edges = bead.get('dependencies') or []
    if any(edge.get('type') == 'validates' and edge.get('depends_on_id') == dev_id for edge in edges):
        return True
    if 'stage:qa' not in (bead.get('labels') or []):
        return False
    metadata = bead.get('metadata') if isinstance(bead.get('metadata'), dict) else {}
    return bead.get('parent') == dev_id or metadata.get('checked_bead') == dev_id or any(
        edge.get('type') == 'parent-child' and edge.get('depends_on_id') == dev_id for edge in edges)


def qa_states(graph, beads, index):
    """Share table QA semantics, counting open findings across every round."""
    result = {}
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


def dot_source(graph, phase):
    quote = lambda text: json.dumps(text, ensure_ascii=False)
    phase = str(phase).removeprefix('phase-').upper()
    lines = ['digraph execution {',
             'graph [rankdir=LR, compound=true, bgcolor="white", pad="0.4", nodesep="0.22", ranksep="0.5", splines=spline, fontname="Arial", fontsize=20, labelloc=t, label=' + quote(f'Phase {phase} — execution order\nSprint completes → its gate passes → downstream sprint starts') + '];',
             'node [shape=box, style="rounded,filled", fontname="Arial", fontsize=13, margin="0.12,0.10", color="#9caec2", fillcolor="#edf3fb", fontcolor="#172b43"];',
             'edge [color="#647a94", penwidth=1.15, arrowsize=0.75];']
    pairs = graph['gate_pairs']
    def order(key):
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
def badge(parent,x,y,state,count=None):
 w=38 if count is not None else 20
 g=ET.SubElement(parent,tag('g'),{'class':'state-badge','transform':f'translate({x-w/2:g} {y-9:g})'})
 ET.SubElement(g,tag('rect'),{'width':str(w),'height':'18','rx':'6','fill':'white','stroke':colors[state],'stroke-width':'1.2'})
 if state=='done':ET.SubElement(g,tag('path'),{'d':'M 5 9 L 8 12 L 14 5','fill':'none','stroke':colors[state],'stroke-width':'2.5','stroke-linecap':'round','stroke-linejoin':'round'})
 else:
  t=ET.SubElement(g,tag('text'),{'x':'10','y':'13','text-anchor':'middle','font-family':'Arial','font-size':'15','font-weight':'bold','fill':colors[state]});t.text=symbols[state]
 if count is not None:
  t=ET.SubElement(g,tag('text'),{'x':'27','y':'13','text-anchor':'middle','font-family':'Arial','font-size':'12','font-weight':'bold','fill':colors[state]});t.text=str(count)


def qa_badge(parent, x, y, qa):
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
            group.remove(group.find(tag('text')))
            ET.SubElement(group, tag('path'), {'d': 'M4 10V14H16V10 M10 3V10 M7 7L10 10L13 7',
                'fill': 'none', 'stroke': colors['waiting'], 'stroke-width': '1.5'})
    title = ET.SubElement(group, tag('title'))
    title.text = 'QA ' + (qa['verdict'] or icon)
    if counts is not None:
        title.text += ': ' + counts + ' open blocking:important:minor'
    if qa['qa_beads']:
        title.text += '; ' + ', '.join(qa['qa_beads'])


def overlay(layout, icons, captured_at, qa=None):
    root = ET.fromstring(layout)
    graph = root.find(tag('g'))
    # Put badges over edges, preserving every original node/edge coordinate.
    badges = ET.Element(tag('g'), {'class': 'state-badges'})
    for node in graph.findall(tag('g')):
        if node.get('class') != 'node':
            continue
        title = node.find(tag('title'))
        key = title.text
        icon = icons[key]
        path = node.find(tag('path'))
        numbers = [float(n) for n in re.findall(r'-?\d+(?:\.\d+)?', path.get('d'))]
        y = min(numbers[1::2])
        x = float(node.find(tag('text')).get('x'))
        title.text = key + ' — ' + icon['evidence']
        if icon['sanity_iterations'] is not None:
            title.text += f"; completed sanity iterations: {icon['sanity_iterations']}"
        badge(badges, x, y, icon['state'], icon['sanity_iterations'])
        if qa and key in qa:
            qa_badge(badges, x, max(numbers[1::2]), qa[key])
    graph.append(badges)
    view = list(map(float, root.get('viewBox').split()))
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


def render(mode, source, target):
    subprocess.run(['node', str(RENDERER / 'render.cjs'), mode, str(source), str(target)], check=True)


def html_view(svg, phase, root_bead_id):
    template = (RENDERER.parent / 'dag-view.html').read_text()
    return Template(template).substitute(
        title=escape(f'Sprint review · Phase {phase.upper()}'), svg=svg,
        root_bead_id=escape(root_bead_id, quote=True))


def open_wyvern(artifact):
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


def generate(repo, index, counts, phase, output=None, open_image=False, open_view=False,
             plan_html=None):
    if not (RENDERER / 'node_modules/@viz-js/viz').exists():
        raise RuntimeError(f'DAG renderer dependencies missing; run npm ci --prefix {RENDERER}')
    phase = str(phase).removeprefix('phase-')
    output = output or repo / 'scratchpad' / f'phase-{phase}-dag' / f'phase-{phase}-dag'
    output = output if output.is_absolute() else repo / output
    output.parent.mkdir(parents=True, exist_ok=True)
    def path(suffix):
        return output.parent / (output.name + suffix)
    beads = {b['id']: b for b in run_json(repo, 'bd', 'list', '--all', '-n', '0', '--json')}
    graph = build_graph(index, beads)
    snapshot = collect_state(repo, graph, beads, counts)
    icons = states(graph, snapshot, index)
    qa = qa_states(graph, beads, index)
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
        # Written next to the plan; never committed or pushed.
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
