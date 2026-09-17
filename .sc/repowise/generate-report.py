#!/usr/bin/env python3
"""Generate the final comprehensive repowise report for sc-compose.

Reads raw JSON from .sc/repowise/data/ and writes the compiled health report
to docs/repowise/health.md.  The version, commit, and date are resolved from
git at run time so the report is always self-describing.
"""
import json, os, subprocess, sys
from pathlib import Path

# ── Git metadata ──────────────────────────────────────────────────────────────
REPO = Path(__file__).resolve().parent.parent.parent

def _git(*args):
    return subprocess.check_output(["git", "-C", str(REPO), *args], text=True).strip()

VERSION    = _git("describe", "--tags", "--always")
COMMIT     = _git("rev-parse", "--short", "HEAD")
GENERATED  = _git("log", "-1", "--format=%cs")

# ── Helpers ───────────────────────────────────────────────────────────────────
DATA_DIR = Path(__file__).resolve().parent / "data"
OUT_DIR  = REPO / "docs" / "repowise"
OUT_PATH = OUT_DIR / "health.md"

def load_json(path):
    with open(path) as f:
        content = f.read()
    for i, c in enumerate(content):
        if c in '{[':
            return json.loads(content[i:])
    raise ValueError(f"No JSON in {path}")

def safe(v, fmt=".1f", default="—"):
    if v is None: return default
    return f"{v:{fmt}}"

h1 = load_json(DATA_DIR / "repowise-health-compose.json")
rt = load_json(DATA_DIR / "repowise-refactoring-targets.json")
dc = load_json(DATA_DIR / "repowise-dead-code.json")
k = h1['kpis']
metrics = sorted(h1['metrics'], key=lambda m: m['score'])
findings = h1['findings']

out = []
def w(*args): out.append(" ".join(str(a) for a in args))
def blank(): out.append("")

w("# sc-compose — Repowise Code Health Analysis")
blank()
w(f"**Version:** {VERSION} | **Commit:** {COMMIT} | **Generated:** {GENERATED}")
w("**Analyzed by:** repowise health + dead-code + refactoring-targets")
blank()

# ── Summary ──
w("## Quick Summary")
blank()
w("| Metric | Value |")
w("|---|---|")
w(f"| Overall Health | **{k['average_health']:.1f}/10** |")
w(f"| Hotspot Health | {k['hotspot_health']:.1f}/10 |")
w(f"| Worst File | `{k['worst_performer_path']}` ({k['worst_performer_score']:.1f}/10) |")
w(f"| Files Indexed | {k['file_count']} |")
w(f"| Biomarker Findings | {len(findings)} |")
w(f"| Dead Code Items | {len(dc)} |")
w(f"| Refactoring Targets | {len(rt.get('targets',[]))} |")
blank()

w("### Health Dimensions")
blank()
w("| Dimension | Average | Hotspot |")
w("|---|---|---|")
w(f"| Maintainability | {k['maintainability_average']:.1f}/10 | {k['maintainability_hotspot']:.1f}/10 |")
w(f"| Performance | {k['performance_average']:.1f}/10 | {k['performance_hotspot']:.1f}/10 |")
w(f"| Overall | {k['average_health']:.1f}/10 | {k['hotspot_health']:.1f}/10 |")
blank()

w("*Interpretation:* 8.2/10 average with 5.2 hotspot health means most files are healthy but a concentrated few drag the score down. Maintainability (9.1) and performance (9.8) average are excellent, but the hotspot maintainability (7.3) reveals files needing modularization. The worst file (`validation.rs`) at 2.6/10 accounts for most of the hotspot drag.")
blank()

# ── Worst files ──
w("## Worst 20 Files by Health Score")
blank()
w("| Score | File | NLOC | CCN | Nest | Dup% |")
w("|---|---|---|---|---|---|")
for m in metrics[:20]:
    w(f"| {m['score']:.1f} | `{m['file_path']}` | {m['nloc']} | {m['max_ccn']} | {safe(m['max_nesting'],'d')} | {safe(m['duplication_pct'])} |")
blank()

w("**Key observations:**")
w(f"- `validation.rs` (2.6/10, 1388 NLOC, CCN=11): the single biggest problem — large, complex, and duplicated (28% dup). This is a prime candidate for decomposition.")
w(f"- Test files dominate the worst list: `cli.rs` (2586 NLOC, 61% dup), `json_cli.rs` (1640 NLOC, 76% dup) — these are expected for thorough testing but the duplication indicates test helper opportunities.")
w(f"- `types.rs` (4.8/10, CCN=11, nest=6): deeply nested validation logic — the 6-deep nesting in `validate_input_value_at` is flagged separately.")
blank()

# ── Best files ──
w("## Best 10 Files (for contrast)")
blank()
w("| Score | File | NLOC |")
w("|---|---|---|")
for m in metrics[-10:][::-1]:
    w(f"| {m['score']:.1f} | `{m['file_path']}` | {m['nloc']} |")
blank()

# ── Biomarkers ──
by_type = {}
for f in findings:
    t = f['biomarker_type']
    by_type.setdefault(t, []).append(f)

w("## Biomarker Findings")
blank()
w("| Type | Count | What It Means |")
w("|---|---|---|")
w("| duplicated_assertion_block | 130 | Repeated assertion patterns — test helper opportunity |")
w("| hot_path_sync_io | 52 | Sync I/O on hot paths — should be async |")
w("| prior_defect | 46 | Files with bug-fix history — strong defect predictor |")
w("| dry_violation | 38 | DRY violations — opportunities to extract shared code |")
w("| error_handling | 30 | Error handling gaps or inconsistencies |")
w("| hidden_coupling | 21 | Implicit dependencies between modules |")
w("| co_change_scatter | 21 | Files that change together → high coupling |")
for btype in sorted(by_type, key=lambda t: -len(by_type[t])):
    if btype in ('duplicated_assertion_block','hot_path_sync_io','prior_defect','dry_violation','error_handling','hidden_coupling','co_change_scatter'):
        continue
    w(f"| {btype} | {len(by_type[btype])} | |")
blank()

# Top biomarker categories in detail
priority_types = ['prior_defect', 'untested_hotspot', 'co_change_scatter', 
                  'churn_risk', 'hidden_coupling', 'error_handling']
for btype in priority_types:
    items = by_type.get(btype, [])
    if not items: continue
    w(f"### {btype} ({len(items)} findings)")
    blank()
    top = sorted(items, key=lambda i: i.get('health_impact',0), reverse=True)[:6]
    for item in top:
        fn = item.get('function_name') or '(top-level)'
        w(f"- **{item['severity']}** `{item['file_path']}` `{fn}`: {item.get('reason','')[:140]}")
    if len(items) > 6:
        w(f"- *... and {len(items)-6} more*")
    blank()

# ── Refactoring Targets ──
w("## Refactoring Targets")
blank()
w("Prioritized by impact-per-effort ratio (highest ROI first).")
blank()
for i, t in enumerate(rt.get('targets',[])[:10]):
    rank = i + 1
    w(f"### #{rank}: `{t['file_path']}` ({t['score']:.1f}/10, {t['nloc']} NLOC)")
    blank()
    w(f"| Metric | Value |")
    w(f"|---|---|")
    w(f"| Biomarker | **{t['primary_biomarker']}** ({t['primary_severity']}) |")
    w(f"| Impact Score | {t['total_impact']:.1f} |")
    w(f"| Effort | {t['effort_bucket']} |")
    w(f"| ROI | {t['impact_per_effort']:.1f} |")
    w(f"| Finding Count | {t['finding_count']} |")
    w(f"| Reason | {t['primary_reason']} |")
    blank()
    plans = t.get('plans', [])
    for plan in plans[:2]:
        ptype = plan.get('refactoring_type', '?')
        pdesc = plan.get('plan', {})
        if isinstance(pdesc, dict):
            desc = pdesc.get('suggestion', pdesc.get('summary', str(pdesc)[:150]))
        else:
            desc = str(pdesc)[:150]
        w(f"- **{ptype}**: {desc}")
    if plans:
        blank()

# ── Dead Code ──
w("## Dead Code Analysis")
blank()
w("**Note:** 51 `unused_export` findings in `bindings/python/python/sc_compose/_native.pyi` are PyO3 auto-generated type stubs — not genuine dead code. They are excluded from the actionable count below.")
blank()

by_kind = {}
for f in dc:
    kk = f['kind']
    by_kind.setdefault(kk, []).append(f)

w("| Kind | Total | Actionable | Action |")
w("|---|---|---|---|")
# unreachable files
unreach = by_kind.get('unreachable_file', [])
actionable_unreach = [f for f in unreach if not f['file_path'].startswith('bindings/python/')]
w(f"| unreachable_file | {len(unreach)} | {len(actionable_unreach)} | Review — may be dead or scripts/prototypes |")
# unused exports (exclude .pyi)
unused = by_kind.get('unused_export', [])
real_unused = [f for f in unused if '.pyi' not in f.get('file_path','')]
w(f"| unused_export | {len(unused)} | {len(real_unused)} | {len(real_unused)} clean-up candidates |")
# zombie packages
zombie = by_kind.get('zombie_package', [])
w(f"| zombie_package | {len(zombie)} | {len(zombie)} | Review prototype/ package |")
blank()

# Actionable unreachable files
if actionable_unreach:
    w("### Unreachable Files")
    blank()
    for f in actionable_unreach:
        risks = ', '.join(f.get('risk_factors', [])[:3]) or 'none'
        w(f"- `{f['file_path']}` ({f.get('lines',0)} lines) — {f.get('reason','')} [risks: {risks}]")
    blank()

# Real unused exports  
if real_unused:
    w("### Actionable Unused Exports (excl. PyO3 stubs)")
    blank()
    for f in real_unused[:10]:
        w(f"- `{f['file_path']}`: `{f.get('symbol_name','?')}` — {f.get('reason','')}")
    if len(real_unused) > 10:
        w(f"- *... and {len(real_unused)-10} more*")
    blank()

# ── Recommendations ──
w("## Top Recommendations")
blank()
w("### 1. Decompose `validation.rs` (2.6/10, 1388 NLOC)")
w("The largest and worst-scoring file. It has 20 biomarker findings, CCN=11, 28% duplication, and co-changes with 24 other files. Split into per-category validators: `var_validation.rs`, `frontmatter_validation.rs`, `include_validation.rs`.")
blank()
w("### 2. Add tests for untested hotspots")
w("11 files flagged as untested hotspots — `path_utils.rs` (16 dependents), `diagnostics.rs` (13 dependents), `cli.rs` (7 dependents). These are heavily depended-upon files with no paired test coverage. Prioritize `path_utils.rs` first (critical severity, highest ROI refactoring target).")
blank()
w("### 3. Extract test helpers (`cli.rs` 61% dup, `json_cli.rs` 76% dup)")
w("130 duplicated assertion blocks in test files — extract shared assertion helpers. The high duplication percentage in test files is expected but the volume (130 findings) signals a real maintenance burden.")
blank()
w("### 4. Address sync I/O on hot paths (52 findings)")
w("52 hot path sync I/O findings — likely from file I/O in the rendering pipeline. Consider async or at minimum document the sync I/O is intentional for CLI tools.")
blank()
w("### 5. Review prototype/ package visibility")
w("The `prototype/` directory is flagged as a zombie package. If actively used for experimentation, add to the repowise config's `annotated` section. Otherwise, consider archiving.")
blank()

# ── Footer ──
w("---")
w("*Generated by repowise v1.x — codebase intelligence for developers. Config: .sc/repowise.yaml with modules `crates/sc-compose`, `crates/sc-composer` and annotated paths `bindings/python`, `prototype`, `scripts`.*")

OUT_DIR.mkdir(parents=True, exist_ok=True)
with open(OUT_PATH, "w") as f:
    f.write("\n".join(out))

print(f"Wrote {len(out)} lines to {OUT_PATH}")
