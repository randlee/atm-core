# NotificationSink vs StorageNotifier

## Decision

`NotificationSink` and `StorageNotifier` remain separate interfaces.

They may be bridged at composition time, but they are not the same contract and
must not be collapsed into a single shared trait.

## Why They Stay Separate

- `NotificationSink` is transport/runtime-facing.
  It delivers operator-visible or workflow-visible notification events through
  the retained runtime and daemon composition layers.
- `StorageNotifier` is storage-facing.
  It represents post-commit storage events tied to shared `atm-storage`
  semantics after durable writes succeed.

Combining them would reintroduce the exact architectural leak Phase AC is
removing:
- transport concerns would bleed into the storage contract
- storage backends would inherit runtime notification semantics
- backend interchangeability would weaken because storage would depend on a
  higher-layer event model

## Allowed Relationship

Composition roots may bridge them explicitly:

- a runtime can observe a successful storage write
- then emit a `NotificationSink` event derived from that committed state

That bridge belongs in composition/runtime code, not in `atm-storage`.

## Boundary Rule

- `atm-storage` owns `StorageNotifier`
- `atm-core` / `atm-daemon` / `atm-runtime` own `NotificationSink`
- no storage backend should depend on `NotificationSink`
- no runtime transport surface should redefine `StorageNotifier`
