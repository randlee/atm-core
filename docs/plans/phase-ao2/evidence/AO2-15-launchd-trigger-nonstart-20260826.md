# AO2.15 LaunchAgent trigger failed-attempt note — 2026-08-26

## Scope

This records an official-trigger attempt that did **not** reach benchmark
measurement. It is not a campaign and contains no performance result.

| Field | Value |
|---|---|
| Account | `atmbench@rand-m5.local` (uid 502) |
| Selected branch | `feature/ao2-15-benchmark-official-trigger` |
| Selected source | `16b874d325ab47445ce987caf8c25f42c766d81c` |
| Trigger | Account-local `com.atm.benchmark-official` LaunchAgent template |
| Measurement started | No |
| Failure boundary | launchd bootstrap |

## Observed outcome

The account has no GUI session, as intended for an unattended benchmark
account. The normal LaunchAgent bootstrap therefore failed without starting a
job:

```text
launchctl bootstrap gui/502 ...
Bootstrap failed: 125: Domain does not support specified action
```

The headless-domain attempt also did not create a job or start the runner:

```text
launchctl bootstrap user/502 ...
Bootstrap failed: 5: Input/output error
```

`launchctl print user/502/com.atm.benchmark-official` confirmed that no job
was registered. The error requested a root-level retry for further launchd
diagnostics; the benchmark account has no passwordless privileged bootstrap.
No shell substitute was treated as a LaunchAgent run.

## Cleanup proof

The temporary plist was removed after each attempt. Final account inspection
confirmed no `atmbench`-owned `atm-daemon`, no `~/.atm/db`, no raw benchmark
trace directory, and no `com.atm.benchmark-official.plist` remained. No
campaign JSON, report artifact, benchmark database, or source state was
created or altered by this attempt.

## Disposition

The AO2.15 launchd leg remains unverified on this headless account. A future
attempt requires a supported privileged bootstrap arrangement or an approved
headless trigger design; it must leave a new immutable attempt artifact.
