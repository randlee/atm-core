# Hermes ATM native-tools proof plan

Run this only after the wheel and isolated test evidence is green. Keep the
profile configuration local; never include a chat identifier in commands,
terminal output, git history, or proof artifacts.

1. Record the installed `hermes-atm`, `atm-graft`, and ATM daemon versions and
   the active profile name. Confirm the profile's existing installer-owned
   configuration supplies identity, team, ATM home, and workspace root.
2. Install the candidate `atm-graft` and `hermes-atm` wheels through normal
   package installation, then rerun `python -m hermes_atm install` for the
   profile. Do not edit generated hook or plugin files.
3. Reset that profile's managed gateway once. Confirm Hermes registered exactly
   `atm_send`, `atm_read`, and `atm_list` from `hermes-atm-native-tools`.
4. Invoke `atm_send` with a distinct ordinary test body and `requires_ack`
   false. Verify the structured success envelope and ordinary mailbox delivery.
5. Invoke `atm_list` and `atm_read` with a bounded, read-only selection. Before
   and after each invocation, compare the selected message's read and pending
   acknowledgement state; neither may change.
6. Invoke each tool once with an invalid/unknown argument. Verify a structured
   `kind=error` envelope with `layer=ingress_validation`, and verify no daemon
   mailbox operation occurred.
7. If a capability, registration, or structured-result check fails, reinstall
   the previous known-good package pair, rerun the package installer, reset the
   managed gateway, and record only non-sensitive failure details.
