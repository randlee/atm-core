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
python3 docs/reports/generate_diagram_pages.py
```

## Outputs

This command regenerates:
- `docs/reports/cli-diagrams.html`
- `docs/reports/cli-diagrams.json`
- `docs/reports/client-interface-diagrams.html`
- `docs/reports/client-interface-diagrams.json`
- `docs/reports/query-diagrams.html`
- `docs/reports/query-diagrams.json`
- `docs/reports/panels/*.html`
