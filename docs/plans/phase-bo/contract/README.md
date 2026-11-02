# atm-bd framework contract

The types and traits every atm-bd sprint builds against. The framework owns all of it. A profile
plug-in (atm-bd-lite now, orchestration later) supplies only hook scripts written to `../hooks/*.md`,
templates, and a `policy.toml` naming rules and their params. The crate names no profile.

| File | Contents |
|---|---|
| `src/hook.rs` | Bead model, QA verdicts, the three transition enums with `select`, `BeadPatch`, `PhaseFile`, `ErrorId` + `Refusal` (one error type for both surfaces), the `hooks/*.md` Errors tables, traits `BeadStore`, `PhaseFiles`. |
| `src/wire.rs` | ATM's hook wire: `HookRequest::parse` (atm-core `hook/request.rs`), `HookAction`, `HookReply::to_json` (exactly what atm-core `HookReply::parse` accepts). |
| `src/monitor.rs` | Collect -> decide -> execute: facts, `PhaseFacts`, `Action` (`Assign`/`Report`/`Notify`), `Rule` trait, `Policy`, `PhaseView`, `TickResult`, ports `CommandRunner`, `BeadsCache`. |
| `atm_bd_types.pyi` | The Python surface: the names the lite hooks use, plus the monitor names. |
| `phase_monitor.py` | The console's `PhaseMonitor` Protocol (`queues`, `snapshot`, `step`, `cron`). |

Framework: everything above, plus the beads-cache adapter, collectors, decide, executor, console.
Plug-in: `../lite/skills/atm-bd-lite/scripts/` (the three hooks), templates, `policy.toml`.
The build sprint adds pyo3 bindings and generates the stub from the Rust.

## Verify

```
cargo test                                # reply bytes match atm-core's HookReply::parse
npx pyright -p .                          # strict: the three hooks, 0 errors (phase_monitor.py imports the atm-bd console; check it there)
uv run --with pytest pytest ../lite/tests # hook behaviour against the fake store
```
