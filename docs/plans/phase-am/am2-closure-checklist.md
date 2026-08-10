# AM.2 Closure Checklist

This checklist records the post-implementation review of AM.2 against
`sprint-AM2-delete-legacy-http.md`. Complete each item in order and retain
the associated validation evidence.

- [x] 1. Removed the permanently disabled legacy core raw-framing test module.
      Validation: `cargo test -p agent-team-mail-core --lib` (402 passed).
- [ ] 2. Eliminate route-surface/decoder drift risk by deriving framework
      decoding from the canonical route contract or testing every retained
      route against it.
- [ ] 3. Add focused typed-client compatibility coverage for the retained
      non-write UDS and loopback-TCP paths, including an in-runtime caller.
- [ ] 4. Add focused framework route/body-limit coverage for the relocated
      runtime decoder.
- [ ] 5. Produce and record post-deletion local and M5 smoke evidence for the
      final candidate.
- [ ] 6. Update the AM.1 ledger with closure evidence, re-read the AM.2 plan,
      and run the complete regression suite.
