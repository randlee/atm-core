# Triage Report Dependencies

## Check first

```bash
which python3 && python3 -c 'import rdflib'
which gh && gh auth status
```

## Repair the Python environment

Install the dependency into the same interpreter used to run the report:

```bash
python3 -m pip install --upgrade rdflib
```

## Repair GitHub access

Authenticate the GitHub CLI, then repeat the check:

```bash
gh auth login
gh auth status
```

The report treats unavailable GitHub state as an authoritative data gap. Do
not replace it with manually entered PR or CI fields.

## Optional renderer

Rendering additionally requires `sc-compose`:

```bash
which sc-compose && sc-compose --version
```

Use the project-supported release channel to install it, then rerun the check.
