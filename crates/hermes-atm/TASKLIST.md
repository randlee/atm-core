# hermes-atm installation and proof checklist

Use this checklist for every profile installation. Keep profile-specific values,
especially a chat identifier, in local configuration only.

- [ ] Install a Hermes Agent release that exposes the public gateway and plugin
  seams required by `hermes-atm`.
- [ ] Configure the gateway LaunchAgent environment with `ATM_HOME`,
  `ATM_IDENTITY`, `ATM_TEAM`, `ATM_CHAT_ID`, and `ATM_WORKSPACE_ROOT`.
- [ ] Install matching released `atm-graft` and `hermes-atm` wheels into the
  Python interpreter selected by that LaunchAgent.
- [ ] Publish the matching ATM roster member with `harness hermes`, the
  profile home directory, and the exact workspace root.
- [ ] Run `python -m hermes_atm install` using the same Python interpreter.
  The installer owns the generated hook, native-tools plugin, and
  `plugins.enabled` configuration entry.
- [ ] Reset the gateway; do not hand-edit generated package files.
- [ ] Prove a localhost ATM send reaches the profile as a nudge and produces
  an autonomous reply.
- [ ] Prove the registered `atm_send`, `atm_read`, and `atm_list` tools return
  their structured JSON envelopes. See [NATIVE_TOOLS_PROOF.md](NATIVE_TOOLS_PROOF.md)
  for the detailed native-tool proof procedure.

After a package update, repeat the wheel install, package installer, gateway
reset, and proof steps. Never patch an installed wheel or generated hook to
make a proof pass.
