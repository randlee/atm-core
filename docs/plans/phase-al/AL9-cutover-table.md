# AL.9 adapter cutover table

**Release operator:** team-lead. **Status:** not-yet-activated. No row below
is evidence of a switched host until team-lead records the activation
observation and rollback result.

| Adapter | Status / operator | Add / serving implementation | Activation trigger | One active listener / publisher invariant | Retire action | Rollback action |
| --- | --- | --- | --- | --- | --- | --- |
| In-process router | `operator=team-lead`; `status=not-yet-activated` (test-only) | `canonical_api_router` inside `atm-http-runtime` test/runtime assembly | Team-lead runs only a test harness; no release activation | No socket and no endpoint record exist | Test process exits | Test harness drops the runtime |
| Unix UDS (Unix, non-root) | `operator=team-lead`; `status=not-yet-activated` | `HttpRuntime<Configured>::start` binds the configured owner-only `HOST_RUNTIME_SOCKET_FILE` | Team-lead starts the replacement daemon | The one `HttpRuntime` owns and removes its own socket; no endpoint-record publisher applies | Stop legacy listener before activating this listener; Phase AM deletes legacy code only after AL.9 accepts | Team-lead stops replacement; removes only its owned socket through normal drain; keep legacy unchanged until a new approved proof |
| Loopback TCP | `operator=team-lead`; `status=not-yet-activated` | `HttpRuntime` binds loopback and `publish_loopback_endpoint_record` writes `local-http.json` | Team-lead starts the replacement daemon | One `HttpRuntime` listener publishes exactly one capability record after all enabled listeners bind; `LoopbackEndpointRecordGuard` removes only its own generation | Stop legacy listener/publisher before activation; Phase AM later deletes legacy publisher | Team-lead drains replacement; its generation guard cleans its record without removing a successor record |
| CLI local write | `operator=team-lead`; `status=not-yet-activated` | `preferred_local_client` -> UDS or loopback `HttpRuntimeClient` | No listener; client becomes usable only after the corresponding runtime listener is active | Does not publish a record or bind a listener | AM.1 migrates retained synchronous read/ack/admin before deleting compatibility client | Team-lead restores the previously approved client/runtime pair as one switch, not a client-only fallback |
| Graft local write | `operator=team-lead`; `status=not-yet-activated` | Same `preferred_local_client` / `HttpRuntimeClient` path as CLI | No listener; independent graft process connects only after runtime is active | Does not publish a record or bind a listener | AM.1 owns async non-write conversion and legacy client deletion | Same paired client/runtime rollback; do not attach to or kill an ambient daemon for smoke |
| Cross-host/M5 | `operator=team-lead`; `status=deferred` | No MVP adapter exists in this proof subject | Not activatable under AL.9 | No listener/publisher may be invented for proof | N/A | Dropped from AL.9 pending a separately assigned secure connector |

## Hard activation conditions

Before team-lead switches any host, this table must be amended with:

1. operator identity, exact source SHA, binary version/path, and start time;
2. the observed process/listener for each enabled adapter and the loopback
   record publisher/generation;
3. a successful same-host CLI write, graft write, and direct-failure check;
4. the rollback command, named owner, and post-rollback listener/record check.

If any item fails, team-lead keeps the existing activation state,
parks AL, and does not authorize AM or a ledger freeze. This document makes no
claim that the ambient daemon currently running on this machine is either the
replacement or the legacy process.
