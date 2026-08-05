# AK.8 QA follow-up task list

Baseline: `feature/pak-s10-post-write-router-boundary` after the AK.8/AK.9
message-array work and AK.10 boundary closure. This list records the critical
review and implementation decision for every QA finding supplied for the AK.8
follow-up.

| Finding | Severity | Critical review | Decision / acceptance proof | Status |
| --- | --- | --- | --- | --- |
| `RBQA-AK8-F001` | important | `WriteProvenance` was reconstructed from `WriteRequest` fields at each ingress and writer seam, making new provenance fields easy to omit. Test-only value matrices and the inbox-derived ACK check are different domains. | Give `WriteRequest` one public borrowed `provenance()` projection and use it at every request-derived production call site. Retain non-request fixtures/derivations. | complete |
| `RBQA-AK8-F002` | important | `write_mail_with_runtime_impl_with_mode` and peer-array admission had already validated provenance before `prepare_send_context` validated the same values again; peer arrays also repeated the router check at canonical admission. | Thread the validated facts into `prepare_send_context`; validate ordinary peer arrays once at canonical admission and singleton ACK arrays once at their distinct ACK route. | complete |
| `RBQA-AK8-F003` | minor | Local write and peer-ACK routes contained identical `WriteOutcome` to response conversion matches. Their post-commit error policy intentionally differs. | Extract only the common response conversion helper; preserve each route's existing post-commit policy. | complete |
| `RBQA-AK8-F004` | minor | The runtime-root self-send test asserted the same error code twice. | Remove the duplicate assertion. | complete |
| `ATM-QA-002-AK8-doc-conflict` | important | The Phase AK sprint line called AK.8 `feature/pak-s8-peer-message-array-ingress`, while the completed implementation was `feature/ak8-11-peer-message-array`; AK.8 also had no completion marker. | Correct the branch name and mark AK.8 complete, retaining the existing AK.9 bundled-sender note. | complete |
| `RBP-F001-AK8` | important | Peer-array admission reported all array-shape failures as generic validation errors, despite callers needing different corrections. Provenance policy errors already contain a recovery action. | Add recovery text to empty-array, ACK-in-array, dry-run, missing-origin-ID, and duplicate-ID admission failures; retain `MessageValidationFailed` as the stable code. | complete |
| `RBP-F002-AK8` | important | This is the same production request-derived provenance reconstruction issue as `RBQA-AK8-F001`; the apparent count differs because the QA reports counted sites at different revisions. | Closed by the one `WriteRequest::provenance()` projection and all production request-derived call-site replacements. | complete |
| `RSH-001-AK8` | important | `connect_configured_peer` performed synchronous OS DNS resolution before asking whether any request-deadline budget remained. A fresh DNS lookup is not cancellable through the current direct synchronous transport, but an already-expired request must never enter DNS. | Preflight the deadline immediately before DNS and retain per-address checks. Add a source-order regression guard proving the deadline check precedes `to_socket_addrs()`. | complete |

Validation required before handoff: formatter, lint, tests, localhost smoke,
local-IP smoke, and a tagged/pushed review commit.
