# AM.2 Closure Checklist

This checklist records the post-implementation review of AM.2 against
`sprint-AM2-delete-legacy-http.md`. Complete each item in order and retain
the associated validation evidence.

- [x] 1. Removed the permanently disabled legacy core raw-framing test module.
      Validation: `cargo test -p agent-team-mail-core --lib` (402 passed).
- [x] 2. Added active coverage proving the framework decoder recognizes every
      route in `http_route_surface()`. Validation: focused runtime test passes.
- [x] 3. Verified focused shared typed-client integration coverage for UDS and
      loopback-TCP retained paths. Validation: both runtime client tests pass.
- [x] 4. Verified focused framework route/body-limit coverage for the
      relocated decoder. Validation: route-contract and body-limit tests pass.
- [x] 5. M5 local isolated fast smoke passed after the deletion. Evidence:
      `site/reports/smoke/macos/rand-m5.local/20260810T195635850095Z-pid13391-smoke-fast`.
- [x] 6. Ledger updated with closure evidence; AM.2 plan re-read after every
      item. Final complete regression validation is recorded with the commit.
