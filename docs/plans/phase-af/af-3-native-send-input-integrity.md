# AF-3 — Native send-input integrity

## Sprint intent

Close the native CLI regression reported after the initial 1.3.0 smoke:
`atm send <to> --stdin < non-empty-file` fails with `message text cannot be
empty`, while `--file` with the same content succeeds. This is a standalone
input-to-RPC correctness sprint, not an AF-2 observability subtask.

The cause is confirmed: `SendMessageSource::Stdin` crosses the local IPC
boundary unchanged; `resolve_message_body` then reads the daemon process's
stdin, which daemon launch intentionally configures as null. The daemon must
never read a caller's stdin.

## Boundary contract

```rust
pub enum CliSendMessageSource {
    Inline(String),
    Stdin,
    File { path: PathBuf, message: Option<String> },
}

pub enum WireSendMessageSource {
    Inline(String),
    File { path: PathBuf, message: Option<String> },
    // No Stdin variant: stdin is consumed by the CLI before RPC.
}

pub fn materialize_cli_message_source(
    source: CliSendMessageSource,
) -> Result<WireSendMessageSource, AtmError>;
```

The exact type names may differ, but the production invariant is fixed: a
daemon-bound compose request contains message bytes or a valid file-reference
contract, never an instruction to read daemon stdin. Client-side materializing
must preserve the existing 256 KiB bound and typed empty, oversized, UTF-8,
and conflicting-input failures.

## Authoritative deliverables

| ID | Deliverable | Primary paths | Acceptance criteria | Required validation |
| --- | --- | --- | --- | --- |
| AF3-D1 | Separate CLI input selection from daemon-wire source | `crates/atm/src/commands/send.rs`, `crates/atm-core/src/send/mod.rs`, send request/RPC DTOs | `CliSendMessageSource` selects inline, file, or stdin; before `CliComposition::bootstrap` and RPC, `Stdin` is read once and materialized into `WireSendMessageSource`. A daemon-bound compose request cannot encode `Stdin`. Existing `--file` semantics remain unchanged. | Unit tests cover source selection and prove a daemon-wire DTO cannot represent `Stdin`; RPC round-trip covers materialized stdin bytes. |
| AF3-D2 | Preserve local input error contract | `crates/atm-core/src/send/input.rs`, CLI error rendering/tests | Empty, whitespace-only, oversized, non-UTF-8, unreadable, and conflicting `--stdin` inputs fail at the CLI boundary with their typed ATM errors and recovery guidance. No daemon starts or receives a request after a local input failure. | Boundary tests for each failure mode plus a process test asserting daemon PID/count is unchanged after invalid stdin. |
| AF3-D3 | Prove native input modes through the release daemon | CLI integration tests and `scripts/smoke/run_thorough_shared_host.py` | Inline, stdin, and file sends persist their exact expected bodies through a daemon whose stdin is null. The smoke lane reads the resulting message and compares the durable body, not merely command exit status. | Release-binary process tests pipe a non-empty 4 KiB fixture to `--stdin`, use equivalent `--file`, and send inline text; each readback is byte-for-byte correct and produces no unexpected daemon error event. |

## Paths to delete or replace

| Retired path | Required replacement / proof |
| --- | --- |
| Daemon-side `SendMessageSource::Stdin => read_message_from_stdin()` | D1's client-side materialization and a wire source with no `Stdin` variant. |
| Smoke that treats send exit status as sufficient for input modes | D3's durable byte-for-byte readback for inline, stdin, and file. |

## Non-closure

AF-3 does not change hook selection, doctor reporting, daemon singleton
admission, or file-reference policy beyond preserving the existing `--file`
contract. Those concerns remain respectively AF-2, AF-1, and existing product
behavior.
