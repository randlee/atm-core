# Sprint Report Dependencies

## Check first

```bash
which sc-compose && sc-compose --version
python3 -c 'import rdflib'
```

## Find an existing `sc-compose`

```bash
for p in "$HOME/.local/bin/sc-compose" \
  "$(python3 -m site --user-base 2>/dev/null)/bin/sc-compose" \
  "/opt/homebrew/bin/sc-compose"; do
  [ -x "$p" ] && echo "Found at: $p" && break
done
```

Use the discovered full path or add its directory to `PATH` for this session.

## Install or repair

Install the released CLI using the project-supported package channel, then
rerun the check. Install the Python dependency into the Python interpreter
that runs the report:

```bash
python3 -m pip install --upgrade rdflib
```

Do not use a missing CLI or a different Python environment as a degraded
fallback: the report must run with both dependencies available.
