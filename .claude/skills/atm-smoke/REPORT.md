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

Rules: never include message bodies, addresses beyond `agent@team`, chat ids, tokens,
capability values or raw config. A FAIL line carries the error code or count, not narrative.
