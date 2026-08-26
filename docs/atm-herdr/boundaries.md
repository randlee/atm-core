# ATM Herdr Boundary Inventory

`atm-herdr` owns the Tokio-native, shell-free process boundary for the Herdr
CLI. It is shared by the replacement daemon's Herdr received hook and the
AQ2.7 queue wake pump.

Canonical machine-readable boundary source:

- [herdr-process-adapter.toml](../../boundaries/atm-herdr/herdr-process-adapter.toml)

The crate owns exact Herdr argv construction, child-only `HERDR_SESSION`
selection, bounded process execution, structured JSON/error parsing, and the
spawn breaker. It must not own ATM routing, roster persistence, tmux input, a
shell, SQLite, or daemon request dispatch. Tests below this boundary use the
feature-gated `atm_herdr::testing::FakeHerdrProcessAdapter`; they do not spawn
the real CLI.
