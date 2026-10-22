# Sprint bl-1 — Phase BL contract

closure_type: contract; target_boundary: PhaseBlContract; branch: sprint/bl-1-phase-contract; PR target: integrate/phase-bl; wave: 1; external_ref: none; recommended_agent: arch-ctm; recommended_model: opus; must_follow: none; parallel_safe: bl-2, bl-3, bl-4, bl-5, bl-6, bl-7, bl-8, bl-10, bl-13, bl-14.

Own `apps/atm-bd-console/src/atm_bd_console/{types,codec,registry}.py`, versioned JSON fixtures, and their three test modules. This is the only shared contract owner. Every listed type, codec fixture, registry export, and test double lands production-ready; collection policy, UI, transitions, and delivery do not land here.

## Governing text (verbatim)

> "my expectation for 'atm-bd cron' and 'atm-bd console' is that both are run on a specific phase. They both 'schedule' a info refresh which a) is exactly the same regardless of which system instigated it. b) is checked for duplication so running both does not DOUBLE the query load on bd/dolt, atm, github c) A failed run which did not produce successful data does NOT cause a run to be skipped."

> "we do NOT need to collect data from all sources at the same rate."

## Deliverables

1. Publish the importable cross-package contract at `apps.atm_bd_console.types`; plugin code consumes its versioned JSON fixtures through `apps.atm_bd_console.codec`, never a console implementation import. The registry exports a fixture-reading `CollectorRequestDouble` for every layer test.
2. Define the complete frozen-data contract and source error kinds. Windows are contractual: BD/dolt 60 seconds, ATM 60 seconds, GitHub 600 seconds. Lock, timer, and force behaviour are implementation work owned by bl-8.

```python
from dataclasses import dataclass
from enum import Enum
from typing import Any, Mapping, Protocol

class SourceErrorKind(str, Enum): TIMEOUT = "timeout"; RATE_LIMIT = "rate_limit"; API_FAILURE = "api_failure"
class RequestState(str, Enum): PENDING = "pending"; ACKNOWLEDGED = "acknowledged"; RETRY = "retry"
class TransitionName(str, Enum): ENTER = "enter"; EXIT = "exit"; UPDATE = "update"
class DeliveryOutcome(str, Enum): SENT = "sent"; FAILED = "failed"; RETRY = "retry"
@dataclass(frozen=True)
class SourceStamp: last_success: float | None; last_attempt: float | None; last_error: SourceErrorKind | None; since: float | None
@dataclass(frozen=True)
class CollectorRecord: source: str; payload: Mapping[str, Any] | None; stamp: SourceStamp
@dataclass(frozen=True)
class ActionableProblem: check: str; branch: str | None; sprint_labels: tuple[str, ...]; source_status: SourceErrorKind | None
@dataclass(frozen=True)
class HeldRow: bead: str; task: str; reason: str; source_timestamp: float
@dataclass(frozen=True)
class QaRunInput: reviewers: tuple[Mapping[str, Any], ...]
@dataclass(frozen=True)
class JevInput: lines: tuple[Mapping[str, Any], ...]
@dataclass(frozen=True)
class DagInput: nodes: tuple[Mapping[str, Any], ...]
@dataclass(frozen=True)
class StateSnapshot: phase_id: str; records: tuple[CollectorRecord, ...]; content_at: float | None; carried_reason: str | None; problems: tuple[ActionableProblem, ...]; held_rows: tuple[HeldRow, ...]; qa: QaRunInput; jev: JevInput; dag: DagInput
@dataclass(frozen=True)
class Transition: name: TransitionName; from_state: str; to_state: str; previous: StateSnapshot | None
@dataclass(frozen=True)
class EventRequest: request_id: str; transition: Transition; recipients: tuple[str, ...]; payload: Mapping[str, Any]; created_at: float; due_at: float; debounce_key: str; attempt_state: RequestState
@dataclass(frozen=True)
class DeliveryHistoryEntry: request_id: str; outcome: DeliveryOutcome; attempt_state: RequestState; at: float; error: str | None
class CollectorRequest(Protocol):
    def request(self, source: str, *, force: bool, clock: float) -> CollectorRecord: ...
class EventEnqueue(Protocol):
    def enqueue_event_request(self, request: EventRequest) -> Mapping[str, Any]: ...
class EventSender(Protocol):
    def send(self, request: EventRequest) -> DeliveryOutcome: ...
class ProblemDeriver(Protocol):
    def derive_problem_records(self, records: tuple[CollectorRecord, ...]) -> tuple[ActionableProblem, ...]: ...
BD_WINDOW_S = 60; ATM_WINDOW_S = 60; GITHUB_WINDOW_S = 600
```

3. Supply versioned round-trip codec fixtures for `CollectorRecord`, `StateSnapshot`, held rows, QA/Jev/DAG inputs, `Transition`, and `EventRequest`; registry doubles cover every consumer boundary.
4. `atm_bd_console` is the real Python import path. Plugin scripts never import console implementation modules: they call `atm-bd-console enqueue-event --request <json>` and receive `{\"status\":\"acknowledged|rejected\",\"request_id\":...,\"error\":...}`; fixture/CLI tests cover enqueue, sender, and deriver doubles.

## Deterministic acceptance

- [ ] `test_contract_types_have_exact_fields_enums_and_import_path`
- [ ] `test_contract_codec_round_trips_versioned_json_fixtures`
- [ ] `test_plugin_fixture_decodes_without_console_implementation_import`
- [ ] `test_contract_registry_exports_collector_request_double`
- [ ] `test_contract_windows_and_source_error_kinds_are_stable`
- [ ] `test_public_types_construct_without_delivery_import`

Run frozen ruff, pyright, and pytest with fixtures and controlled values only. No sleep, timing, race, or live-service test is permitted.

## Validation
```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q
```

## This sprint does not close

bl-8 implements locks/timers/force and snapshot construction; bl-10 implements pure problem derivation; bl-3 owns durable queue/delivery; bl-12 owns the integrated pipeline.
