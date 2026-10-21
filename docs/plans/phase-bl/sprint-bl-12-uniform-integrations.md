# Sprint bl-12 — collector/state/event pipeline integration

Wave 3 integration; must follow bl-3 event requests, bl-4 reviewer metadata,
bl-5 sanity state, and bl-7 display. Controlled inputs prove Jev JSONL and
reviewers enter current persisted state, the TUI displays it, eligible
transitions create debounced aggregated event requests, and a separately
advanced delivery clock sends them. Cron reports the same state when it does
not collect; failed sources are eligible next request. No product path changes.
