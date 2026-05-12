# Feature Request: Source-Driven Diagram Panel And Set-Page Generation

## Summary

Add generic `sc-compose` support for source-driven artifact generation where:

- source files such as Mermaid `.mmd` remain the only authored source of truth
- lightweight metadata is stored in the source file itself
- standalone XHTML/HTML fragments can be generated per source file
- aggregate set pages can be generated from those fragments
- no hand-authored per-item HTML is required outside the templates

This is useful for:

- Mermaid diagrams
- SVG documents
- other text assets that need metadata + reusable rendered panels

The motivating use case is a reusable “diagram panel” pattern:

- one standalone XHTML panel per diagram
- one or more aggregate HTML pages that embed those panels
- one copy button per panel that copies AI-ready discussion text derived from
  the same source file

## Why This Matters

Today, `sc-compose` is strong at rendering one template from one vars file.

What is missing for this use case is:

1. treating a directory of source files as a first-class input collection
2. parsing lightweight metadata directly from those source files
3. rendering “one output per source file”
4. rendering aggregate/index pages from the same collection

Without these capabilities, users must add a custom preprocessor script that:

- discovers files
- parses metadata
- reads file contents
- groups sources into sets
- computes output paths
- renders per-item outputs
- renders aggregate pages

That logic is generic enough that it belongs in `sc-compose`.

## Target Outcome

`sc-compose` should support workflows like:

1. author source files only
2. keep metadata inside the source file
3. render standalone fragments from those files
4. render aggregate pages from those fragments
5. never hand-author duplicated HTML per source item

## Source File Example: Mermaid

Example source file:

Path on disk:

- `/Users/randlee/Documents/github/atm-core/docs/atm/diagrams/atm-list.mmd`

Contents:

```text
%% title: `atm list`
%% summary: Bounded metadata-only mailbox query.
%% commentary: This query should start from message-status, exclude deleted rows, collapse superseded rows in normal mode, and fetch only header fields for selected keys.
%% sets: cli,query
flowchart TD
    A[atm list request] --> B[Daemon list handler]
    B --> C1["SQL 1: scan message_status
filter team/agent/query flags
exclude deleted
exclude superseded in normal mode
compute queue counts
select top N message_keys"]
```

Requirements:

- the `%% key: value` lines are metadata
- the Mermaid body remains valid Mermaid
- templates should be able to access:
  - parsed metadata
  - stripped source body
  - raw full source text if needed

## Source File Example: SVG

Example target:

- `/Users/randlee/Documents/github/atm-core/docs/example-diagrams/system-overview.svg`

Possible metadata style:

```svg
<!-- sc-compose:
title: System overview
summary: High-level component relationship diagram.
commentary: Focus on whether the boundary between the daemon and client is clean.
sets: architecture,review
-->
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  ...
</svg>
```

Requirements:

- metadata parsing should not be Mermaid-specific
- the mechanism should support multiple source syntaxes through pluggable
  metadata extraction styles

## Requested Features

### 1. Source Collection Input

Add a first-class collection input mode so templates can iterate over source
files directly.

Conceptual example:

```bash
sc-compose render-many \
  --root /Users/randlee/Documents/github/atm-core \
  --source-glob 'docs/atm/diagrams/*.mmd' \
  --source-kind text-asset \
  --template docs/atm/diagrams/web/panel.xhtml.j2 \
  --output-pattern 'docs/atm/diagrams/panels/{stem}.xhtml'
```

Requested behavior:

- discover files by glob
- expose filename, stem, relative path, absolute path
- expose parsed metadata
- expose body/source text
- render one output per source file

### 2. Metadata Parsing For Arbitrary Text Assets

Add built-in metadata extraction for non-template source files.

At minimum:

- comment-prefix metadata for Mermaid-like files
- block-comment metadata for HTML/SVG/XML-like files
- optionally YAML frontmatter where syntax allows it

Example configuration shape:

```yaml
source_parser:
  mode: comment-prefix
  prefix: "%%"
  format: "key-value"
```

and

```yaml
source_parser:
  mode: block-comment
  start: "<!-- sc-compose:"
  end: "-->"
  format: "yaml"
```

Requested parsed outputs:

- `source.meta.title`
- `source.meta.summary`
- `source.meta.commentary`
- `source.meta.sets`
- `source.body`
- `source.raw`

### 3. Render-Many Command

Add a first-class `render-many` command rather than forcing callers to write a
wrapper script.

Suggested capabilities:

- iterate one template across a source collection
- support output patterns like `{stem}.xhtml`
- optionally emit a machine-readable manifest of rendered outputs

Example:

```bash
sc-compose render-many \
  --root /Users/randlee/Documents/github/atm-core \
  --source-glob 'docs/atm/diagrams/*.mmd' \
  --template docs/atm/diagrams/web/panel.xhtml.j2 \
  --output-pattern 'docs/atm/diagrams/panels/{stem}.xhtml' \
  --manifest-out docs/atm/diagrams/panels/panels.json
```

### 4. Aggregate Rendering From A Source Collection Or Manifest

After per-item rendering, `sc-compose` should be able to render aggregate pages
from the same collection or from the generated manifest.

Example:

```bash
sc-compose render \
  --root /Users/randlee/Documents/github/atm-core \
  --file docs/atm/diagrams/web/set-page.html.j2 \
  --collection-manifest docs/atm/diagrams/panels/panels.json \
  --var title='ATM CLI Interface Diagrams' \
  --output docs/atm/cli-diagrams.html
```

Requested behavior:

- group sources by metadata such as `sets`
- expose rendered fragment paths
- expose source metadata to aggregate templates

### 5. Grouping And Filtering By Metadata

Add grouping/filtering helpers for collection items.

Example use case:

- Mermaid files carry:
  - `sets: cli,query`
  - `sets: client`
- aggregate pages are built by selecting items whose `sets` contain:
  - `cli`
  - `client`
  - `query`

Requested capabilities:

- filter by set/tag membership
- stable ordering by path or metadata field
- optional explicit order field in metadata

### 6. Built-In Path Helpers

Aggregate pages often need relative links to generated fragments and static
assets.

Requested helpers:

- relative path from output page to fragment
- relative path from fragment to shared CSS/JS
- normalized path separators

This avoids custom path math in wrapper scripts.

### 7. Optional Generated Manifest

For each generated fragment, emit a manifest row like:

```json
{
  "source_path": "docs/atm/diagrams/atm-list.mmd",
  "output_path": "docs/atm/diagrams/panels/atm-list.xhtml",
  "stem": "atm-list",
  "meta": {
    "title": "`atm list`",
    "summary": "Bounded metadata-only mailbox query.",
    "commentary": "This query should start from message-status...",
    "sets": ["cli", "query"]
  }
}
```

This manifest can drive:

- aggregate set pages
- validation tooling
- review tooling
- later rebuild steps

### 8. Source-Body Access Without External Scripting

Templates should be able to embed the source body directly.

For example, a panel template should be able to produce:

- visible Mermaid rendering
- hidden source text block for JS copy button logic

without requiring a custom script to read the file contents first.

### 9. One-Shot Package Rendering

Longer term, a single command should be able to:

1. collect sources
2. parse metadata
3. render per-item fragments
4. render aggregate pages
5. emit a manifest

Example:

```bash
sc-compose render-package \
  --root /Users/randlee/Documents/github/atm-core \
  --source-glob 'docs/atm/diagrams/*.mmd' \
  --source-parser mermaid-comment-meta \
  --fragment-template docs/atm/diagrams/web/panel.xhtml.j2 \
  --fragment-output-pattern 'docs/atm/diagrams/panels/{stem}.xhtml' \
  --set-template docs/atm/diagrams/web/set-page.html.j2 \
  --set 'cli=docs/atm/cli-diagrams.html' \
  --set 'client=docs/atm/client-interface-diagrams.html' \
  --set 'query=docs/atm-rusqlite/query-diagrams.html'
```

This is the ideal end state.

## Example Templates

### Standalone Panel Template

Example target path:

- `/Users/randlee/Documents/github/atm-core/docs/atm/diagrams/web/panel.xhtml.j2`

This template should receive:

- title
- summary
- commentary
- ssot path
- diagram source text
- shared CSS/JS asset paths

It renders one independent XHTML panel that:

- shows the diagram
- shows commentary
- has a copy button
- keeps the source file as the only authored diagram source

### Aggregate Set Page Template

Example target path:

- `/Users/randlee/Documents/github/atm-core/docs/atm/diagrams/web/set-page.html.j2`

This template should receive:

- page title
- page intro
- a list of generated fragment paths

It renders a lightweight page that embeds independent panels, for example via
`iframe` or another clean inclusion mechanism.

## Example Output Layout

Using the ATM repo as an example target layout:

- source diagrams:
  - `/Users/randlee/Documents/github/atm-core/docs/atm/diagrams/*.mmd`
- shared panel assets:
  - `/Users/randlee/Documents/github/atm-core/docs/atm/diagrams/web/diagram-panels.css`
  - `/Users/randlee/Documents/github/atm-core/docs/atm/diagrams/web/diagram-panels.js`
- generated independent panels:
  - `/Users/randlee/Documents/github/atm-core/docs/atm/diagrams/panels/*.xhtml`
- generated set pages:
  - `/Users/randlee/Documents/github/atm-core/docs/atm/cli-diagrams.html`
  - `/Users/randlee/Documents/github/atm-core/docs/atm/client-interface-diagrams.html`
  - `/Users/randlee/Documents/github/atm-core/docs/atm-rusqlite/query-diagrams.html`

## Non-Goals

This request is not asking for:

- Mermaid rendering inside `sc-compose`
- browser automation
- site generation beyond source-driven fragments/pages

The browser can still render Mermaid client-side.

The requested value is:

- source collection
- metadata parsing
- render-many
- aggregate rendering
- path helpers
- manifest support

## Minimal Viable Version

If this should be phased, the MVP would be:

1. collection input by glob
2. metadata parsing from comment-based frontmatter
3. render-many with output pattern
4. generated manifest
5. aggregate render from manifest

That alone would eliminate most of the custom scripting required for this
diagram-panel workflow.

## Why This Is General-Purpose

This is not Mermaid-only.

The same pattern applies to:

- Mermaid `.mmd`
- SVG `.svg`
- Markdown snippets
- HTML/XHTML fragments
- other structured text artifacts with embedded metadata

The generic abstraction is:

- “source text asset with embedded metadata”
- “render one artifact per source”
- “render one or more aggregate pages from the resulting collection”

That abstraction fits `sc-compose` well.
