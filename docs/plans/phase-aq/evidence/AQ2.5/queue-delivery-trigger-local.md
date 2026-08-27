# AQ2.5 bare-CLI queue delivery-trigger evidence

Host: `local`
Commit: `bc88a6ce154d12e935eb2e2c15e449dba70a0df4`
Status: **BLOCKED_AMBIENT_DAEMON**

This host has an ambient, already-running `atm-daemon` (pid(s) [1816]) that legitimately owns the OS account's singleton runtime lock (`atm_core::home::current_host_runtime_scope` intentionally ignores `ATM_HOME`/`HOME` — see `DaemonOwnerGuard`). This runner refuses to start a second daemon on this account rather than risk the ambient session, exactly as `run_hermes_atm_restart_matrix.py` (AQ1.9) does for the same reason.

Run this script on a dedicated OS account with no ambient `atm-daemon` to produce positive-path evidence:

```bash
python3 scripts/phase-aq/run_aq25_queue_delivery_trigger_evidence.py \
  --host <dedicated-account-label> \
  --daemon target/release/atm-daemon \
  --atm target/release/atm \
  --evidence-dir docs/plans/phase-aq/evidence/AQ2.5
```
