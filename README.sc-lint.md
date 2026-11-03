# sc-lint adoption kit

Install a generic, drift-checked `sc-lint` consumer contract with:

```text
python3 plugins/sc-lint/install.py .
```

Use `--dry-run` to report drift without writing.

The first install persists its validated input at `.sc-lint/install.json`.
Use `--input <path>` to override it or bootstrap a new input document.

Kit-rendered steps name only shipped binaries or `sc_lint` modules; profile and
test-layer commands are consumer-owned and rendered verbatim.
