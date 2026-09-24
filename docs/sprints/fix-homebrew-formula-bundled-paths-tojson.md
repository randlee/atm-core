---
status: complete
branch: fix/homebrew-formula-bundled-paths-tojson
worktree: /Users/randlee/Documents/github/sc-publish-worktrees/fix/homebrew-formula-bundled-paths-tojson
---

# FIX: Homebrew formula generator emits invalid Ruby for bundled-path destinations (canonical source)

## Source

This is the upstream/canonical counterpart of a live production defect. The
downstream implementation is on `randlee/sc-compose` PR #603 (commits
`85f7dd7`, `d4ada7a`) and remains open at this writing.

`sc-compose` vendors a copy of this repo's Homebrew formula template and
already patched its own copy plus its vendored copy. However, **every other
consumer of `sc-publish` installs the Homebrew channel logic from this
canonical repo**, not from sc-compose's vendored copy. Until this repo's own
template is fixed, the bug remains live for all of those other consumers —
this is the "make sure it never happens again for all sc-publish customers"
fix.

The canonical template is
`plugins/sc-publish/release/homebrew/formula.rb.j2`. The `bundled_paths` loop:

```jinja
{% for bundle in bundled_paths %}
    ({% for component in bundle.destination_components %}{{ component | tojson }}{% if not loop.last %}/{% endif %}{% endfor %}).install Dir[{{ bundle.source_glob | tojson }}]
{% endfor %}
```

`component | tojson` wraps each destination component in double quotes. For a
single-component destination (e.g. `["pkgshare"]`), this renders
`("pkgshare")` — a Ruby string literal — instead of the bare Ruby identifier
`pkgshare` that Homebrew's Formula DSL requires as a method call. Calling
`.install` on that quoted string raises `NoMethodError: undefined method
'install' for an instance of String` for every `brew install`/`brew upgrade`
on every platform, for any release built from this template with a
single-component bundled path.

## Required Fix (deliverables)

- Apply the identical fix already validated in `sc-compose` PR #603: render
  bundled-path destination components as bare Ruby method-call identifiers,
  not `tojson`-quoted string literals, while any destination component that is
  genuinely meant to be a literal path string (if any such case exists in this
  template) still renders correctly.
- Update `plugins/sc-publish/.github/scripts/tests/test_release_artifacts.py`
  and/or `test_install.py`: they currently only substring-match for
  `"pkgshare"` and did not catch this. Add a regression test that actually
  parses/validates the generated `formula.rb` as Ruby (e.g. `ruby -c`) and, for
  at least one single-component bundled-path fixture, executes the generated
  `install` method's dispatch to confirm it resolves to a real Formula DSL
  method call rather than a string literal.
- Grep this template (and any sibling `.rb.j2`/similar DSL-generating
  templates in this repo) for the same `tojson`-on-bare-identifier pattern and
  fix or flag any other occurrence.
- Do not otherwise diverge this template's behavior for multi-component
  bundled paths or any other rendering path.

## Acceptance Criteria

- A freshly rendered formula for a single-component bundled path (e.g.
  `pkgshare`) produces the bare identifier `pkgshare.install Dir[...]`, not
  `("pkgshare").install Dir[...]`.
- The new/updated regression test fails against the old (buggy) template and
  passes against the fixed template.
- No other template in this repo shares the same `tojson`-on-bare-identifier
  bug pattern (or any found instances are fixed/flagged in the report).

## Validation

- The relevant Python test target for `plugins/sc-publish/.github/scripts/tests/`
  passes, including the new regression test.
- Render the fixed template locally against a real single-component
  bundled-path fixture and confirm the output is valid Ruby (`ruby -c`).

## Audit and validation

- Sibling-template sweep: `rg -n 'component \\| tojson|destination_components'
  plugins/sc-publish/release -g '*.j2'` found no other template using a
  `tojson`-quoted value where a Ruby DSL helper is required. The retained
  `tojson` on this template's non-first components deliberately quotes literal
  path segments.
- The regression renders and executes both a single-component destination and
  a three-component destination through `ruby -c` and the Formula-DSL harness.

## References

- Downstream fix in review: `randlee/sc-compose` PR #603, commits `85f7dd7`,
  `d4ada7a`, sprint doc
  `docs/sprints/fix-homebrew-formula-bundled-paths-tojson.md`.
