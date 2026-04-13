# Code Review CR001 Analysis

## Executive Summary

- Total findings: 4 confirmed, 5 accepted-limitations, 3 false-positives
- Highest-risk confirmed findings:
  - `H-1` path traversal through unsanitized `team` / `agent` segments
  - `M-2` unbounded allocation in `normalize_json_number(...)` for large exponents
  - `C-1` temp-file naming is not collision-proof for concurrent same-path writes in one PID
- Recommended phase scope:
  - Phase 1: fix `H-1` and `M-2` first because they combine untrusted input with filesystem or allocation risk
  - Phase 2: fix `C-1` and `H-2` as small correctness/hardening changes
  - Leave accepted limitations documented unless product requirements change

## Finding Analysis

### C-1 — Temp File Name Collision

**Verdict**: CONFIRMED

**Root cause**: `crates/atm-core/src/persistence.rs` builds temp paths as `.{file}.tmp.{pid}.{timestamp_nanos}`. There is no counter, random suffix, or UUID in the name. `Utc::now().timestamp_nanos_opt().unwrap_or_default()` makes the `None -> 0` fallback explicit, although `Utc::now()` should normally stay in-range. The real issue is that two writes for the same target path from the same PID can still pick the same timestamp-based suffix.

**Risk in practice**: Low-to-moderate. Several call sites are already lock-protected, which narrows the window, but `atomic_write_bytes(...)` is a general helper and the collision prevention is not complete. The repo already depends on `uuid`, so a collision-proof suffix is available without adding a dependency.

**Recommended action**: Switch temp naming to a guaranteed-unique suffix, for example `Uuid::new_v4()` or a per-process atomic counter. Keep the target basename in the temp filename for debuggability, but stop relying on timestamp uniqueness.

### C-2 — Stale Envelope Fallback After Re-Lock

**Verdict**: FALSE-POSITIVE

**Root cause**: `read_mail(...)` re-locks, reloads `source_files`, rebuilds `selected`, and only then constructs `output_messages`. The fallback to `selected_message.envelope` is reached only if the reloaded `source_files` no longer contain the exact `source_path + source_index` tuple that was just rebuilt from the same `source_files`. Under the current implementation that tuple is stable: `apply_display_mutations(...)` mutates envelopes in place and does not reorder or remove elements.

**Risk in practice**: Negligible. The fallback is defensive code, not a real stale-snapshot path under current control flow. If the lookup fails, a deeper invariant has already been broken elsewhere.

**Recommended action**: No bug fix required. Optional cleanup later: replace the fallback with a debug assertion or explicit invariant comment if the team wants the defensive branch to be more obviously unreachable.

### H-1 — Path Traversal Via Unsanitized Team / Agent Names

**Verdict**: CONFIRMED

**Root cause**: `AgentAddress::from_str(...)` in `crates/atm-core/src/address.rs` only rejects empty segments and multiple `@` separators. `team_dir_from_home(...)` and `inbox_path_from_home(...)` in `crates/atm-core/src/home.rs` then join the raw `team` and `agent` strings directly into filesystem paths. There is no downstream sanitization or canonical-root enforcement.

**Risk in practice**: High for correctness and medium for security. ATM is a local CLI, so the attacker model is narrower than a network service, but this still allows crafted input like `../other-team` to escape the intended `.claude/teams` subtree.

**Recommended action**: Add validated newtypes or a shared validator for team/member path segments. Reject path separators, `..`, empty segments, and platform-specific path escapes before any path construction.

### H-2 — Spin Loop After Repeated Successful Stale-Lock Eviction

**Verdict**: CONFIRMED

**Root cause**: In `acquire_send_alert_lock(...)`, the `AlreadyExists` branch does `if evict_stale_send_alert_lock(path) { continue; }` and only sleeps in the `false` branch. If a stale lock can be evicted repeatedly, the loop can re-enter without backoff.

**Risk in practice**: Low, but real. In the common case, one successful eviction is followed by a successful `create_new(...)` and the loop exits. The busy-loop only shows up under unusual churn or adversarial recreation of stale lock files.

**Recommended action**: Add a small sleep or bounded backoff even after successful eviction, or restructure the loop so every `AlreadyExists` turn yields at least once.

### H-3 — PID Reuse In Stale-Lock Detection

**Verdict**: ACCEPTED-LIMITATION

**Root cause**: `process_is_alive(...)` uses PID-only liveness checks (`kill(pid, 0)` on Unix, `OpenProcess(...)` on Windows). This cannot distinguish “the original lock owner is still alive” from “that PID now belongs to some unrelated process.” The review note’s exact failure mode is slightly off: PID reuse causes a false-alive outcome, not a false-dead eviction. ATM will conservatively keep the lock rather than incorrectly evicting a live one.

**Risk in practice**: Low and availability-only. The effect is a stale lock that may survive until manual cleanup or timeout, not corrupted state.

**Recommended action**: Keep the current behavior as an accepted limitation, but document that PID-only stale-lock detection is conservative and may preserve a stale lock if the PID has been reused.

### H-4 — TOCTOU Window Between Lock Drop And Reacquire In `ack_mail`

**Verdict**: ACCEPTED-LIMITATION

**Root cause**: `ack_mail(...)` intentionally drops the initial actor-source lock set before acquiring the full superset that includes the reply inbox. The source already documents the reason in code comments, and `docs/architecture.md` section `18.4.1` explains the two-phase locking pattern and the post-reacquire revalidation.

**Risk in practice**: Low and already mitigated. The unlock gap is real, but the function re-discovers source paths, reloads state, and revalidates pending-ack state before mutating anything.

**Recommended action**: No code change required. Existing architecture documentation is sufficient.

### H-5 — Infallible `team_dir_from_home(...)` / `inbox_path_from_home(...)` Return `Result`

**Verdict**: ACCEPTED-LIMITATION

**Root cause**: The helpers in `crates/atm-core/src/home.rs` are currently infallible after `home_dir` resolution, but they intentionally keep a `Result<PathBuf, AtmError>` shape. The doc comments already state why: callers share one path-construction contract and the helper may grow validation later.

**Risk in practice**: Low. This is API-shape overhead, not a correctness bug.

**Recommended action**: No change required. The existing doc comments already record the rationale.

### M-1 — Blocking Lock Acquisition In Hot Polling Loop

**Verdict**: FALSE-POSITIVE

**Root cause**: The polling path is fully synchronous. `read_mail(...)` is a sync function, `read::wait::wait_for_eligible_message(...)` uses `std::thread::sleep`, and there is no async executor in this call path. The repo has a `tokio` dependency in the workspace, but not in ATM’s `read` execution path.

**Risk in practice**: None under the current architecture. The code blocks a thread, not an async runtime.

**Recommended action**: No fix required. Revisit only if `atm-core` grows an async public API.

### M-2 — Unbounded Allocation In `normalize_json_number(...)`

**Verdict**: CONFIRMED

**Root cause**: `normalize_json_number(...)` builds strings with `"0".repeat(scale as usize)` for non-negative exponents and `"0".repeat((-point_index) as usize)` for large negative scales. The exponent is parsed from user-supplied JSON-number text into `i64`, and there is no size cap before allocation.

**Risk in practice**: Moderate. The panic was already removed, but a large exponent such as `1e1000000000` can still drive extremely large allocations or OOM behavior.

**Recommended action**: Cap the expansion length. If the normalized form would exceed a reasonable bound, return the raw string unchanged and emit a warning instead of allocating.

### M-3 — Full Clone Of Message Vec On Every Poll Tick

**Verdict**: ACCEPTED-LIMITATION

**Root cause**: `selected_after_filters(...)` and `select_messages(...)` clone vectors in the polling path (`messages.to_vec()`). This keeps the selection pipeline simple and ownership-friendly, but it does duplicate work every 100ms while waiting.

**Risk in practice**: Low. ATM mailbox surfaces are local and usually small; this is a throughput trade-off, not a correctness defect. If very large mailboxes become normal, the cost could become noticeable.

**Recommended action**: Keep as-is for now. Consider borrowing-based filter/selection helpers only if real mailbox sizes make polling costs measurable.

### M-4 — Dedup-Before-Append Crash Window For Team-Lead Repair Notification

**Verdict**: ACCEPTED-LIMITATION

**Root cause**: `notify_team_lead_missing_config(...)` records the dedup key through `register_missing_team_config_alert(...)` before appending the notice to the `team-lead` inbox. A crash in between can permanently suppress that notification until the config is restored and the dedup state is cleared.

**Risk in practice**: Low and aligned with the documented delivery model. The requirements and repair docs already say these notifications are best-effort and deduplicated, not guaranteed delivery.

**Recommended action**: Keep the current behavior, but document the implied at-most-once semantics explicitly so operators understand that crash recovery favors duplicate suppression over guaranteed resend.

### M-5 — Inconsistent Sort Keys Across Origin Inbox Discovery

**Verdict**: FALSE-POSITIVE

**Root cause**: `discover_origin_inboxes(...)` sorts the origin-only list with `paths.sort()`, but `discover_source_paths(...)` immediately re-sorts the combined primary+origin list with `sort_by_key(|path| path.to_string_lossy().into_owned())`. The externally consumed ordering is therefore the second sort, not the first.

**Risk in practice**: Negligible. For ATM-created paths, the final ordering is stable and the first sort is just redundant work. The two functions do not expose conflicting orderings for the same final result.

**Recommended action**: No correctness fix required. Optional cleanup later: remove the redundant first sort or align both helpers to one canonical ordering style.

## Fix Plan

Confirmed findings only:

1. `H-1` — Path-segment validation for team/agent names
   - Effort: medium
   - Suggested sprint grouping: Security and path-hardening sprint
   - Dependencies: none

2. `M-2` — Cap or short-circuit large exponent normalization
   - Effort: small
   - Suggested sprint grouping: Observability hardening sprint
   - Dependencies: none

3. `C-1` — Collision-proof temp-file naming in persistence helpers
   - Effort: small
   - Suggested sprint grouping: Filesystem durability / persistence sprint
   - Dependencies: none

4. `H-2` — Add backoff after successful stale-lock eviction
   - Effort: trivial
   - Suggested sprint grouping: Send-alert lock hardening sprint
   - Dependencies: none

Recommended grouping:

- Sprint A: `H-1` + `M-2`
  - highest user-controlled input risk
- Sprint B: `C-1` + `H-2`
  - low-risk hardening changes with small code surface

No dependency chain blocks those sprints from running independently, but Sprint A should land first because it closes the most externally exposed risk.
