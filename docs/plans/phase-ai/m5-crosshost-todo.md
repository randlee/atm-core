# M5 cross-host TODO

Worktree: `~/Documents/github/atm-core-worktrees/fix/crosshost-ack-provenance`

1. Pull `fix/crosshost-ack-provenance`; verify `f5ac77f9` or newer.
2. In a login shell, run `python3 --version`; it must be Homebrew Python 3.11.
   Run `cargo build --workspace --release`.
3. Use `daemon-switch` to activate the worktree's matched release `atm` and
   `atm-daemon` pair with service `com.atm.daemon.crosshost-smoke`. Verify
   `daemon-switch status --doctor` reports the worktree paths and a healthy
   daemon. Do not start a raw foreground daemon. If LaunchAgent bootstrap
   fails, record the exact error and stop that step.
4. Keep `rand-m4.local` as the sole M4 peer name. It resolves on M5 through
   mDNS and is pinned to fingerprint
   `97C8D32D52D7F7E7CF0C2EB045784B3579209590704FCAD3CF8D4A6CFDBF4A23` on
   port `43101`. Do not add bare `rand-m4`: it does not resolve on M5.
5. From the active M5 ATM team shell, send one ordinary message to
   `arch-ctm@atm-dev.rand-m4.local`. Record its ULID. M4 must read that exact
   ULID and show source host `rand-m5`.
6. Repeat with `--requires-ack`; M4 must read the same ULID and acknowledge it;
   M5 must observe the acknowledgement.
7. Send one ordinary message M4 -> M5 and verify the exact ULID, body, and
   source host on M5.
8. If `atm send` reports an error, do not retry. Record the ULID first and ask
   the receiver to read it; an error after persistence can otherwise duplicate
   a delivered message.

Record each command, ULID, source host, and result in the smoke evidence
report. Do not modify `integrate/phase-ai-31-33` for this investigation.
