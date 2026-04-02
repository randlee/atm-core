# Phase PG Planning

## 1. Goal

Phase PG adds a per-agent `post_send` hook to the daemon-free `atm-core` send
path so async recipients can be nudged after a successful mailbox write without
changing send success semantics.

This planning doc is implementation-focused and complements:

- [`docs/atm-core/requirements.md`](./atm-core/requirements.md)
- [`docs/atm-core/architecture.md`](./atm-core/architecture.md)
- [`docs/project-plan.md`](./project-plan.md)

## 2. Scope

The phase covers:

- config schema support for `.atm.toml [agents.<name>]`
- typed `AtmConfig` support for per-agent hook settings
- send-path integration after successful mailbox write
- unit and integration coverage for the hook contract

The phase does not cover:

- blocking on hook completion
- daemon delivery or queue orchestration
- CLI flag changes

## 3. Sprint Breakdown

### PG.1: Config Schema And Type Model

Goal:
- Extend config parsing so `atm-core` can deserialize per-agent hook
  configuration from `.atm.toml`.

Primary files:
- `crates/atm-core/src/config/types.rs`
- `crates/atm-core/src/config/mod.rs`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`

Required outcome:
- `AtmConfig` gains `agents: HashMap<String, AgentConfig>`
- `AgentConfig` gains `post_send: Option<String>`
- config-loading tests cover present and absent `[agents.<name>]` tables
- crate docs describe the config ownership and hook contract

### PG.2: Send Path Integration And Unit Tests

Goal:
- Invoke the recipient-specific hook as a best-effort side effect after the
  outbound message write succeeds.

Primary files:
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/send/` test helpers as needed
- `docs/atm-core/modules/send.md` if module references need refresh during implementation

Required outcome:
- hook lookup is keyed by recipient agent name
- dry-run sends do not spawn hooks
- hook launch uses `Command::new("sh").arg("-c").arg(hook).spawn()`
- spawned command receives `ATM_SENDER`, `ATM_RECIPIENT`, and
  `ATM_MESSAGE_BODY`
- hook errors do not change the successful send result

### PG.3: Integration Test With Mock Hook

Goal:
- Prove the contract end-to-end with a mock hook that records its invocation.

Primary files:
- `crates/atm-core/tests/` integration coverage or equivalent workspace test location
- temporary fixture scripts under the test tree

Required outcome:
- successful send writes the message and triggers exactly one hook invocation
- the mock hook observes the expected env vars
- failed hook execution does not fail `send_mail`
- no-hook recipients still send successfully without extra side effects

## 4. Exit Criteria

Phase PG is complete when:

- the requirements and architecture docs agree on config shape and error policy
- `atm-core` can parse optional per-agent `post_send` config
- the send path launches the hook only after a successful write
- tests cover both the happy path and error non-propagation
