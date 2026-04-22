# Issue #98 Fix Plan: Hook Command Resolution And Skip-Noise Demotion

## Scope

This plan covers two related post-send-hook issues:

1. primary: hook command resolution is surprising and incorrect for bare binary
   names such as `["bash", "-c", ...]`
2. secondary: ATM emits a warn-level "post-send hook skipped" message when only
   one or both hook filter axes do not match the current send, even though that
   is expected behavior rather than an error

The deliverable is planning only. No implementation is included here.

## Priority Correction

The operator priority is:

1. fix hook command resolution so normal executable names work without forcing
   `"/bin/bash"`-style absolute-path workarounds
2. then fix the warn-level skip-noise behavior so non-match is silent to the
   caller

The warning-noise issue is real, but it is not the primary usability problem.
The more important product failure is that a natural hook configuration like
`post_send_hook = ["bash", "-c", "..."]` currently fails because ATM rewrites
`"bash"` to `{config_root}/bash`.

## Problem A: Hook Command Resolution

In `resolve_command_path(...)`, the current command-resolution rule is too
aggressive:

- absolute path: use as-is
- everything else: join under `config_root`

That breaks natural hook configurations such as:

```toml
post_send_hook = ["bash", "-c", "..."]
```

because ATM rewrites `bash` to `{config_root}/bash` and fails with `ENOENT`
instead of using normal `PATH` lookup.

## Planned Change A

File: `crates/atm-core/src/send/hook.rs`
Function: `resolve_command_path`

### Planned resolution rules

```rust
if path.is_absolute() {
    use as-is
} else if command_path contains a path separator {
    resolve relative to config_root
} else {
    keep the bare command name unchanged
}
```

This preserves both intended config styles:

```toml
post_send_hook = ["scripts/tmux-nudge.sh", ...]
post_send_hook = ["bash", "-c", "..."]
```

### Tests needed for Problem A

Add unit coverage in `crates/atm-core/src/send/hook.rs` for:

- absolute command path is unchanged
- relative path with a separator resolves under `config_root`
- bare command name does not resolve under `config_root`

Add integration coverage in `crates/atm/tests/send.rs` for:

- bare-binary hook command such as `["sh", "-c", "..."]` or `["bash", "-c", "..."]`
  launches successfully when available on `PATH`
- existing relative-script hook fixture behavior still works

### UX/docs updates required for Problem A

- update `.atm.toml` hook docs to explain path-like vs bare-command behavior
- improve the spawn failure guidance so it distinguishes:
  - bad absolute/relative path
  - missing `PATH`-resolved program

## Problem B: Skip Warning Noise

In `maybe_run_post_send_hook(...)`, the current skip branch is:

1. If both `post_send_hook_senders` and `post_send_hook_recipients` are empty:
   emit `debug!` and return.
2. Otherwise, if both `hook_match.sender` and `hook_match.recipient` are false:
   emit `warn!` and push a warning string to the caller.

That second branch is too broad. It treats these two situations the same:

- configured filters did not match, which is expected behavior
- actual hook execution failure, which is the only case that should warn

The concrete issue `#98` reproducer is the single-axis partial-config case:
`post_send_hook_recipients` is configured, `post_send_hook_senders` is
unconfigured/empty, and a send to a different recipient still emits a
caller-visible warning even though the hook was never scheduled to run.

Issue `#98` is the first case. Non-match should be silent at the CLI warning
layer and visible only in debug diagnostics.

## Planned Change B

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
branch so any filter non-match path is debug-only and does not push a warning to
the caller.

Minimal shape:

```rust
if !sender_filters_configured && !recipient_filters_configured {
    debug!(... "post-send hook disabled because no sender or recipient filters are configured");
    return;
}

if !hook_match.sender && !hook_match.recipient {
    debug!(... "post-send hook did not match configured sender/recipient filters");
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
  `debug!`, no user-visible warning, no hook execution
- either axis matches:
  existing hook execution path unchanged

This keeps matching/execution semantics unchanged while reserving warnings for
actual execution failures only.

## Existing `crates/atm/tests/send.rs` Tests Affected

The existing CLI-level hook-skip warning coverage is:

1. `test_send_emits_post_send_hook_skip_warning_when_no_filter_matches`
   - change
   - this currently asserts warning behavior for non-match
   - after the fix, it should assert no stderr warning

2. `test_send_emits_post_send_hook_skip_warning_on_stderr_in_json_mode`
   - change
   - after the fix, it should assert no stderr warning in `--json` mode

3. `test_send_skip_warning_marks_unconfigured_axis_explicitly`
   - change or replace
   - this also currently encodes noisy non-match behavior
   - after the fix, this case should no longer emit a stderr warning

Related nearby coverage that should remain unchanged:

- `test_send_runs_post_send_hook_when_recipient_matches_filter`
- `test_send_runs_post_send_hook_when_sender_filter_is_wildcard`
- `test_send_runs_post_send_hook_when_recipient_filter_is_wildcard`
- `test_send_does_not_run_post_send_hook_when_filter_lists_are_empty`

## New Test Needed

Add one CLI regression test in `crates/atm/tests/send.rs`:

### Non-match hook paths are silent

Suggested tests:

- `test_send_recipient_only_hook_filter_non_match_is_silent`
- `test_send_two_axis_hook_filter_non_match_is_silent`
- `test_send_no_hook_skip_warning_on_stderr_in_json_mode`

Suggested coverage:

- recipient-only configured, non-matching recipient
- sender+recipient configured, neither matches
- JSON mode stays silent too

Assertions:

- command succeeds
- hook payload file is not created
- stderr is empty
- inbox still receives the sent message

These should replace the old warning expectations.

## Helper Assessment

### `format_post_send_hook_skipped_warning(...)`

Removal is recommended.

Recommended plan:

- remove the helper if hook non-match no longer creates caller-visible warnings
- if a debug-only formatted message is still desired, either:
  - keep the helper but make it debug-only, or
  - inline the debug message and delete the helper

Reasoning:

- once non-match is no longer a caller-visible warning, the helper may become
  unnecessary
- keeping dead warning-formatting surface would be needless complexity
- implementation must also remove or rewrite the existing unit test at
  `crates/atm-core/src/send/hook.rs:445-457`, which currently asserts the
  helper's warning template and will otherwise fail to compile if the helper is
  deleted

### `display_filter_list(...)`

Keep it unchanged.

Reasoning:

- it is still useful for debug logging
- it is still covered by unit tests
- `(not configured)` remains a useful debug/log rendering even if it is no
  longer surfaced in the CLI warning string for the issue `#98` case

## Related Hook Command Resolution Issue

During investigation, `team-lead` reported a separate but closely related DX
failure from the raptor team:

- `post_send_hook = ["bash", "-c", "..."]` failed with `ENOENT`
- ATM resolved the first argv element relative to `config_root`
- `"bash"` therefore became `{config_root}/bash` instead of using `PATH`

The operational workaround was to use `"/bin/bash"`, but that is not an
acceptable product fix on its own.

### Required plan note

The issue `#98` implementation should explicitly record that:

- absolute paths must continue to work
- relative script paths such as `["scripts/tmux-nudge.sh", ...]` must continue
  to resolve relative to `config_root`
- bare executable names such as `["bash", ...]`, `["python3", ...]`, or
  `["tmux", ...]` should be treated as program names and resolved via `PATH`,
  not rewritten to `{config_root}/{binary}`

### Recommended product fix direction

Adjust `resolve_command_path(...)` so it distinguishes between:

- absolute paths:
  use as-is
- relative paths containing a path separator:
  resolve relative to `config_root`
- bare command names with no path separator:
  pass through unchanged so `Command::new(...)` uses normal `PATH` lookup

That gives the expected behavior for both config styles:

```toml
post_send_hook = ["scripts/tmux-nudge.sh", ...]
post_send_hook = ["bash", "-c", "..."]
```

### Follow-up validation/tests to include when implemented

- relative script path resolves from `config_root`
- bare binary name uses `PATH` resolution and does not join `config_root`
- startup error text explains the failing hook path/command clearly when launch
  still fails

### Relationship to issue `#98`

This is not the same bug as the warn-noise problem, but it is the higher-priority
operator issue around post-send hooks. The implementation must not cement the
`"/bin/bash"` workaround as intended behavior.

## Documentation Updates Required During Implementation

Update these docs together when implementing:

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `README.md`
- `crates/atm/src/commands/send.rs` help text
- `docs/atm-error-codes.md`
  - retire `ATM_WARNING_HOOK_SKIPPED` in section 5.8.2 for this change
  - section 5.8.2 must no longer describe hook filter non-match as a
    user-visible warning/stderr path
  - actual caller-visible hook warnings remain under
    `ATM_WARNING_HOOK_EXECUTION_FAILED` in section 5.8.3

## Release / Version Step

Implementation plan must include a release/version step tied to `1.0.2`:

- if the implementation branch starts from a pre-`1.0.2` base, bump the
  workspace version to `1.0.2` and regenerate any lockfile changes
- update the stale `atm-core = { version = "1.0.1", path = "../atm-core" }`
  pin in `crates/atm/Cargo.toml` to `1.0.2` as part of the same release-sync
  pass
- if the implementation branch already starts at `1.0.2`, retain `1.0.2` and
  do not introduce an additional version bump as part of this fix

The version step must be explicit in the implementation checklist so the fix is
not merged with ambiguous release-state assumptions.

## Implementation Checklist

1. Fix `resolve_command_path(...)` for absolute path vs relative path vs bare
   command name behavior.
2. Add unit tests for command-resolution rules in `crates/atm-core/src/send/hook.rs`.
3. Add integration tests in `crates/atm/tests/send.rs` for:
   - bare binary hook command works
   - relative script hook still works
   - recipient-only non-match is silent
   - two-axis non-match is silent
   - JSON mode remains silent on non-match
4. Remove or narrow the skip-warning formatting/helper path, and remove or
   rewrite the existing helper-specific unit test in
   `crates/atm-core/src/send/hook.rs`.
5. Update product and crate docs listed above.
6. Apply the explicit `1.0.2` version/release step, including the stale
   `crates/atm/Cargo.toml` path-dependency pin.
7. Validate with:
   - `cargo test --workspace`
   - `cargo clippy --workspace --all-targets -- -D warnings`

## Non-Goals

- no change to hook matching semantics
- no change to wildcard behavior
- no change to hook execution payload (`ATM_POST_SEND`)
- no change to hook timeout/failure handling
- no change to sender-only or recipient-only successful match behavior

## Validation To Run During Implementation

- targeted: `cargo test --workspace`
- targeted: `cargo clippy --workspace --all-targets -- -D warnings`
- confirm updated hook coverage:
  - bare binary command works via `PATH`
  - relative script path still resolves from config root
  - recipient-only non-match is silent
  - two-axis non-match is silent
  - actual hook execution failures still warn
