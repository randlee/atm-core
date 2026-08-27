# AQ1.9 Hermes ATM restart matrix — local loopback run

Status: `PENDING-DEDICATED-HOST-RUN`

The local loopback rows were not fabricated: this workstation has an ambient
account-owned `atm-daemon`, and the daemon runtime/database intentionally ignore
`ATM_HOME`. The runner therefore refuses to start or stop a competing process.

Run the following command from the AQ1.9 branch on a dedicated local ATM OS
account with no ambient daemon and the candidate wheel installed:

```bash
python3 scripts/phase-aq/run_hermes_atm_restart_matrix.py \
  --host local \
  --daemon target/release/atm-daemon \
  --atm target/release/atm \
  --evidence-dir docs/plans/phase-aq/evidence/AQ1.9
```

The live run replaces this slot with `restart-matrix-local.json` and
`restart-matrix-local.md` only after all three loopback rows produce real
timestamps, message IDs, and `atm doctor --json` captures.
