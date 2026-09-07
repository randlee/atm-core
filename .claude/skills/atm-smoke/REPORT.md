# ATM test report template

Every ATM test skill sends exactly one report message in this shape. Same shape on every
fixture; the `fixture` line is the only fixture-specific content.

```
ATM TEST REPORT
skill: <skill-name>
fixture: <$ATM_TEST_FIXTURE or hostname>
agent: <agent@team>  tools: <cli | native | native+cli>
atm: client <x.y.z> daemon <x.y.z>
result: PASS | FAIL   (<passed>/<total> steps)
steps:
  1 PASS <step name> — <observable: message id / count / exit code / error code / seconds>
  2 FAIL <step name> — <observable>
  ...
elapsed: <seconds>s
```

A FAIL in one step never stops the run: keep going, finish every remaining step, then report once.
On any FAIL, before sending the report:
1. run `../atm-troubleshoot/SKILL.md` for that step and get the cause;
2. if the fix is within your reach on the fixture (roster entry, your own gateway or tool session,
   a stale receiver registration, an environment variable, a wrong command form), apply it;
3. re-run the failed step once and record the retest;
4. the step line becomes `FAIL <step> — cause: <component/evidence>; fix: <what you did | none possible>; retest: PASS|FAIL`.
A FAIL without a cause line is not a finished report. Never change ATM code or binaries; a cause
in those goes to the requester as-is.

Rules: never include message bodies, addresses beyond `agent@team`, chat ids, tokens,
capability values or raw config. A FAIL line carries the error code or count, not narrative.
