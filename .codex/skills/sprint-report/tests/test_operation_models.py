import importlib.machinery
import importlib.util
from pathlib import Path
import subprocess
import sys
from types import ModuleType
import unittest
from collections.abc import Mapping
from typing import Literal, TypedDict, cast
from unittest import mock

SKILLS = Path(__file__).resolve().parents[2]
BEADS_SCRIPTS = SKILLS / 'atm-beads/scripts'
REPORT_SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
sys.path[:0] = [str(BEADS_SCRIPTS), str(REPORT_SCRIPTS),
                str(SKILLS / 'atm-bd-orchestration/scripts')]
import bead_formulas
import operation_models as models
JsonValue = models.JsonValue

loader = importlib.machinery.SourceFileLoader('sprint_report_models', str(REPORT_SCRIPTS / 'sprint-report'))
spec = importlib.util.spec_from_loader(loader.name, loader)
assert spec is not None
report = importlib.util.module_from_spec(spec)
loader.exec_module(report)


class OperationModelTests(unittest.TestCase):
    def test_typed_dict_annotation_kinds_accept_valid_and_reject_wrong_types(self):
        class Nested(TypedDict):
            count: int

        class Probe(TypedDict):
            value: object

        annotations = cast(dict[str, object], Probe.__annotations__)
        cases = [
            ('str', str, 'text', 3),
            ('int excludes bool', int, 3, True),
            ('bool excludes int', bool, True, 1),
            ('float excludes int', float, 2.5, 2),
            ('Literal', Literal['ready', 1], 'ready', True),
            ('union and Optional', int | None, None, 'none'),
            ('list[X]', list[str], ['a', 'b'], ['a', 2]),
            ('nested TypedDict', Nested, {'count': 2}, {'count': True}),
            ('Mapping[str, X]', Mapping[str, int], {'a': 2}, {1: 2}),
            ('recursive JsonValue alias', models.JsonValue,
             {'items': [None, True, 3, 2.5, 'text', {'nested': ['end']}]}, {'items': [object()]}),
        ]
        for label, annotation, valid, invalid in cases:
            with self.subTest(annotation=label):
                annotations['value'] = annotation
                self.assertTrue(models.matches_typeddict({'value': valid}, Probe))
                self.assertFalse(models.matches_typeddict({'value': invalid}, Probe))

    def test_existing_typed_dict_models_accept_valid_fixtures(self):
        cases = [
            (models.is_dependency_record, {'type': 'blocks', 'depends_on_id': 'atm-x-1'}),
            (models.is_bead_metadata, {'branch': 'sprint/x-1', 'round': 2}),
            (models.is_bead_record, {'id': 'atm-x-1', 'status': 'open'}),
            (models.is_atm_member, {'identity': 'dev'}),
            (models.is_atm_task, {'task_id': 'task-1'}),
            (models.is_atm_task_event, {'event': 'completed'}),
            (models.is_pull_request_ref, {'ref': 'main'}),
            (models.is_pull_request_check, {'state': 'SUCCESS'}),
            (models.is_pull_request, {'number': 1, 'state': 'OPEN'}),
        ]
        for guard, fixture in cases:
            with self.subTest(model=guard.__name__):
                self.assertTrue(guard(fixture))

    def test_typed_dict_guards_preserve_unknown_keys_and_optional_fields(self):
        self.assertTrue(models.is_bead_metadata({'branch': 'main', 'future_field': {'x': 1}}))
        self.assertTrue(models.is_bead_record({'id': 'atm-x-1', 'status': 'open', 'future_field': True}))

    def test_matches_typeddict_resolves_unregistered_schema_module_globals(self):
        namespace = ModuleType('dynamic_schema').__dict__
        namespace.update(TypedDict=TypedDict, JsonValue=models.JsonValue, matches=models.matches_typeddict)
        exec('class DynamicRecord(TypedDict):\n    payload: JsonValue\n'
             'def check(value): return matches(value, DynamicRecord)', namespace)
        self.assertNotIn(namespace['DynamicRecord'].__module__, sys.modules)
        self.assertTrue(namespace['check']({'payload': {'nested': [1, True, None]}}))

    def test_bd_records_validate_operation_fields_and_dependencies(self):
        rows = [{
            'id': 'atm-x-1.group-dev',
            'status': 'open',
            'labels': ['phase-x', 'stage:dev'],
            'dependencies': [{'type': 'blocks', 'depends_on_id': 'atm-x-1'}],
            'metadata': {'branch': 'sprint/x-1', 'layer': 1},
        }]
        self.assertTrue(models.is_bead_records(rows))
        self.assertFalse(models.is_bead_records([{'id': 'atm-x-1', 'status': 3}]))
        self.assertFalse(models.is_bead_records([{
            'id': 'atm-x-1', 'dependencies': [{'type': 4, 'depends_on_id': 'atm-x-2'}],
        }]))
        self.assertTrue(models.is_bead_records([{
            'id': 'atm-x-1', 'status': 'open',
            'dependencies': [{'dependency_type': 'blocks', 'id': 'atm-x-2'}],
        }]))
        self.assertFalse(models.is_bead_records([{
            'id': 'atm-x-1', 'status': 'open', 'dependencies': [{'type': 'blocks'}],
        }]))
        self.assertFalse(models.is_bead_records([{'id': 'atm-x-1', 'metadata': {'branch': 8}}]))

    def test_bead_records_accept_the_historical_closed_qa_shape(self):
        # Closed QA beads poured before the link step filled branch/pr_target
        # keep null placeholders forever (atm-bd-orchestration#631); older
        # stack beads carry an integer stack number. `bd list` returns them
        # all, so the guard must read them without rewriting closed beads.
        historical = {
            'id': 'atm-bg-1.group-qa-f1-qa', 'status': 'closed',
            'labels': ['phase-bg', 'stack:phase-bg', 'stage:qa'],
            'metadata': {'branch': None, 'checked_bead': 'atm-bg-1.group-qa-f1', 'commit': None,
                         'difficulty': 'normal', 'layer': None, 'phase': 'bg', 'pr_target': None,
                         'quick_fix': False, 'round': 1, 'sprint': 'bg-1', 'stack': 'phase-bg'},
        }
        numbered_stack = {'id': 'atm-phase-bg.stack', 'status': 'closed', 'metadata': {'stack': 611}}
        current = {'id': 'atm-x-2-qa', 'status': 'open',
                   'metadata': {'branch': 'sprint/x-2', 'pr_target': 'integrate/phase-x', 'stack': 'phase-x'}}
        self.assertTrue(models.is_bead_record(historical))
        self.assertTrue(models.is_bead_record(numbered_stack))
        self.assertTrue(models.is_bead_records([historical, numbered_stack, current]))
        # Wrong types are still rejected: only null and the integer stack are historical.
        self.assertFalse(models.is_bead_metadata({'branch': 8}))
        self.assertFalse(models.is_bead_metadata({'pr_target': ['integrate/phase-x']}))
        self.assertFalse(models.is_bead_metadata({'stack': 1.5}))

    def test_bd_show_returns_a_validated_record(self):
        process = subprocess.CompletedProcess([], 0, stdout='[{"id":"atm-x-1","status":"open"}]', stderr='')
        with mock.patch.object(bead_formulas.Bd, 'run', return_value=process):
            record = bead_formulas.Bd().show('atm-x-1')
        self.assertEqual(record, {'id': 'atm-x-1', 'status': 'open'})

        malformed = subprocess.CompletedProcess([], 0, stdout='[{"status":"open"}]', stderr='')
        with mock.patch.object(bead_formulas.Bd, 'run', return_value=malformed):
            with self.assertRaisesRegex(ValueError, 'invalid bead records'):
                bead_formulas.Bd().show('atm-x-1')

    def test_report_adapters_reject_mismatched_operation_records(self):
        bead = {'id': 'atm-x-1', 'status': 'open', 'metadata': {'sprint': 'x-1'}}
        with mock.patch.object(report, 'run_json', return_value={'beads': [bead]}):
            self.assertEqual(report.run_bead_records(Path('/repo'), 'beads', 'bd', 'show', 'atm-x-1', '--json'), [bead])
        with mock.patch.object(report, 'run_json', return_value={'tasks': [{'task_id': 'task-1', 'state': 'active'}]}):
            self.assertEqual(report.run_atm_tasks(Path('/repo'), 'atm', 'task', 'list', '--json'),
                             [{'task_id': 'task-1', 'state': 'active'}])
        with mock.patch.object(report, 'run_json', return_value={'events': [{'event': 'completed', 'at': '2026-10-28T00:00:00Z'}]}):
            self.assertEqual(report.run_atm_task_events(Path('/repo'), 'atm', 'task', 'events', 'task-1', '--json'),
                             [{'event': 'completed', 'at': '2026-10-28T00:00:00Z'}])
        pull_request = {'number': 7, 'state': 'OPEN', 'head': {'ref': 'fix/example'}, 'base': {'ref': 'main'}}
        with mock.patch.object(report, 'run_json', return_value=[[pull_request]]):
            self.assertEqual(report.run_pull_request_pages(Path('/repo'), 'gh', 'api', '--paginate'), [[pull_request]])

        with mock.patch.object(report, 'run_json', return_value={'members': [{'name': 'dev', 'agent_type': 5}]}):
            with self.assertRaisesRegex(RuntimeError, 'ATM member records'):
                report.run_atm_members(Path('/repo'), 'atm', 'members', '--json')
        with mock.patch.object(report, 'run_json', return_value={'events': [{'event': 2}]}):
            with self.assertRaisesRegex(RuntimeError, 'ATM task-event records'):
                report.run_atm_task_events(Path('/repo'), 'atm', 'task', 'events', 'task-1', '--json')
        with mock.patch.object(report, 'run_json', return_value=[[{'number': 7, 'state': 'OPEN', 'head': {'ref': 9}}]]):
            with self.assertRaisesRegex(RuntimeError, 'pull-request pages'):
                report.run_pull_request_pages(Path('/repo'), 'gh', 'api', '--paginate')

    def test_atm_and_github_operation_models_check_required_identity_and_nested_shapes(self):
        self.assertTrue(models.is_atm_tasks([{'task_id': 'atm-x-1', 'state': 'active'}]))
        self.assertFalse(models.is_atm_tasks([{'state': 'active'}]))
        self.assertTrue(models.is_atm_task_events([{'event': 'completed', 'at': '2026-10-28T00:00:00Z'}]))
        self.assertFalse(models.is_atm_task_events([{'to_state': 'completed'}]))
        self.assertTrue(models.is_pull_requests([{
            'number': 9, 'state': 'OPEN', 'head': {'ref': 'fix/example'},
            'base': {'ref': 'main'}, 'merged_at': None,
        }]))
        self.assertFalse(models.is_pull_requests([{
            'number': 9, 'state': 'OPEN', 'head': {'ref': 1}, 'base': {'ref': 'main'},
        }]))


if __name__ == '__main__':
    unittest.main()
