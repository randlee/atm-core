import io
import json
import re
import subprocess
import sys
from pathlib import Path
import unittest
from unittest import mock

SKILL = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(SKILL / 'scripts'), str(SKILL.parent / 'atm-beads/scripts')]
import phase_beads as pb
import sprint_dag as dag

ROWS = [{'id': 'atm-x-1.group-dev', 'labels': ['phase-x', 'stage:dev']}]


class FakeCache:
    def __init__(self, rows=ROWS, sync_error=None):
        self.rows, self.sync_error, self.synced = rows, sync_error, []

    def sync(self, phase):
        self.synced.append(phase)
        if self.sync_error:
            raise self.sync_error

    def list(self, phase):
        return list(self.rows)


def unavailable(repo):
    raise pb.bd_cache.CacheError('BD_CACHE.NO_DATABASE', 'no .beads', 'run from the beads repository root')


class PhaseBeadsTests(unittest.TestCase):
    def read(self, factory):
        with mock.patch.object(pb.sys, 'stderr', new_callable=io.StringIO) as stderr:
            return pb.phase_beads(Path('/repo'), 'x', cache_factory=factory), stderr.getvalue()

    def test_syncs_the_phase_then_reads_the_cache(self):
        cache = FakeCache()
        rows, warning = self.read(lambda repo: cache)
        self.assertEqual(cache.synced, ['x'])
        self.assertEqual(rows, ROWS)
        self.assertEqual(warning, '')

    def test_failed_sync_renders_cached_rows_and_says_so(self):
        error = pb.bd_cache.CacheError('BD_CACHE.SYNC_FAILED', 'bd timed out', 'retry')
        rows, warning = self.read(lambda repo: FakeCache(sync_error=error))
        self.assertEqual(rows, ROWS)
        self.assertIn('bd-cache sync failed (BD_CACHE.SYNC_FAILED: bd timed out); rendering from cached rows', warning)

    def test_unavailable_cache_reads_only_the_phase_label_from_bd(self):
        done = subprocess.CompletedProcess([], 0, stdout=json.dumps(ROWS), stderr='')
        with mock.patch.object(pb.subprocess, 'run', return_value=done) as run:
            rows, warning = self.read(unavailable)
        command = run.call_args.args[0]
        self.assertEqual(command[:5], ['bd', 'list', '-l', 'phase-x', '-n'])
        self.assertNotIn('--all', command)
        self.assertEqual(rows, ROWS)
        self.assertIn('bd-cache unavailable (BD_CACHE.NO_DATABASE: no .beads); reading phase-x from bd', warning)

    def test_unavailable_cache_and_failing_bd_is_an_error(self):
        failed = subprocess.CompletedProcess([], 1, stdout='', stderr='dolt down')
        with mock.patch.object(pb.subprocess, 'run', return_value=failed):
            with self.assertRaisesRegex(RuntimeError, 'dolt down'):
                self.read(unavailable)

    def test_empty_phase_is_an_error(self):
        with self.assertRaisesRegex(RuntimeError, 'no phase-x beads'):
            self.read(lambda repo: FakeCache(rows=[]))

    def test_dag_blocked_read_is_scoped_to_the_phase_dev_and_gate_beads(self):
        calls = []
        def run_json(repo, *command):
            calls.append(command)
            return {'events': []} if command[:3] == ('atm', 'task', 'events') else []
        graph = {'nodes': ['atm-x-1.group-dev']}
        with mock.patch.object(dag, 'run_json', side_effect=run_json):
            dag.collect_state(Path('/repo'), graph, {}, {}, 'x')
        blocked = [c for c in calls if c[:2] == ('bd', 'blocked')]
        self.assertEqual(blocked, [('bd', 'blocked', '-l', 'phase-x', '--label-any', 'stage:dev,stage:dev-sanity', '--json')])

    def test_no_report_script_reads_every_bead(self):
        for name in ('sprint-report', 'sprint_dag.py', 'phase_beads.py'):
            text = (SKILL / 'scripts' / name).read_text()
            self.assertIsNone(re.search(r'["\']bd["\'],\s*["\']list["\'],\s*["\']--all["\']', text), name)


if __name__ == '__main__':
    unittest.main()
