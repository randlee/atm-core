# Issue #98 Fix Plan: Post-Send Hook Skip-Noise Demotion

## Scope

This plan covers a targeted behavior change in `crates/atm-core/src/send/hook.rs`
for issue `#98`: do not emit a warn-level "post-send hook skipped" message when
only one hook filter axis is configured and that configured axis simply does not
match the current send.

The deliverable is planning only. No implementation is included here.

## Root Cause

In `maybe_run_post_send_hook(...)`, the current skip branch is:

1. If both `post_send_hook_senders` and `post_send_hook_recipients` are empty:
   emit `debug!` and return.
2. Otherwise, if both `hook_match.sender` and `hook_match.recipient` are false:
   emit `warn!` and push a warning string to the caller.

That second branch is too broad. It treats these two situations the same:

- genuine two-axis misconfiguration:
  both sender and recipient filters are configured, and neither matched
- expected single-axis non-match:
  only one filter axis is configured, and that configured axis did not match

Issue `#98` is the second case. That should be silent at the CLI warning layer.

## Planned Code Change

File: `crates/atm-core/src/send/hook.rs`
Function: `maybe_run_post_send_hook`

### Current decision shape

```rust
if !hook_match.sender && !hook_match.recipient {
    if !sender_filters_configured && !recipient_filters_configured {
        debug!(... "post-send hook disabled because no sender or recipient filters are configured");
        return;
    }

    let warning = format_post_send_hook_skipped_warning(...);
    warn!(... "post-send hook skipped");
    warnings.push(warning);
    return;
}
```

### Planned restructure

Keep the current "both lists empty" fast path as-is. Change the remaining skip
branch so warn-level output only happens when both filter axes are configured and
both configured axes miss.

Minimal shape:

```rust
if !sender_filters_configured && !recipient_filters_configured {
    debug!(... "post-send hook disabled because no sender or recipient filters are configured");
    return;
}

if !hook_match.sender && !hook_match.recipient {
    if sender_filters_configured && recipient_filters_configured {
        let warning = format_post_send_hook_skipped_warning(...);
        warn!(... "post-send hook skipped");
        warnings.push(warning);
    } else {
        debug!(... "post-send hook skipped because configured filter did not match");
    }
    return;
}
```

### Intended behavior after the change

- both lists empty:
  `debug!`, no user-visible warning, no hook execution
- sender-only configured and sender does not match:
  `debug!`, no user-visible warning, no hook execution
- recipient-only configured and recipient does not match:
  `debug!`, no user-visible warning, no hook execution
- both sender and recipient configured and neither matches:
  keep current `warn!` plus `warnings.push(...)`
- either axis matches:
  existing hook execution path unchanged

This is the smallest change that fixes the noise without changing matching
semantics or execution semantics.

## Existing `crates/atm/tests/send.rs` Tests Affected

The existing CLI-level hook-skip warning coverage is:

1. `test_send_emits_post_send_hook_skip_warning_when_no_filter_matches`
   - keep
   - this is the genuine two-axis mismatch case
   - still expected to emit the warn-level skip message

2. `test_send_emits_post_send_hook_skip_warning_on_stderr_in_json_mode`
   - keep
   - same genuine two-axis mismatch, but asserts stderr behavior under `--json`

3. `test_send_skip_warning_marks_unconfigured_axis_explicitly`
   - change or replace
   - this test currently encodes the noisy behavior from issue `#98`
   - after the fix, this case should no longer emit a stderr warning

Related nearby coverage that should remain unchanged:

- `test_send_runs_post_send_hook_when_recipient_matches_filter`
- `test_send_runs_post_send_hook_when_sender_filter_is_wildcard`
- `test_send_runs_post_send_hook_when_recipient_filter_is_wildcard`
- `test_send_does_not_run_post_send_hook_when_filter_lists_are_empty`

## New Test Needed

Add one CLI regression test in `crates/atm/tests/send.rs`:

### Recipient-only filter, non-matching recipient, no warning

Suggested name:

`test_send_recipient_only_hook_filter_non_match_is_silent`

Suggested setup:

- configure:
  - `post_send_hook = [...]`
  - `post_send_hook_recipients = ['quality-mgr']`
  - omit `post_send_hook_senders`
- send to `recipient@atm-dev`

Assertions:

- command succeeds
- hook payload file is not created
- stderr is empty
- inbox still receives the sent message

This should replace the old expectation in
`test_send_skip_warning_marks_unconfigured_axis_explicitly`.

## Helper Assessment

### `format_post_send_hook_skipped_warning(...)`

No removal is required.

Recommended plan:

- keep the helper
- keep its current string format
- only call it from the narrowed warn-path where both filter axes are configured

Reasoning:

- the helper still serves the genuine two-axis mismatch case
- the issue is not the warning text itself; it is the branch that decides when
  to emit that warning
- removing the helper would not simplify the fix materially

Optional cleanup, not required for the fix:

- update the helper comment or callsite comment to state it is only used for
  genuine configured-filter mismatch diagnostics

### `display_filter_list(...)`

Keep it unchanged.

Reasoning:

- it is still useful for debug logging
- it is still covered by unit tests
- `(not configured)` remains a useful debug/log rendering even if it is no
  longer surfaced in the CLI warning string for the issue `#98` case

## Non-Goals

- no change to hook matching semantics
- no change to wildcard behavior
- no change to hook execution payload (`ATM_POST_SEND`)
- no change to hook timeout/failure handling
- no change to sender-only or recipient-only successful match behavior

## Validation To Run During Implementation

- targeted: `cargo test --workspace`
- targeted: `cargo clippy --workspace --all-targets -- -D warnings`
- confirm the updated CLI test coverage:
  - two-axis mismatch still warns
  - recipient-only non-match is silent
