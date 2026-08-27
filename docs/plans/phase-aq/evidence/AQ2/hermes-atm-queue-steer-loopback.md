# AQ2 Hermes ATM queue/steer loopback evidence

Date: 2026-08-27  
Branch: `feature/aq-2-queue-graft`  
Harness: the repository's isolated Hermes ATM wheel/loopback contract runner

Per the 2026-08-27 Fenix ruling, this loopback harness evidence is accepted for
AQ2; live Hermes-fronted evidence is deferred to AQ5 phase evidence.

The following command builds the current `atm-graft` wheel, builds the current
universal `hermes-atm` wheel, installs both into a clean temporary virtual
environment, and runs the Hermes callback loopback tests:

```text
$ python3 .just/run_hermes_graft_bridge_tests.py
...
Ran 18 tests in 0.094s
OK
```

The exercised callback assertions prove the two routing rows required by AQ2:

```text
atm send  -> PyNudge(kind="steer") -> Hermes mode="steer" (/steer)
atm queue -> PyNudge(kind="queue") -> Hermes mode="queue" (/queue)
```

This is loopback contract evidence, not a claim about an external live Hermes
host. The runner deliberately uses a clean temporary environment and the
checked-in fake Hermes injector/session, so no host daemon or Hermes account is
required for this proof.
