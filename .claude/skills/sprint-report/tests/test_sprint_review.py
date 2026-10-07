import importlib.machinery
import importlib.util
import json
import re
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

SKILLS = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(SKILLS / 'sprint-report/scripts'), str(SKILLS / 'atm-beads/scripts')]
import sprint_dag as dag


def load_script(name, path):
    loader = importlib.machinery.SourceFileLoader(name, str(path))
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


review = load_script('sprint_review', SKILLS / 'sprint-review/scripts/sprint-review')
artifact_check = load_script('artifact_check', SKILLS / 'atm-beads/scripts/check-phase-artifact')


class ViewTests(unittest.TestCase):
    def test_dag_edges_come_from_canonical_plan_not_live_extra_blockers(self):
        index = {'root_bead_id': 'root', 'sprints': [
            {'dev_bead_id': 'dev-a', 'sanity_bead_id': 'gate-a'},
            {'dev_bead_id': 'dev-b', 'sanity_bead_id': 'gate-b',
             'depends_on_sanity_bead_ids': ['gate-a']},
        ]}
        beads = {
            'dev-a': {'id': 'dev-a', 'dependencies': []},
            'gate-a': {'id': 'gate-a', 'labels': ['stage:dev-sanity'],
                       'dependencies': [{'type': 'blocks', 'depends_on_id': 'dev-a'}]},
            # This execution-only blocker must not become a plan/DAG edge.
            'dev-b': {'id': 'dev-b', 'dependencies': [{'type': 'blocks', 'depends_on_id': 'runtime-gate'}]},
            'gate-b': {'id': 'gate-b', 'labels': ['stage:dev-sanity'],
                       'dependencies': [{'type': 'blocks', 'depends_on_id': 'dev-b'}]},
        }
        graph = dag.build_graph(index, beads)
        self.assertEqual(graph['edges'], [
            ['dev-b', 'gate-a'], ['gate-a', 'dev-a'], ['gate-b', 'dev-b'],
        ])

    def generate(self, repo, scratch, view=False, calls=None):
        index = {'root_bead_id': 'root', 'sprints': [{'dev_bead_id': 'dev', 'sanity_bead_id': 'gate'}]}
        beads = [
            {'id': 'dev', 'dependencies': []},
            {'id': 'gate', 'labels': ['stage:dev-sanity'],
             'dependencies': [{'type': 'blocks', 'depends_on_id': 'dev'}]},
        ]
        svg = '<svg xmlns="http://www.w3.org/2000/svg"/>'
        snapshot = {'captured_at': 'now', 'errors': {}}
        calls = [] if calls is None else calls
        plan_html = repo / 'plans/phase-d/phase-d-dag.html'
        with patch.object(dag, 'run_json', return_value=beads), patch.object(dag, 'collect_state', return_value=snapshot), patch.object(dag, 'states', return_value={'dev': {}, 'gate': {}}), patch.object(dag, 'qa_states', return_value={'dev': {}, 'gate': {}}), patch.object(dag, 'overlay', return_value=svg), patch.object(dag, 'render', side_effect=lambda mode, source, target: target.write_text(svg)), patch.object(dag, 'open_wyvern', side_effect=lambda path: calls.append('view')):
            # Dependency availability is a CLI prerequisite, not part of this boundary test.
            with patch.object(dag, 'RENDERER', scratch):
                (scratch / 'node_modules/@viz-js/viz').mkdir(parents=True, exist_ok=True)
                with patch.object(dag, 'html_view', return_value='<html>' + svg + '</html>'):
                    dag.generate(repo, index, {}, 'd', output=scratch / 'out/phase-d-dag', open_view=view,
                                 plan_html=plan_html)
        return plan_html

    def test_generation_writes_plan_html_and_never_views_by_default(self):
        for view in (False, True):
            calls = []
            with tempfile.TemporaryDirectory() as directory:
                plan_html = self.generate(Path(directory) / 'repo', Path(directory), view, calls)
                self.assertIn('<svg', plan_html.read_text())
            self.assertEqual(calls, ['view'] if view else [])

    def test_generation_never_commits_or_pushes(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repo = base / 'repo'
            def git(*args):
                return subprocess.check_output(['git', *args], cwd=repo, stderr=subprocess.PIPE, text=True).strip()
            repo.mkdir()
            git('init', '-b', 'integrate/phase-d')
            git('config', 'user.email', 'test@example.invalid')
            git('config', 'user.name', 'DAG test')
            (repo / 'README').write_text('base')
            git('add', 'README')
            git('commit', '-m', 'base')
            head = git('rev-parse', 'HEAD')
            # No remote: any push would fail.
            self.generate(repo, base / 'scratch')
            self.assertEqual(git('rev-parse', 'HEAD'), head)
            self.assertEqual(git('status', '--porcelain', '--untracked-files=all'), '?? plans/phase-d/phase-d-dag.html')

    def test_initial_window_uses_eighty_percent_of_logical_screen_bounds(self):
        page = dag.html_view('<svg/>', 'd', 'root')
        script = re.search(r'<script id="wyvern-initial-size">(.*?)</script>', page, re.S).group(1)
        harness = r"""
const vm = require('node:vm');
const assert = require('node:assert/strict');
const code = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
function run(bounds, screen, withIpc = true) {
  const messages = [], listeners = {}, metas = {};
  const window = { __wyvernViewportBounds: bounds, screen,
    addEventListener: (event, callback) => listeners[event] = callback };
  if (withIpc) window.ipc = { postMessage: value => messages.push(value) };
  const document = { querySelector: name => metas[name] ||= {} };
  vm.runInNewContext(code, {window, document});
  return {messages, listeners, metas};
}
// Native bounds are logical pixels, independent of physical/Retina resolution.
const native = run({available_width: 2560, available_height: 1440}, {availWidth: 5120, availHeight: 2880});
assert.deepEqual(native.messages, ['resize:2048x1152']);
native.listeners['wyvern:viewport-bounds']({detail: {available_width: 1920, available_height: 1080}});
assert.equal(native.messages[1], 'resize:1536x864');
assert.equal(native.metas['meta[name="wyvern:width"]'].content, '1536');
assert.equal(native.metas['meta[name="wyvern:height"]'].content, '864');
assert.deepEqual(run(null, {availWidth: 1440, availHeight: 900}).messages, ['resize:1152x720']);
// The permanent artifact also works as ordinary HTML with no native bridge.
assert.deepEqual(run(null, {}, false).messages, []);
"""
        subprocess.run(['node', '-e', harness], input=json.dumps(script), text=True, check=True)

    def test_html_embeds_svg_not_png_and_escapes_root(self):
        svg = '<svg xmlns="http://www.w3.org/2000/svg"><title>Evidence</title></svg>'
        page = dag.html_view(svg, 'd', 'root"<value>')
        self.assertIn(svg, page)
        self.assertNotIn('<img', page)
        self.assertIn('content="root&quot;&lt;value&gt;"', page)
        self.assertIn('id="zoom-in"', page)

    def test_review_defaults_to_dag_without_view(self):
        for arguments, mode in [([], '--dag'), (['--view'], '--view')]:
            with self.subTest(arguments=arguments), patch.object(sys, 'argv', ['sprint-review', *arguments]), patch.object(review.os, 'execv') as execute:
                review.main()
                self.assertIn(mode, execute.call_args.args[1])
                self.assertNotIn('--output', execute.call_args.args[1])
                if not arguments:
                    self.assertNotIn('--view', execute.call_args.args[1])

    def test_missing_wyvern_does_not_launch_or_fail(self):
        with patch.object(dag.shutil, 'which', return_value=None), patch.object(dag.subprocess, 'Popen') as launch:
            self.assertFalse(dag.open_wyvern(Path('artifact.html')))
            launch.assert_not_called()

    def test_wyvern_runs_detached_without_waiting_for_user_close(self):
        process = Mock(pid=123)
        process.wait.side_effect = subprocess.TimeoutExpired('wyvern', 1)
        with tempfile.TemporaryDirectory() as directory, patch.object(dag.shutil, 'which', return_value='/bin/wyvern'), patch.object(dag.subprocess, 'Popen', return_value=process) as launch:
            path = Path(directory) / 'artifact.html'
            self.assertTrue(dag.open_wyvern(path))
            self.assertEqual(launch.call_args.args[0], ['/bin/wyvern', str(path), '--viewer', 'embedded'])
            self.assertTrue(launch.call_args.kwargs['start_new_session'])
            self.assertEqual(launch.call_args.kwargs['stdin'], subprocess.DEVNULL)
            self.assertEqual(launch.call_args.kwargs['stderr'], subprocess.STDOUT)
            self.assertNotEqual(launch.call_args.kwargs['stdout'], subprocess.PIPE)
            process.wait.assert_called_once_with(timeout=1)

    def test_wyvern_failure_does_not_fail_saved_artifact(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(dag.shutil, 'which', return_value='/bin/wyvern'), patch.object(dag.subprocess, 'Popen', return_value=Mock(wait=Mock(return_value=1))):
            self.assertFalse(dag.open_wyvern(Path(directory) / 'artifact.html'))


class PlanCheckTests(unittest.TestCase):
    def test_validate_remote_plan_without_touching_source_checkout(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            remote, repo = base / 'remote.git', base / 'repo'
            def run(*args, cwd=base):
                return subprocess.check_output(['git', *args], cwd=cwd, stderr=subprocess.PIPE, text=True).strip()
            run('init', '--bare', str(remote))
            run('init', '-b', 'integrate/phase-test', str(repo))
            run('config', 'user.email', 'test@example.invalid', cwd=repo)
            run('config', 'user.name', 'Artifact test', cwd=repo)
            (repo / 'README').write_text('base')
            # a non-default plans_dir proves the check reads it from the repository configuration
            config = repo / '.claude/project/atm-bd-orchestration.yaml'
            config.parent.mkdir(parents=True)
            config.write_text('plans_dir: work/plans\n')
            (repo / '.atm-bd').mkdir()
            (repo / '.atm-bd/phase-test.toml').write_text(
                'plan = "work/plans/phase-test.jsonl"\nroot = "tp-phase-test"\nintegration_branch = "integrate/phase-test"\n')
            plan = repo / 'work/plans/phase-test.jsonl'
            plan.parent.mkdir(parents=True)
            plan.write_text('{"sprint": "test-1"}\n')
            run('add', 'README', cwd=repo)
            run('commit', '-m', 'base', cwd=repo)
            run('remote', 'add', 'origin', str(remote), cwd=repo)
            run('push', '-u', 'origin', 'integrate/phase-test', cwd=repo)
            with self.assertRaisesRegex(RuntimeError, 'plan file work/plans/phase-test.jsonl missing'):
                artifact_check.check_artifact(repo, 'tp-phase-test', plan)
            run('add', str(plan.relative_to(repo)), cwd=repo)
            run('commit', '-m', 'plan', cwd=repo)
            run('push', 'origin', 'integrate/phase-test', cwd=repo)
            artifact_check.check_artifact(repo, 'tp-phase-test', plan)
            plan.write_text('{"sprint": "test-2"}\n')
            with self.assertRaisesRegex(RuntimeError, 'published plan file differs'):
                artifact_check.check_artifact(repo, 'tp-phase-test', plan)
            with self.assertRaisesRegex(RuntimeError, 'the phase file names tp-phase-test'):
                artifact_check.check_artifact(repo, 'other-phase-test', plan)

if __name__ == '__main__':
    unittest.main()
