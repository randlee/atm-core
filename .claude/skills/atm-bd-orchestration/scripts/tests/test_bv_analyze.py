"""Fresh input and source-selection regressions; no live bead mutations."""
import importlib.machinery
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "bv-analyze"
LOADER = importlib.machinery.SourceFileLoader("bv_analyze", str(SCRIPT))
spec = importlib.util.spec_from_loader("bv_analyze", LOADER)
analysis = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analysis)


class AnalysisTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.outputs = []
        self.calls = []
        self.rows = [{"id": "task-test", "status": "open", "labels": ["phase-test"]}]
        self.fail_export = False
        self.wrong_source = False
        self.stale = False
        self.partial = False
        self.reject = set()

    def directory(self, **kwargs):
        path = self.root / str(len(self.outputs))
        path.mkdir()
        self.outputs.append(path)
        return str(path)

    def fake_run(self, argv, repo):
        self.calls.append(argv)
        if argv[:2] == ["bd", "info"]:
            return json.dumps({"database_path": str(self.root / '.beads/dolt'), "mode": "direct"})
        if argv in (["bd", "version"], ["bv", "--version"]):
            return "test-version"
        if "export" in argv:
            if self.fail_export:
                raise subprocess.CalledProcessError(1, argv)
            Path(argv[-1]).write_text(''.join(json.dumps(row)+'\n' for row in self.rows))
            return ""
        self.assertEqual(argv[0], "bv")
        self.assertTrue(any(flag.startswith('--robot-') for flag in argv))
        source = argv[argv.index('--db')+1]
        self.assertTrue(Path(source).is_file())
        loaded = [json.loads(line) for line in Path(source).read_text().splitlines()]
        warnings = [f"line {n}: updated_at is before created_at"
                    for n, row in enumerate(loaded, 1) if row.get("id") in self.reject]
        partial = self.partial or bool(warnings)
        return json.dumps({
            "source_path": str(self.root / 'stale.jsonl') if self.wrong_source else source,
            "source_authority": {"state": "partial" if partial else "complete",
                                 "valid": len(loaded) - len(warnings),
                                 "sources": [{"stale": self.stale, "warnings": warnings}]},
        })

    def analyze(self, **kwargs):
        with patch.object(analysis, 'run', side_effect=self.fake_run), \
             patch.object(analysis.tempfile, 'mkdtemp', side_effect=self.directory):
            return analysis.analyze(self.root, **kwargs)

    def test_each_pass_exports_again_and_uses_only_its_snapshot(self):
        first = self.analyze(label='phase-test', target='task-test')
        self.rows[0]['status'] = 'closed'
        second = self.analyze(label='phase-test')
        self.assertNotEqual(first['snapshot'], second['snapshot'])
        self.assertNotEqual(first['sha256'], second['sha256'])
        self.assertTrue(first['complete'] and second['complete'])
        self.assertEqual(len([c for c in self.calls if 'export' in c]), 2)
        self.assertTrue(all('--readonly' in c and '--all' in c for c in self.calls if 'export' in c))
        chain = next(c for c in self.calls if '--robot-blocker-chain' in c)
        self.assertNotIn('--label', chain)
        prerequisites = str(Path(first['snapshot']).parent / 'prerequisites.jsonl')
        self.assertEqual(chain[chain.index('--db')+1], prerequisites)
        self.assertEqual(first['prerequisites'], ['task-test'])
        self.assertEqual(first['outputs']['blocker-chain'],
                         str(Path(first['snapshot']).parent / 'blocker-chain.json'))

    def test_export_failure_never_uses_an_existing_jsonl(self):
        (self.root / 'issues.jsonl').write_text('{"id":"old"}\n')
        self.fail_export = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.analyze()
        self.assertFalse(any('--robot-triage' in c for c in self.calls))
        self.assertFalse(json.loads((self.outputs[0] / 'receipt.json').read_text())['complete'])

    def test_duplicate_export_records_are_rejected(self):
        self.rows *= 2
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            self.analyze()

    def test_wrong_bv_source_is_rejected(self):
        self.wrong_source = True
        with self.assertRaisesRegex(ValueError, 'different source'):
            self.analyze()

    def test_partial_source_is_rejected(self):
        self.partial = True
        with self.assertRaisesRegex(ValueError, 'completely load'):
            self.analyze()

    def test_stale_source_is_rejected(self):
        self.stale = True
        with self.assertRaisesRegex(ValueError, 'stale source'):
            self.analyze()

    def test_default_collects_triage_then_alerts_on_the_same_source(self):
        receipt = self.analyze(label='phase-test')
        self.assertEqual(list(receipt['outputs']), ['triage', 'alerts'])
        self.assertEqual([c[-1] for c in receipt['commands']],
                         ['--robot-triage', '--robot-alerts'])
        for command in receipt['commands']:
            self.assertIn('--no-cache', command)
            self.assertEqual(command[command.index('--db')+1], receipt['snapshot'])
        for output in receipt['outputs'].values():
            self.assertTrue(Path(output).is_file())

    def test_requested_modes_only_and_deduplicated(self):
        receipt = self.analyze(modes=['plan', 'insights', 'priority', 'plan'])
        self.assertEqual(list(receipt['outputs']), ['plan', 'insights', 'priority'])

    def test_invalid_mode_never_reaches_tools(self):
        with self.assertRaisesRegex(ValueError, 'supported robot modes'):
            self.analyze(modes=['update'])
        self.assertEqual(self.calls, [])

    def test_missing_scope_or_target_fails_before_bv(self):
        for kwargs in ({'label': 'absent'}, {'target': 'absent'}):
            with self.subTest(kwargs=kwargs):
                self.calls.clear()
                with self.assertRaisesRegex(ValueError, 'absent from export'):
                    self.analyze(**kwargs)
                self.assertFalse(any(c[0] == 'bv' and '--version' not in c
                                     for c in self.calls))

    def test_epic_filters_before_bv_and_retains_full_graph_for_blockers(self):
        def dep(parent, kind='parent-child'):
            return {'depends_on_id': parent, 'type': kind}
        self.rows = [
            {'id': 'phase', 'issue_type': 'feature', 'dependent_count': 99},
            {'id': 'child', 'dependencies': [dep('phase'), dep('external', 'blocks')]},
            {'id': 'grandchild', 'dependencies': [dep('child')]},
            {'id': 'external'},
            {'id': 'neighbor', 'dependencies': [dep('child', 'blocks')]},
            {'id': 'phase-prefixed-but-unrelated'},
        ]
        original = json.loads(json.dumps(self.rows))
        receipt = self.analyze(epic='phase', target='child')
        self.assertEqual(self.rows, original)
        scoped = Path(receipt['analysis_snapshot'])
        loaded = [json.loads(line) for line in scoped.read_text().splitlines()]
        self.assertEqual([r['id'] for r in loaded], ['phase', 'child', 'grandchild'])
        self.assertEqual(loaded[1]['dependencies'], [dep('phase')])
        self.assertEqual(loaded[0]['dependent_count'], 1)
        self.assertEqual(loaded[1]['dependency_count'], 1)
        self.assertEqual(receipt['excluded_dependencies'], [
            {'issue_id': 'child', 'depends_on_id': 'external', 'type': 'blocks'},
            {'issue_id': 'neighbor', 'depends_on_id': 'child', 'type': 'blocks'},
        ])
        self.assertEqual(receipt['records'], 6)
        self.assertEqual(receipt['analysis_records'], 3)
        self.assertEqual(Path(receipt['snapshot']).read_text().count('\n'), 6)
        # The chain reaches the external prerequisite but not the parent or the neighbor.
        self.assertEqual(receipt['prerequisites'], ['child', 'external'])
        chain_path = Path(receipt['snapshot']).parent / 'prerequisites.jsonl'
        chain_rows = [json.loads(line) for line in chain_path.read_text().splitlines()]
        self.assertEqual([r['id'] for r in chain_rows], ['child', 'external'])
        self.assertEqual(chain_rows[0]['dependencies'], [dep('external', 'blocks')])
        for command in receipt['commands']:
            expected = str(chain_path) if '--robot-blocker-chain' in command else str(scoped)
            self.assertEqual(command[command.index('--db')+1], expected)

    def test_memory_rows_are_dropped_and_issue_rows_kept(self):
        self.rows = [
            {'_type': 'issue', 'id': 'task-test', 'labels': ['phase-test']},
            {'_type': 'memory', 'key': 'note', 'value': 'remember this'},
            {'id': 'untyped'},
        ]
        receipt = self.analyze(label='phase-test')
        raw = Path(receipt['snapshot']).parent / 'export.jsonl'
        self.assertEqual(raw.read_text().count('\n'), 3)
        kept = [json.loads(line) for line in Path(receipt['snapshot']).read_text().splitlines()]
        self.assertEqual([r['id'] for r in kept], ['task-test', 'untyped'])
        self.assertEqual(receipt['records'], 2)
        self.assertTrue(receipt['complete'])

    def test_prerequisite_scope_follows_blocks_transitively_only(self):
        rows = [
            {'id': 'a', 'dependencies': [{'depends_on_id': 'parent', 'type': 'parent-child'},
                                         {'depends_on_id': 'b', 'type': 'blocks'}]},
            {'id': 'b', 'dependencies': [{'depends_on_id': 'c', 'type': 'blocks'}]},
            {'id': 'c'},
            {'id': 'parent', 'dependencies': [{'depends_on_id': 'grandparent', 'type': 'blocks'}]},
            {'id': 'grandparent'},
            {'id': 'dependent', 'dependencies': [{'depends_on_id': 'a', 'type': 'blocks'}]},
            {'id': 'unrelated'},
        ]
        selected, excluded = analysis.prerequisite_scope(rows, 'a')
        self.assertEqual([r['id'] for r in selected], ['a', 'b', 'c'])
        self.assertEqual(selected[0]['dependencies'], [{'depends_on_id': 'b', 'type': 'blocks'}])
        self.assertEqual(excluded, [
            {'issue_id': 'a', 'depends_on_id': 'parent', 'type': 'parent-child'},
            {'issue_id': 'dependent', 'depends_on_id': 'a', 'type': 'blocks'},
        ])

    def test_bad_unrelated_row_cannot_fail_the_blocker_chain(self):
        def dep(parent, kind='parent-child'):
            return {'depends_on_id': parent, 'type': kind}
        self.rows = [
            {'id': 'phase'},
            {'id': 'a', 'dependencies': [dep('phase'), dep('b', 'blocks')]},
            {'id': 'b', 'dependencies': [dep('phase')]},
            {'id': 'old-bad-row'},
        ]
        self.reject = {'old-bad-row'}
        receipt = self.analyze(epic='phase', target='a', modes=['triage'])
        self.assertTrue(receipt['complete'])
        self.assertEqual(receipt['prerequisites'], ['a', 'b'])
        self.assertEqual(list(receipt['outputs']), ['triage', 'blocker-chain'])

    def test_partial_load_names_the_rejected_bead(self):
        self.rows = [{'id': 'fine'}, {'id': 'broken'}, {'id': 'also-fine'}]
        self.reject = {'broken'}
        with self.assertRaises(ValueError) as caught:
            self.analyze()
        message = str(caught.exception)
        self.assertIn("rejected ['broken']", message)
        self.assertIn('line 2: updated_at is before created_at', message)
        receipt = json.loads((self.outputs[0] / 'receipt.json').read_text())
        self.assertFalse(receipt['complete'])
        self.assertEqual(receipt['error'], message)

    def test_epic_traversal_terminates_with_cyclic_parent_links(self):
        self.rows = [
            {'id': 'a', 'issue_type': 'epic', 'dependencies': [{'type': 'parent-child', 'depends_on_id': 'b'}]},
            {'id': 'b', 'dependencies': [{'type': 'parent-child', 'depends_on_id': 'a'}]},
        ]
        receipt = self.analyze(epic='a')
        self.assertEqual(receipt['members'], ['a', 'b'])

    def test_invalid_epic_fails_before_analysis(self):
        with self.assertRaisesRegex(ValueError, 'absent from export'):
            self.analyze(epic='absent')
        self.assertFalse(any('--robot-triage' in c for c in self.calls))

    def test_plan_file_is_analyzed_without_bd_and_scoped_by_epic(self):
        plan = self.root / 'plan.jsonl'
        rows = [
            {'id': 'root', 'issue_type': 'epic'},
            {'id': 'x-1', 'dependencies': [{'depends_on_id': 'root', 'type': 'parent-child'}]},
            {'id': 'x-1-sanity', 'dependencies': [{'depends_on_id': 'root', 'type': 'parent-child'},
                                                   {'depends_on_id': 'x-1', 'type': 'blocks'}]},
        ]
        plan.write_text(''.join(json.dumps(row) + '\n' for row in rows))
        receipt = self.analyze(plan_file=plan, epic='root', modes=['insights'])
        self.assertTrue(receipt['complete'])
        self.assertFalse(any(c[0] == 'bd' for c in self.calls))
        self.assertEqual(receipt['plan_file'], str(plan.resolve()))
        self.assertEqual(Path(receipt['snapshot']).read_bytes(), plan.read_bytes())
        self.assertEqual(receipt['members'], ['root', 'x-1', 'x-1-sanity'])
        self.assertEqual(list(receipt['outputs']), ['insights'])

    def test_missing_plan_file_fails_before_any_tool(self):
        with self.assertRaises(FileNotFoundError):
            self.analyze(plan_file=self.root / 'absent.jsonl')
        self.assertEqual(self.calls, [])

    def test_epic_and_label_cannot_silently_intersect(self):
        with self.assertRaisesRegex(ValueError, 'not both'):
            self.analyze(epic='a', label='phase-test')
        self.assertEqual(self.calls, [])


if __name__ == '__main__':
    unittest.main()
