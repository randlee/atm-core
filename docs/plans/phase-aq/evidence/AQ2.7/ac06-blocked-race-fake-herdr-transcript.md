# AC6 blocked-race deterministic fixture

This is the recorded `FakeHerdrProcessAdapter` transcript for
`ac06_blocked_race_releases_pending_with_zero_injected_bytes`. It substitutes
for a live blocked-dialog transcript, which is not available in CI.

```text
list(session=aq27-test)
  -> agent aq27-agent status=idle
claim(member=aq27-agent@aq27-team)
  -> message=<claimed>, attempt=0, nudge_pending_at cleared
prompt(agent=aq27-agent, session=aq27-test)
  -> error=AgentBlocked / agent_blocked
  -> accepted prompt bytes=0
release_pending(member=aq27-agent@aq27-team, message=<claimed>)
  -> marker restored, attempt=0
tick stats
  -> prompted=0, released=1
```

The fixture also asserts that the post-claim prompt call occurs exactly once,
the marker remains claimable, and no retry debt is consumed.
