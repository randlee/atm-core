# Evaluation: established workspace adoption

1. Start with a Rust workspace that has root Just recipes, a CI matrix, and a
   consumer-owned recipe outside the managed import block.
2. Follow all seven skill steps verbatim, deriving analyzer reasons from
   observed async-runtime and target-platform facts.
3. Assert the generated consumer PR body includes the literal line
   `sc-lint adoption dry-run exit 0` after installation and recheck.
4. Assert the PR body enumerates only actual consumer-local scaffolding removed
   by the adopting agent and records the reason for each removal.
5. Assert the consumer-owned recipe remains outside the managed marker block.
