# atm-bd framework contract

The types and traits every atm-bd sprint builds against. The crate names no profile.

## Boundary

Rand, 2026-11-02: "An important way to determine where the boundary is is if decisions are made that change operator state in a meaningful way. i.e. error recovery if it would be the same for everyone is not meaningful state change. closing a bead, assigning a task/bead are meaningful state changes. plug-in will use different logic and different types of teams i.e. dev->sanity->qa (orchestration), dev->qa (lite) are 2 meaningful differences." And: "once this boundary is established, we can then start replacing items on both sides of the boundary w/ more mature implementations."

Rand, 2026-11-02, follow-up: "the python/monty code called by cron would be part of the plug-in."

- Plug-in decides: close or reopen a bead, assign a task or bead, which stage follows which, who gets assigned, what the cron does each tick (its `decide` script).
- Framework does: collect facts, load params from `policy.toml`, call `decide`, dedupe and execute decided actions, retries and error recovery, wire format, console, cron scheduling, install mechanics.

Plug-ins import only `atm_bd_types`; `lite/tests` enforces it.

Composition (Rand, 2026-11-02): "it is fine if each plug-in builds its own composition root. as long as most of what it is composing is 'common'. this is probably the most powerful architecture since the plug-in would have the power to replace any component it deemed necessary." Each lite hook's `__main__` is its composition root: it wires the common `CacheStore` and `PhaseFilesCli` into the pure `hook(...)`.

| File | Contents |
|---|---|
| `src/plugin.rs` | Plug-in surface: re-exports the hook and wire types below; defines facts, `PhaseFacts`, `Action` (`Assign`/`Report`/`Notify`), `Params`, `Cron` (the one cron entry point, `decide`), `Profile`. |
| `src/framework.rs` | Framework only: `Policy` (the params `decide` gets), `Decision`, `PhaseView` and `View*`, `ActionOutcome`, `TickResult`, ports `CommandRunner`, `BeadsCache`, `Ports`; re-exports the hook Errors tables and `HOOK_ACTIONS_MAX`. |
| `src/hook.rs` | Bead model, QA verdicts, the three transition enums with `select`, `BeadPatch`, `PhaseFile`, `ErrorId` + `Refusal`, the `hooks/*.md` Errors tables, traits `BeadStore`, `PhaseFiles`. |
| `src/wire.rs` | ATM's hook wire: `HookRequest::parse` (atm-core `hook/request.rs`), `HookAction`, `HookReply::to_json` (exactly what atm-core `HookReply::parse` accepts). |
| `atm_bd_types.pyi` | Python plug-in surface: the names the lite hooks use, facts, `Params`, `Action`, `Cron`, `Profile`. |
| `atm_bd_core.pyi` | Python framework surface, mirroring `src/framework.rs`. |
| `phase_monitor.py` | The console's `PhaseMonitor` Protocol (`queues`, `snapshot`, `step`, `cron`). |
| `../lite/skills/atm-bd-lite/profile.toml` | The lite `Profile`: stages dev, qa; its three hooks; templates; `policy.toml`; its cron script. |
| `../lite/skills/atm-bd-lite/scripts/atm-bd-cron.py` | Lite's cron `decide`: assign each released QA bead (intent.md, "The close contract"). |

The build sprint adds pyo3 bindings and generates the stubs from the Rust.

## Verify

```
cargo test                                # reply bytes match atm-core's HookReply::parse; lite profile.toml parses
npx pyright -p .                          # strict: the three hooks, 0 errors (phase_monitor.py imports the atm-bd console; check it there)
uv run --with pytest pytest ../lite/tests # hook behaviour against the fake store; the import boundary
```
