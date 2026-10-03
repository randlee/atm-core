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
import phase_artifact as publication


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

    def test_generation_publishes_before_optional_view_and_never_views_by_default(self):
        index = {'root_bead_id': 'root', 'sprints': [{'dev_bead_id': 'dev', 'sanity_bead_id': 'gate'}]}
        beads = [
            {'id': 'dev', 'dependencies': []},
            {'id': 'gate', 'labels': ['stage:dev-sanity'],
             'dependencies': [{'type': 'blocks', 'depends_on_id': 'dev'}]},
        ]
        svg = '<svg xmlns="http://www.w3.org/2000/svg"/>'
        snapshot = {'captured_at': 'now', 'errors': {}}
        for view in (False, True):
            calls = []
            def publish(*args):
                calls.append('publish')
                return {'branch': 'integrate/phase-d', 'html_path': 'phase-d-dag.html', 'commit': 'abc123'}
            with tempfile.TemporaryDirectory() as directory, patch.object(dag, 'run_json', return_value=beads), patch.object(dag, 'collect_state', return_value=snapshot), patch.object(dag, 'states', return_value={'dev': {}, 'gate': {}}), patch.object(dag, 'qa_states', return_value={'dev': {}, 'gate': {}}), patch.object(dag, 'overlay', return_value=svg), patch.object(dag, 'render', side_effect=lambda mode, source, target: target.write_text(svg)), patch.object(publication, 'publish_artifact', side_effect=publish), patch.object(dag, 'open_wyvern', side_effect=lambda path: calls.append('view')):
                # Dependency availability is a CLI prerequisite, not part of this boundary test.
                with patch.object(dag, 'RENDERER', Path(directory)):
                    (Path(directory) / 'node_modules/@viz-js/viz').mkdir(parents=True)
                    with patch.object(dag, 'html_view', return_value='<html>' + svg + '</html>'):
                        dag.generate(Path(directory), index, {}, 'd', open_view=view,
                                     publish_branch='integrate/phase-d')
            self.assertEqual(calls, ['publish', 'view'] if view else ['publish'])

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

    def test_review_defaults_to_publish_without_view(self):
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


class PublicationTests(unittest.TestCase):
    def test_publish_and_validate_remote_artifact_without_touching_source_checkout(self):
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
            # a non-default plans_dir proves both scripts read it from the repository configuration
            config = repo / '.claude/project/atm-bd-orchestration.yaml'
            config.parent.mkdir(parents=True)
            config.write_text('plans_dir: work/plans\n')
            plan = repo / 'work/plans/phase-test/sprints.jsonl'
            plan.parent.mkdir(parents=True)
            plan.write_text('["test-1", "gate", []]\n')
            run('add', 'README', str(plan.relative_to(repo)), cwd=repo)
            run('commit', '-m', 'base', cwd=repo)
            run('remote', 'add', 'origin', str(remote), cwd=repo)
            run('push', '-u', 'origin', 'integrate/phase-test', cwd=repo)
            run('switch', '-c', 'feature', cwd=repo)
            (repo / 'unrelated.txt').write_text('preserve staged work')
            run('add', 'unrelated.txt', cwd=repo)
            initial = run('rev-parse', 'HEAD', cwd=repo)
            root = [{'id': 'tp-phase-test', 'metadata': {'phase': 'test', 'integration_branch': 'integrate/phase-test'}}]
            with patch.object(artifact_check, 'run_json', return_value=root):
                with self.assertRaisesRegex(RuntimeError, 'required phase index/HTML artifact missing'):
                    artifact_check.check_artifact(repo, 'tp-phase-test', plan)
            html = dag.html_view('<svg xmlns="http://www.w3.org/2000/svg"/>', 'test', 'tp-phase-test')
            result = publication.publish_artifact(repo, 'integrate/phase-test', 'test', html)
            self.assertEqual(run('rev-parse', 'HEAD', cwd=repo), initial)
            self.assertEqual(run('diff', '--cached', '--name-only', cwd=repo), 'unrelated.txt')
            self.assertEqual(run('show', f"{result['commit']}:{result['html_path']}", cwd=repo), html.strip())
            remote_head = run('--git-dir', str(remote), 'rev-parse', 'refs/heads/integrate/phase-test')
            self.assertEqual(remote_head, result['commit'])
            changed = set(run('diff-tree', '--no-commit-id', '--name-only', '-r', result['commit'], cwd=repo).splitlines())
            self.assertEqual(changed, {result['html_path']})
            self.assertEqual(result['html_path'], 'work/plans/phase-test/phase-test-dag.html')
            self.assertEqual(run('worktree', 'list', '--porcelain', cwd=repo).count('worktree '), 1)
            with patch.object(artifact_check, 'run_json', return_value=root):
                artifact_check.check_artifact(repo, 'tp-phase-test', plan)
                plan.write_text('["test-2", "other-gate", []]\n')
                with self.assertRaisesRegex(RuntimeError, 'phase plan differs'):
                    artifact_check.check_artifact(repo, 'tp-phase-test', plan)
            # Publishing identical bytes must not create an extra commit.
            again = publication.publish_artifact(repo, 'integrate/phase-test', 'test', html)
            self.assertEqual(again['commit'], result['commit'])


if __name__ == '__main__':
    unittest.main()
