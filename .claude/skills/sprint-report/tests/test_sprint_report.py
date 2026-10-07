import io
import importlib.machinery
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/sprint-report'
loader = importlib.machinery.SourceFileLoader('sprint_report', str(SCRIPT))
spec = importlib.util.spec_from_loader(loader.name, loader)
report = importlib.util.module_from_spec(spec)
loader.exec_module(report)


class SprintReportTests(unittest.TestCase):
    def run_main_with_index(self, repo, path):
        with mock.patch.object(report.subprocess, 'check_output', return_value=f'{repo}\n'):
            with mock.patch.object(report.sys, 'argv', ['sprint-report', '--index', str(path)]):
                with mock.patch.object(report.sys, 'stderr', new_callable=io.StringIO) as stderr:
                    return report.main(), stderr.getvalue()

    def test_report_template_and_skill_keep_current_usage_text(self):
        repo = Path(__file__).resolve().parents[4]
        template = (repo / '.claude/skills/sprint-report/report.md.j2').read_text()
        skill = (repo / '.claude/skills/sprint-report/SKILL.md').read_text()
        script = (repo / '.claude/skills/sprint-report/scripts/sprint-report').read_text()
        self.assertIn('Sprint status report for phase plans.', template)
        self.assertNotIn('agent-team-mail', template)
        self.assertIn('`--table` is the default mode', skill)
        self.assertIn('"\\n\\n".join(detailed_rows)', script)

    def test_loads_plan_lines_and_rejects_invalid_rows(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            path = repo / 'docs/plans/phase-x.jsonl'
            path.parent.mkdir(parents=True)
            path.write_text('{"sprint": "x-1"}\n{"sprint": "x-2", "depends_on": ["x-1"]}\n')
            index = report.load_index(repo, path, 'atm-phase-x')[1]
            self.assertEqual(index['root_bead_id'], 'atm-phase-x')
            self.assertEqual(index['sprints'][1], {
                'sprint_bead_id': 'atm-x-2', 'dev_bead_id': 'atm-x-2.group-dev',
                'sanity_bead_id': 'atm-x-2.group-sanity',
                'depends_on_sanity_bead_ids': ['atm-x-1.group-sanity'],
            })
            path.write_text('["x-1", "gate-1", []]\n')
            with self.assertRaisesRegex(RuntimeError, 'each line is'):
                report.load_index(repo, path, 'atm-phase-x')
            path.write_text('{"sprint": "x-1", "depends_on": ["unknown"]}\n')
            with self.assertRaisesRegex(RuntimeError, 'unknown sprint'):
                report.load_index(repo, path, 'atm-phase-x')
            path.write_text('')
            with self.assertRaisesRegex(RuntimeError, 'lists no sprints'):
                report.load_index(repo, path, 'atm-phase-x')

    def test_root_lookup_uses_phase_index_path_without_root_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            path = repo / 'docs/plans/phase-x.jsonl'
            path.parent.mkdir(parents=True)
            path.write_text('{"sprint": "x-1"}\n')
            with mock.patch.object(report, 'index_path', return_value=path) as index_path:
                index = report.load_index(repo, None, 'atm-phase-x')[1]
            index_path.assert_called_once_with(repo, 'atm-phase-x')
            self.assertEqual(index['root_bead_id'], 'atm-phase-x')

    def test_root_lookup_executes_phase_id_fallback_end_to_end(self):
        checkout = Path(__file__).resolve().parents[4]
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            subprocess.run(['git', 'init', '-q'], cwd=repo, check=True)
            helper_dir = repo / '.claude/skills/atm-beads/scripts'
            helper_dir.mkdir(parents=True)
            for name in ('phase-index-path', 'sprint_index_common.py', 'repo_config.py'):
                shutil.copy2(checkout / '.claude/skills/atm-beads/scripts' / name, helper_dir / name)
            config = repo / '.claude/project/atm-bd-orchestration.yaml'
            config.parent.mkdir(parents=True)
            config.write_text('plans_dir: docs/plans\n')
            (repo / '.atm-bd').mkdir()
            (repo / '.atm-bd/phase-x.toml').write_text(
                'plan = "docs/plans/phase-x.jsonl"\nroot = "atm-phase-x"\nintegration_branch = "integrate/phase-x"\n')
            path = repo / 'docs/plans/phase-x.jsonl'
            path.parent.mkdir(parents=True)
            path.write_text('{"sprint": "x-1"}\n')
            bin_dir = repo / 'bin'
            bin_dir.mkdir()
            bd = bin_dir / 'bd'
            bd.write_text('#!/bin/sh\nprintf \'[{"id":"atm-phase-x","metadata":{}}]\\n\'\n')
            bd.chmod(0o755)
            old_cwd = Path.cwd()
            try:
                os.chdir(repo)
                with mock.patch.dict(os.environ, {'PATH': f'{bin_dir}{os.pathsep}{os.environ["PATH"]}'}):
                    actual_path, index = report.load_index(repo, None, 'atm-phase-x')
            finally:
                os.chdir(old_cwd)
            self.assertEqual(actual_path.resolve(), path.resolve())
            self.assertEqual(index['root_bead_id'], 'atm-phase-x')

    def test_main_reports_malformed_json_without_traceback(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            path = repo / 'docs/plans/phase-x.jsonl'
            path.parent.mkdir(parents=True)
            path.write_text('{not json}\n')
            code, error = self.run_main_with_index(repo, path)
            self.assertEqual(code, 2)
            self.assertIn('sprint-report:', error)
            self.assertNotIn('Traceback', error)

    def test_main_reports_row_without_id_without_traceback(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            path = repo / 'docs/plans/phase-x.jsonl'
            path.parent.mkdir(parents=True)
            path.write_text('{"sprint": ""}\n')
            code, error = self.run_main_with_index(repo, path)
            self.assertEqual(code, 2)
            self.assertIn('sprint-report:', error)
            self.assertNotIn('Traceback', error)

    def test_main_rejects_empty_index_before_bd_show(self):
        with tempfile.TemporaryDirectory() as directory:
            repo = Path(directory)
            path = repo / 'docs/plans/phase-x.jsonl'
            path.parent.mkdir(parents=True)
            path.write_text('')
            with mock.patch.object(report, 'run_json') as run_json:
                code, error = self.run_main_with_index(repo, path)
            self.assertEqual(code, 2)
            run_json.assert_not_called()
            self.assertIn('lists no sprints', error)

    def test_membership_index_reads_names_and_order_from_live_beads(self):
        index = {'sprints': [
            {'sprint_bead_id': 's-1', 'dev_bead_id': 'dev-1', 'sanity_bead_id': 'gate-1'},
            {'dev_bead_id': 'dev-2', 'sanity_bead_id': 'gate-2'}]}
        beads = {'dev-1': {'title': 's-1: dev', 'metadata': {'sprint': 's-1', 'layer': 5}},
                 's-1': {'title': 'First', 'metadata': {'sprint': 's-1', 'layer': 2}},
                 'dev-2': {'title': 'Second', 'metadata': {'sprint': 's-2', 'layer': 1}}}
        rows = report.live_sprint_rows(index, beads)
        self.assertEqual([row['id'] for row in rows], ['dev-2', 'dev-1'])
        beads['s-1']['metadata']['layer'] = 0
        beads['s-1']['title'] = 'Changed in beads'
        rows = report.live_sprint_rows(index, beads)
        self.assertEqual(rows[0]['title'], 'Changed in beads')
        self.assertEqual(rows[0]['sprint'], 's-1')
        self.assertEqual(set(index['sprints'][1]), {'dev_bead_id', 'sanity_bead_id'})

    def test_historical_sanity_count_counts_only_completed_runs(self):
        events = {'events': [{'event': event} for event in (
            'assigned', 'started', 'completed', 'reopened', 'started',
            'completed', 'reopened', 'started', 'completed', 'refused'
        )]}
        self.assertEqual(report.completed_sanity_runs(events), 3)
        self.assertEqual(report.completed_sanity_runs({'events': []}), 0)

    def test_sanity_iterations_use_cumulative_completion_not_line_count(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'phase-d.jsonl'
            self.assertEqual(report.sanity_iterations(path), {})
            runs = [
                {'task': 'sanity-1', 'iteration': i, 'verdict': 'PASS', 'completed_at': '2026-09-26'}
                for i in (10, 13, 13, 11)
            ]
            runs.append({'task': 'sanity-1', 'iteration': 14, 'verdict': 'IN PROGRESS'})
            runs.append({'task': 'sanity-2', 'iteration': 6, 'verdict': 'FAIL', 'completed_at': '2026-09-26'})
            path.write_text('\n'.join(json.dumps(run) for run in runs) + '\n')
            self.assertEqual(report.sanity_iterations(path), {'sanity-1': 13, 'sanity-2': 6})

    def test_renamed_sanity_history_merges_with_phase_log_and_filters_other_phases(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            historical = root / 'sanity-llm.jsonl'
            old_rows = [
                {'task': 'sanity-1', 'phase': 'phase-d', 'iteration': 9},
                {'task': 'sanity-old', 'sprint': 'd-2', 'iteration': 4},
                {'task': 'sanity-1', 'phase': 'e', 'iteration': 99},
                {'task': 'other-phase', 'sprint': 'e-2', 'iteration': 80},
            ]
            for row in old_rows:
                row.update(verdict='PASS', completed_at='2026-09-26T00:00:00Z')
            historical.write_text('\n'.join(json.dumps(row) for row in old_rows) + '\n')
            original = historical.read_bytes()
            self.assertEqual(report.phase_sanity_iterations(root, 'd'), {'sanity-1': 9, 'sanity-old': 4})
            paired = [dict(task='sanity-1', iteration=10, verdict='PASS', run_id='paired',
                           reviewer=reviewer, completed_at='2026-09-30T00:00:00Z')
                      for reviewer in ('sanity-llm', 'sanity-jev')]
            (root / 'phase-d.jsonl').write_text('\n'.join(json.dumps(row) for row in paired) + '\n')
            self.assertEqual(report.phase_sanity_iterations(root, 'phase-d'), {'sanity-1': 10, 'sanity-old': 4})
            self.assertEqual(historical.read_bytes(), original)

    def test_qa_table_uses_original_icons(self):
        cases = [
            (None, [], ''),
            ({'status': 'open'}, [], '📥'),
            ({'status': 'in_progress'}, [], '🌀'),
            ({'status': 'closed', 'close_reason': 'PASS: verified'}, [], '✅'),
            ({'status': 'closed', 'close_reason': 'FAIL: findings'}, [{'status': 'closed'}], '🚩'),
            ({'status': 'closed', 'close_reason': 'PASS: verified'}, [{'status': 'open'}], '🚩'),
        ]
        for qa, findings, expected in cases:
            with self.subTest(qa=qa, findings=findings):
                self.assertEqual(report.qa_icon(qa, findings), expected)

    def test_open_pr_preferred_over_closed_reuse(self):
        prs = [{'number': 4, 'headRefName': 'sprint/d-1', 'state': 'OPEN'},
               {'number': 6, 'headRefName': 'sprint/d-1', 'state': 'CLOSED'}]
        self.assertEqual(report.select_pr(prs, 'sprint/d-1')['number'], 4)
        self.assertIsNone(report.select_pr(prs, None))

    def test_pr_lookup_uses_head_branch_not_planned_base(self):
        # d-13's PR is based on d-12 even though its plan targeted d-21.
        # A lookup by base would miss it (or choose an unrelated PR sharing
        # the plan target); the report must use the source branch alone.
        prs = [
            {'number': 234, 'headRefName': 'sprint/d-13-logging-contract',
             'baseRefName': 'sprint/d-12-types-and-otlp-contract', 'state': 'OPEN'},
            {'number': 999, 'headRefName': 'unrelated',
             'baseRefName': 'sprint/d-21-otlp-contract', 'state': 'OPEN'},
        ]
        self.assertEqual(
            report.select_pr(prs, 'sprint/d-13-logging-contract')['number'], 234
        )
        self.assertIsNone(report.select_pr(prs, None))

    def test_integration_target_is_the_pr_base_not_a_fixed_branch(self):
        pr = {'number': 7, 'state': 'OPEN', 'headRefName': 'integrate/phase-x', 'baseRefName': 'main',
              'statusCheckRollup': []}
        self.assertTrue(report.integration_detail('integrate/phase-x', pr).startswith('Integration: integrate/phase-x → main\n'))
        self.assertTrue(report.integration_detail('integrate/phase-x', None).startswith('Integration: integrate/phase-x\n'))
        self.assertNotIn('develop', report.integration_detail('integrate/phase-x', None))

    def test_running_check_is_not_green(self):
        self.assertEqual(report.check_icon({'statusCheckRollup': [{'status': 'IN_PROGRESS', 'conclusion': ''}]}), '🌀')

    def test_closed_dev_without_sanity_is_flagged(self):
        self.assertEqual(report.status_icon({'status': 'closed'}, None), '🚩')

    def test_missing_sanity_gate_is_flagged_except_for_blocked_work(self):
        self.assertEqual(report.status_icon({'status': 'in_progress'}, None), '🚩')
        self.assertEqual(report.status_icon({'status': 'blocked'}, None), '🚧')

    def test_dev_done_requires_explicit_sanity_pass(self):
        dev = {'status': 'closed'}
        self.assertEqual(report.status_icon(dev, {'status': 'closed'}), '🚩')
        self.assertEqual(report.status_icon(dev, {'status': 'closed', 'close_reason': 'FAIL at abc'}), '🚩')
        self.assertEqual(report.status_icon(dev, {'status': 'closed', 'close_reason': 'PASS at abc'}), '✅')
        self.assertEqual(report.status_icon(dev, {'status': 'closed', 'metadata': {'verdict': 'PASS'}}), '✅')
        self.assertEqual(report.status_icon(dev, {'status': 'open'}), '📥')

    def test_dependency_blocked_open_sprint_is_not_assigned(self):
        dev, sanity = {'status': 'open'}, {'status': 'open'}
        self.assertEqual(report.status_icon(dev, sanity, blocked=True), '🚧')
        self.assertEqual(report.status_icon(dev, sanity), '📥')
        self.assertEqual(report.status_icon({'status': 'blocked'}, sanity), '🚧')

    def test_findings_counts_severity_and_excludes_closed(self):
        findings = [
            {'status': 'open', 'metadata': {'severity': 'blocking'}},
            {'status': 'in_progress', 'metadata': {'severity': 'important'}},
            {'status': 'open', 'labels': ['severity:minor']},
            {'status': 'closed', 'metadata': {'severity': 'blocking'}},
        ]
        self.assertEqual(report.findings_summary(findings), '1:1:1')
        self.assertEqual(report.findings_summary([]), '0:0:0')

    def test_dev_sanity_findings_and_fixing_icons(self):
        dev, sanity = {'status': 'closed'}, {'status': 'open'}
        self.assertEqual(report.status_icon(dev, sanity, [{'status': 'open'}]), '🚩')
        self.assertEqual(report.status_icon(dev, sanity, [{'status': 'in_progress'}]), '🔨')

    def test_ci_blocked_ready_and_failure_precedence(self):
        pr = {'state': 'OPEN', 'isDraft': False, 'mergeable': 'MERGEABLE',
              'mergeStateStatus': 'CLEAN', 'statusCheckRollup': [{'conclusion': 'SUCCESS'}]}
        self.assertEqual(report.check_icon(pr), '✅')
        self.assertEqual(report.check_icon(pr, merge_ready=True), '🚀')
        self.assertEqual(report.check_icon(dict(pr, isDraft=True)), '✅')
        pr['mergeStateStatus'] = 'BLOCKED'
        self.assertEqual(report.check_icon(pr), '🚧')
        pr['statusCheckRollup'] = [{'conclusion': 'FAILURE'}]
        self.assertEqual(report.check_icon(pr), '❌')

    def test_merge_ready_requires_sanity_qa_pass_and_closed_findings(self):
        qa = {'status': 'closed', 'metadata': {'verdict': 'PASS'}}
        self.assertTrue(report.review_ready('✅', qa, []))
        self.assertFalse(report.review_ready('📥', qa, []))
        self.assertFalse(report.review_ready('✅', None, []))
        self.assertFalse(report.review_ready('✅', {'status': 'in_progress'}, []))
        self.assertFalse(report.review_ready('✅', {'status': 'closed', 'close_reason': 'FAIL: findings'}, []))
        self.assertFalse(report.review_ready('✅', qa, [
            {'status': 'open', 'metadata': {'severity': 'blocking'}}]))

    def test_fail_verdict_survives_closed_findings(self):
        qa = {'status': 'closed', 'close_reason': 'FAIL: two findings', 'metadata': {'round': 2}}
        self.assertEqual(report.qa_summary(qa, [{'status': 'closed'}]), 'R2 FAIL (0 open)')

    def test_explicit_qa_verdict_overrides_legacy_close_text(self):
        qa = {
            'status': 'closed',
            'close_reason': 'PASS: stale legacy text',
            'metadata': {'round': 2, 'verdict': 'FAIL'},
        }
        self.assertEqual(report.qa_verdict(qa), 'FAIL')
        self.assertEqual(report.qa_icon(qa, [{'status': 'closed'}]), '🚩')

    def test_verdict_is_prefix_not_substring(self):
        qa = {'status': 'closed', 'close_reason': 'not a FAIL verdict', 'metadata': {'round': 1}}
        self.assertEqual(report.qa_summary(qa, []), 'R1 UNKNOWN (0 open)')

    def test_finding_relation_not_id_prefix(self):
        finding = {'id': 'unrelated-name', 'dependencies': [{'depends_on_id': 'qa-2', 'type': 'discovered-from'}]}
        self.assertTrue(report.related_bead(finding, 'qa-2', 'discovered-from'))
        self.assertFalse(report.related_bead({'id': 'qa-2-f1'}, 'qa-2', 'discovered-from'))

    def test_dispatch_prioritizes_and_never_assigns_unclassified(self):
        rows = report.dispatch_rows([
            {'id': 'minor', 'priority': 4, 'metadata': {'layer': 3, 'difficulty': 'fast'}},
            {'id': 'blocking', 'priority': 1, 'metadata': {'layer': 2, 'severity': 'blocking', 'difficulty': 'hard'}},
            {'id': 'unknown', 'priority': 2, 'metadata': {'layer': 1}},
        ], [{'identity': 'luna', 'model': 'gpt-6-luna'}, {'identity': 'astra', 'model': 'gpt-6-astra'}], set())
        self.assertEqual([row['id'] for row in rows], ['blocking', 'unknown', 'minor'])
        self.assertEqual(rows[0]['agents'], 'astra')
        self.assertEqual(rows[1]['agents'], 'UNCLASSIFIED')
        self.assertIn('UNCLASSIFIED', report.render_dispatch(rows))

    def test_dispatch_fixture_matches_three_model_classes_and_waits(self):
        members = [
            {'identity': 'luna', 'model': 'gpt-6-luna'},
            {'identity': 'terra', 'model': 'gpt-6-terra'},
            {'identity': 'astra', 'model': 'gpt-6-astra'},
        ]
        ready = [
            {'id': 'normal', 'priority': 2, 'metadata': {'layer': 2, 'difficulty': 'normal'}},
            {'id': 'fast', 'priority': 2, 'metadata': {'layer': 3, 'difficulty': 'fast'}},
            {'id': 'hard', 'priority': 1, 'metadata': {'layer': 4, 'difficulty': 'hard'}},
        ]
        rows = report.dispatch_rows(ready, members, set())
        self.assertEqual([row['id'] for row in rows], ['hard', 'normal', 'fast'])
        self.assertEqual([row['agents'] for row in rows], ['astra', 'terra', 'luna'])
        hard_wait = report.dispatch_rows([ready[2]], members[:1], set())
        self.assertEqual(hard_wait[0]['agents'], 'WAIT')


if __name__ == '__main__':
    unittest.main()
