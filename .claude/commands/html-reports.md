---
name: html-reports
version: 0.1.0
description: Regenerate ATM HTML diagram reports and JSON sidecars from Mermaid SSOT sources.
---

# /html-reports command

Regenerate the ATM diagram reports after editing:
- `docs/atm/*.mmd`
- `docs/atm-rusqlite/*.mmd`
- report templates
- report CSS/JS
- schema popup rendering logic

## Prompt

Rebuild the static ATM HTML reports and JSON sidecars from the Mermaid source-of-truth files using the existing report generator script in this worktree.

## Execution

```python
python3 docs/atm/diagrams/scripts/generate_diagram_pages.py
```

## Outputs

This command regenerates:
- `docs/atm/cli-diagrams.html`
- `docs/atm/cli-diagrams.json`
- `docs/atm/client-interface-diagrams.html`
- `docs/atm/client-interface-diagrams.json`
- `docs/atm-rusqlite/query-diagrams.html`
- `docs/atm-rusqlite/query-diagrams.json`
- `docs/atm/diagrams/panels/*.html`
