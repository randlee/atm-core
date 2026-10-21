# Sprint bl-12 — collector/state/event pipeline integration

Wave 3 integration; must follow bl-3 event requests, bl-4 reviewer metadata,
bl-5 sanity state, and bl-7 display. Controlled inputs prove Jev JSONL and
reviewers enter current persisted state, the TUI displays it, eligible
transitions create debounced aggregated event requests, and a separately
advanced delivery clock sends them. Cron reports the same state when it does
not collect; failed sources are eligible next request. No product path changes.

## Relations, scope, and owned paths

Own new pipeline integration fixtures/tests only. It consumes bl-3 delivery
history, bl-4 reviewers, bl-5 sanity state and bl-7 display; it edits none of
their producer paths.

## Authoritative deliverables, acceptance, and validation

For every Jev error kind, controlled JSONL flows through sanity state to the
do-not-enter display; reviewer metadata reaches the Quality pane unchanged.
Controlled transitions create aggregated/debounced requests and only an
advanced delivery clock sends them. Cron/TUI equality and failed-source retry
are asserted from persisted records. Run deterministic fixture tests and lint.
