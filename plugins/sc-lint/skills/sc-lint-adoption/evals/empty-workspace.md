# Evaluation: empty workspace adoption

1. Copy `tests/fixtures/adoption/empty-workspace` to a clean temporary directory.
2. Copy `tests/fixtures/adoption/install.json` to that directory.
3. Follow all seven skill steps verbatim.
4. Assert the consumer PR body contains `sc-lint adoption dry-run exit 0` after
   the apply-and-recheck cycle, plus the aggregate validation result.
5. Assert no consumer-local wrapper removal is claimed unless it existed.

This evaluation is durable: it verifies the command wording as well as the
consumer-facing PR evidence.
