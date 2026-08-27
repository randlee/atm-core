# AQ2.7 merge-forward record

The branch contains the requested forward merges of `origin/integrate/phase-aq`
through `334e5ca89` (`385422bf5`, then `bd9fb8c6d`). The later merge commit
`def0365ee` brought in a sibling feature branch and reintroduced the retired
file-record implementation into the graft boundary. The subsequent
merge-forward to integrate tip `a3c935639` is recorded in merge commit
`64752cb91`.

The preceding correction took `origin/integrate/phase-aq` wholesale for the
file-record-free versions of:

```text
crates/atm-core/src/graft.rs
crates/atm-graft/src/runtime/mod.rs
crates/atm-graft/src/lib.rs
```

The dependent `crates/atm/src/commands/internal_nudge.rs` caller was aligned to
the same registry-lease API. AQ2.7 Herdr, HTTP-runtime, and daemon-bootstrap
files remain on this branch's versions. The latest merge resolved its sole
comment-only conflict with integrate's version. No rebase or force push was
used.
