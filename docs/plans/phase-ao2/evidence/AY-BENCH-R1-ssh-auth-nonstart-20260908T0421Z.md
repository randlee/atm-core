# AY-BENCH-R1 SSH authentication failed-attempt note — 2026-09-08

## Scope

This records the Rand-directed Phase AY ordinary benchmark attempt that did
not reach measurement. It is not a benchmark campaign and contains no
performance result.

| Field | Value |
|---|---|
| Account | `atmbench@rand-m5.local` |
| Selected branch | `fix/ay-closeout` |
| Selected branch source | `f3f89da1562651c17aeb916e526113842f6629e4` |
| Code-equivalent trunk source | `integrate/phase-ay` at `97cf27f5121695fe9601d7a86fffcd0acc84f092` |
| Trigger | Direct SSH preflight for `ATM_CAPACITY_HOST_LABEL=rand-m5 just benchmark` |
| Measurement started | No |
| Failure boundary | SSH public-key authentication before benchmark-account checkout access |

## Observed outcome

The required dedicated benchmark account was contacted directly. Its
configured login key and each other locally available explicit identity were
rejected by the SSH server. Password and keyboard-interactive authentication
cannot run in the non-interactive benchmark procedure.

```text
atmbench@rand-m5.local: Permission denied (publickey,password,keyboard-interactive).
```

No developer account, alternate host, sudo path, or substitute runner was
used. The benchmark checkout, `daemon-switch` preflight, `atm doctor`,
bootstrap, and matrix command were therefore not entered.

## Cleanup proof

The failure occurred before remote shell access, so no account process,
benchmark daemon, temporary ATM home, benchmark database, trace, campaign
JSON, report artifact, or source state was created or altered. No cleanup was
needed or attempted against the inaccessible dedicated account.

## Disposition

The Phase AY benchmark remains unmeasured. A new immutable attempt is required
after the `atmbench@rand-m5.local` public-key authorization is restored; it
must begin with the ordinary account preflight and run the full four-target
matrix without altering this note.
