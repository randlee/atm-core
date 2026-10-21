# Sprint bl-1 — phase BL contract

closure_type: contract; target_boundary: PhaseBlContract; branch:
sprint/bl-1-phase-contract; PR target: integrate/phase-bl; wave 1; external_ref:
#379,#164,#365,#497,#375,#372,#491,#359,#360,#347; recommended_agent:
arch-ctm; recommended_model: opus. must_follow: none. parallel_safe: every
wave-2 sprint, because this sprint owns all shared files.

Own types.py, codec.py, registry.py and test_types.py, test_codec.py,
test_registry.py under apps/atm-bd-console. Every deliverable lands as a
stable import contract, JSON codec, fixture, registry export, and test; no
implementation, UI, policy, or delivery belongs here.

1. Publish apps.atm_bd_console.types. Exact signatures: SourceStamp(last_success,
last_attempt,last_error,since); SourceValue(source,payload,content_at);
CollectorRecord(value,stamp); ActionableProblem(check,branch,sprint_labels,
source_status); StateSnapshot(records,content_at,carried_reason,problems);
Transition(name,from_state,to_state); EventRequest(transition,recipients,
payload,created_at,debounce_key); DeliveryHistoryEntry(request_id,outcome,at,
error); LlmReply(verdict,text); SelectedEvaluation(verdict,source);
IterationFailure(kind,detail); and TriageRoute(queue,reason). All are frozen
dataclasses; codec JSON schemas cover the exchanged records.
2. registry.py exports every listed type; versioned fixtures are the codec
compatibility source of truth.
3. Define request(source, force, clock) -> CollectorRecord | InProgress.
force bypasses only due eligibility, never a held per-collector lock; a held
lock returns InProgress/result seam and never duplicates a call. The collector
owns its poll timer: success resets it; failure remains due.

- [ ] test_contract_types_have_exact_fields_and_import_path
- [ ] test_contract_codec_round_trips_versioned_json_fixtures
- [ ] test_contract_registry_exports_every_shared_type
- [ ] test_force_request_respects_lock_and_result_seam
- [ ] test_success_resets_own_timer_failure_stays_due

Run frozen ruff, pyright and pytest. Tests use controlled clocks/fakes. bl-12
alone closes end-to-end behaviour.
