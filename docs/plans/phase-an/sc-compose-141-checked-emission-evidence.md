# AN.14 Checked-Emission Evidence

AN.14 consumes the crates.io releases `sc-sha`, `sc-composer`, and
`sc-compose` 1.4.1.  The lockfile records the registry source and checksums
for the two adapter-resolved crates: `sc-composer` checksum
`4415ff74a7f91a7505a7c9fc464908ed5e0e684d2648b5d731e0533c371edb2c` and
`sc-sha` checksum
`01502b8bda56eef5c2f445a88396d75cc223c8ce91709ac007dbb81f40e577ba`.

The upstream checked-emission API is supplied by the closed
[sc-compose #448](https://github.com/randlee/sc-compose/issues/448):
`sc_composer::check_rendered_output_with_meta`, `RenderCheckMeta`,
`CheckedOutput`, and `OutputFormat`. Only `atm-template-sc-compose` imports
these types. Its adapter calls the checker after final-body assembly on file
root render, native compose, and stored/decomposed render-on-read. ATM does
not expose sc-composer guidance or user-prompt inputs, so the adapter passes
neither; it never appends unmodelled output blocks.

The regression suite at
`8a7cf89138f670f9d8d6e44799cb9d192d5360ab` records all required checked
emission vectors:

- auto and legacy JSON escaping preserve a hostile string as one JSON value
  rather than emitting an injected top-level key;
- a malformed final body from render pass 2 is rejected with pass provenance;
- file-backed send/rendered fallback and stored/decomposed render inputs stay
  unchanged after rejection; and
- all malformed-output errors retain the stable
  `TEMPLATE_RENDER_VERIFICATION_FAILED` code without exposing the `secret`
  body literal.

Validation after that commit passed locally: `cargo test -p
atm-template-sc-compose` (15 tests), `just lint`, and `just test` (498 Python
tests plus the Rust workspace test suite). CI is the authoritative
cross-platform gate for the final branch tip.
