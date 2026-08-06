# Phase AL/AM — Baseline Boundary Manifest Inventory

**Pinned source:** `develop` `67401907039f92e58e883273f02372a637202f70`
**Enumeration command:** `git ls-tree -r --name-only <sha> -- boundaries | rg '\\.toml$'`

This path list, rather than a manifest count, is the AM.6 reconciliation
baseline. A manifest introduced after this SHA must be listed separately with
its introducing commit and retain/remove disposition.

```text
boundaries/atm-core/atm-protocol.toml
boundaries/atm-core/claude-compatibility-mailbox-writer.toml
boundaries/atm-core/client-transport.toml
boundaries/atm-core/config-doctor.toml
boundaries/atm-core/config-ingress.toml
boundaries/atm-core/daemon-runtime-doctor-report.toml
boundaries/atm-core/graft-post-send-port.toml
boundaries/atm-core/inbox-export.toml
boundaries/atm-core/inbox-ingress.toml
boundaries/atm-core/mail-store-doctor.toml
boundaries/atm-core/mail-store.toml
boundaries/atm-core/non-claude-outbound.toml
boundaries/atm-core/notification-sink.toml
boundaries/atm-core/post-send-hook-emitter.toml
boundaries/atm-core/reconcile-coordinator.toml
boundaries/atm-core/request-dispatcher.toml
boundaries/atm-core/roster-store-doctor.toml
boundaries/atm-core/roster-store.toml
boundaries/atm-core/runtime-factory.toml
boundaries/atm-core/server-transport.toml
boundaries/atm-core/status-source.toml
boundaries/atm-core/watch-event-source.toml
boundaries/atm-daemon-client/daemon-bootstrap.toml
boundaries/atm-daemon-client/rpc-envelope.toml
boundaries/atm-daemon/admission-runtime-view.toml
boundaries/atm-daemon/daemon-config-ingress.toml
boundaries/atm-daemon/daemon-inbox-export.toml
boundaries/atm-daemon/daemon-inbox-ingress.toml
boundaries/atm-daemon/daemon-non-claude-outbound.toml
boundaries/atm-daemon/daemon-notification-sink.toml
boundaries/atm-daemon/daemon-reconcile-coordinator.toml
boundaries/atm-daemon/daemon-request-dispatcher.toml
boundaries/atm-daemon/daemon-status-source.toml
boundaries/atm-daemon/file-watch-event-source.toml
boundaries/atm-daemon/host-ownership-daemon.toml
boundaries/atm-daemon/lifecycle-control-source.toml
boundaries/atm-daemon/peer-delivery-coordinator.toml
boundaries/atm-daemon/peer-http-adapter.toml
boundaries/atm-daemon/post-commit-work-queue.toml
boundaries/atm-daemon/post-write-router.toml
boundaries/atm-daemon/runtime-lifecycle-daemon.toml
boundaries/atm-daemon/socket-server-transport.toml
boundaries/atm-graft-python/hermes-graft-binding.toml
boundaries/atm-graft/post-send-notification-transport.toml
boundaries/atm-graft/shared-client-consumer.toml
boundaries/atm-runtime/runtime-composition.toml
boundaries/atm-storage-rusqlite/mail-store-sqlite.toml
boundaries/atm-storage-rusqlite/roster-store-sqlite.toml
boundaries/atm-storage-rusqlite/shared-db.toml
boundaries/atm-storage/message-store.toml
boundaries/atm-storage/nudge-template-override-store.toml
boundaries/atm-storage/outbound-message-query.toml
boundaries/atm-storage/peer-config-store.toml
boundaries/atm-storage/roster-store.toml
boundaries/atm-storage/storage-notifier.toml
boundaries/atm/cli-observability.toml
boundaries/atm/local-socket-client-transport.toml
```
