# AQ1.9 Hermes ATM restart matrix — m5 live run

Status: `PENDING-LIVE-RUN`

This slot is intentionally not fabricated from loopback evidence. Run the
following command from the checked-out AQ1.9 branch on `m5` after building and
installing the candidate `atm-graft` wheel into the active Hermes Python
environment:

```bash
python3 scripts/phase-aq/run_hermes_atm_restart_matrix.py \
  --host m5 \
  --daemon target/release/atm-daemon \
  --atm target/release/atm \
  --evidence-dir docs/plans/phase-aq/evidence/AQ1.9
```

The runner owns its temporary ATM home, daemon, receiver worker, and cleanup.
It records transcript timestamps, message IDs, and `atm doctor --json` for the
daemon-restart, receiver-restart, and SIGKILL-within-window rows. The command
replaces this pending slot with `restart-matrix-m5.json` and
`restart-matrix-m5.md` only after the live run completes.
