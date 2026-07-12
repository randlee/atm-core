# Phase AE Installed Docs Proof

- status: `passed`
- generated at: `2026-07-12T05:24:19.072933Z`
- reviewed release version: 1.3.0
- source doc root: `docs/user-documents`
- staged install doc root: `target/phase-ae/staged-install-root/share/doc/atm`
- installed entrypoint: `share/doc/atm/README.md`
- release notes check: `passed` (`release/release-notes.md`)
- installed-doc verifier: `passed`

## Verified Installed Corpus

- `share/doc/atm/README.md`
- `share/doc/atm/doctor-and-log.md`
- `share/doc/atm/examples/diagnostics/doctor-json.sh`
- `share/doc/atm/examples/diagnostics/log-queries.sh`
- `share/doc/atm/examples/diagnostics/log-surface.json`
- `share/doc/atm/examples/hooks/post-send-notify.sh`
- `share/doc/atm/examples/hooks/post-send-payload.json`
- `share/doc/atm/examples/hooks/repo-atm-config.toml`
- `share/doc/atm/examples/identity/inspection-vs-mutation.sh`
- `share/doc/atm/examples/identity/resolution-scenarios.json`
- `share/doc/atm/examples/mailbox/clear-flow.sh`
- `share/doc/atm/examples/mailbox/inspect-then-read.sh`
- `share/doc/atm/examples/mailbox/workflow-surfaces.json`
- `share/doc/atm/examples/nudge-templates/acknowledge.xml`
- `share/doc/atm/examples/nudge-templates/acknowledge_task.xml`
- `share/doc/atm/examples/nudge-templates/delivery.xml`
- `share/doc/atm/examples/nudge-templates/delivery_ack.xml`
- `share/doc/atm/examples/nudge-templates/delivery_task.xml`
- `share/doc/atm/examples/nudge-templates/delivery_task_ack.xml`
- `share/doc/atm/examples/nudge-templates/manage-templates.sh`
- `share/doc/atm/examples/nudge-templates/template-lifecycle.json`
- `share/doc/atm/examples/quickstart/doctor-json.sh`
- `share/doc/atm/examples/quickstart/install-paths.json`
- `share/doc/atm/examples/quickstart/minimal-mailbox-flow.sh`
- `share/doc/atm/examples/troubleshooting/identity-recovery.sh`
- `share/doc/atm/examples/troubleshooting/post-send-warning.sh`
- `share/doc/atm/examples/troubleshooting/recovery-scenarios.json`
- `share/doc/atm/hooks.md`
- `share/doc/atm/identity-and-team.md`
- `share/doc/atm/install-layout.md`
- `share/doc/atm/mailbox-workflows.md`
- `share/doc/atm/nudge-templates.md`
- `share/doc/atm/quickstart.md`
- `share/doc/atm/troubleshooting.md`

## Source Corpus Members

- `docs/user-documents/README.md`
- `docs/user-documents/doctor-and-log.md`
- `docs/user-documents/examples/diagnostics/doctor-json.sh`
- `docs/user-documents/examples/diagnostics/log-queries.sh`
- `docs/user-documents/examples/diagnostics/log-surface.json`
- `docs/user-documents/examples/hooks/post-send-notify.sh`
- `docs/user-documents/examples/hooks/post-send-payload.json`
- `docs/user-documents/examples/hooks/repo-atm-config.toml`
- `docs/user-documents/examples/identity/inspection-vs-mutation.sh`
- `docs/user-documents/examples/identity/resolution-scenarios.json`
- `docs/user-documents/examples/mailbox/clear-flow.sh`
- `docs/user-documents/examples/mailbox/inspect-then-read.sh`
- `docs/user-documents/examples/mailbox/workflow-surfaces.json`
- `docs/user-documents/examples/nudge-templates/acknowledge.xml`
- `docs/user-documents/examples/nudge-templates/acknowledge_task.xml`
- `docs/user-documents/examples/nudge-templates/delivery.xml`
- `docs/user-documents/examples/nudge-templates/delivery_ack.xml`
- `docs/user-documents/examples/nudge-templates/delivery_task.xml`
- `docs/user-documents/examples/nudge-templates/delivery_task_ack.xml`
- `docs/user-documents/examples/nudge-templates/manage-templates.sh`
- `docs/user-documents/examples/nudge-templates/template-lifecycle.json`
- `docs/user-documents/examples/quickstart/doctor-json.sh`
- `docs/user-documents/examples/quickstart/install-paths.json`
- `docs/user-documents/examples/quickstart/minimal-mailbox-flow.sh`
- `docs/user-documents/examples/troubleshooting/identity-recovery.sh`
- `docs/user-documents/examples/troubleshooting/post-send-warning.sh`
- `docs/user-documents/examples/troubleshooting/recovery-scenarios.json`
- `docs/user-documents/hooks.md`
- `docs/user-documents/identity-and-team.md`
- `docs/user-documents/install-layout.md`
- `docs/user-documents/mailbox-workflows.md`
- `docs/user-documents/nudge-templates.md`
- `docs/user-documents/quickstart.md`
- `docs/user-documents/troubleshooting.md`

## Validation Inputs

- `python3 scripts/validate_release.py validate --proof-output reports/smoke/phase-AE-installed-docs-proof.md`
- `scripts/verify_user_docs.py` on the repo-owned source corpus and the staged installed copy
- `release/release-notes.md` installed-doc location references
