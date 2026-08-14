# AN.14 Checked-Emission Evidence

AN.14 consumes the crates.io releases `sc-sha`, `sc-composer`, and
`sc-compose` 1.4.1.  The lockfile records the registry source and checksums
for the two adapter-resolved crates: `sc-composer` checksum
`4415ff74a7f91a7505a7c9fc464908ed5e0e684d2648b5d731e0533c371edb2c` and
`sc-sha` checksum
`01502b8bda56eef5c2f445a88396d75cc223c8ce91709ac007dbb81f40e577ba`.

The upstream checked-emission API is supplied by the closed
[sc-compose #448](https://github.com/randlee/sc-compose/issues/448):
`sc_composer::check_rendered_output`, `CheckedOutput`, and `OutputFormat`.
Only `atm-template-sc-compose` imports these types. Its adapter calls the
checker after final-body assembly on file root render, native compose, and
stored/decomposed render-on-read. The rejection tests prove malformed JSON is
not emitted and the diagnostic cause does not expose the `secret` body value.

The final AN.14 CI validation commit is recorded after the QA fix round.
