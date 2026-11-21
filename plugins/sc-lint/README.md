# sc-lint adoption kit

From the installed Claude Code plugin, run its installer with:

```bash
python3 "$CLAUDE_PLUGIN_ROOT/install.py" .
```

The installer seeds only missing destinations. It leaves existing files,
directories, and symlinks untouched. `--dry-run` lists the destinations that
would be seeded without writing them.

The marketplace plugin root is supplied by Claude Code. The repository path
`plugins/sc-lint/install.py` is the consumer layout created by an install; it
is not the marketplace entry point.

Use `--input <path>` to override the input document.
