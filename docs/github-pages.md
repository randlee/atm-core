# GitHub Pages verification site

The repository's public verification site is the generated `site/` tree. The
home page at [`site/index.html`](../site/index.html) links to the durable
report catalog at [`site/reports/index.html`](../site/reports/index.html).
Report producers regenerate and validate that catalog with:

```bash
just reports-index
just reports-index --check
```

The sole publisher is [`.github/workflows/pages.yml`](../.github/workflows/pages.yml).
It runs on pushes to `develop` that touch `site/`, validates the generated
pages and links, uploads only `site/`, and deploys that artifact with the
official GitHub Pages Actions. There is no second workflow, branch publisher,
or transient `artifacts/view` publisher.

## Publish gate

[`.just/check_site_generated.py`](../.just/check_site_generated.py) is the one
definition of "Pages can publish this tree": procedure pages and the report
catalog must match what their generators produce. It runs in three places so a
tree Pages would reject never reaches `develop`:

- **Pre-push hook:** a push touching site inputs runs it, and `check_site_links.py`, against the pushed commit.
- **`just lint site-generated`:** part of `just lint`, and so of the CI lint job, before merge.
- **`pages.yml`:** after merge, as the deploy's own build check.

Generated pages resolve procedure revisions from git ancestry, so every job
that runs the gate checks out full history (`fetch-depth: 0`). The generator
refuses a shallow checkout rather than silently selecting different pages.

## Repository setting

In the repository's **Settings → Pages → Build and deployment**, set **Source**
to **GitHub Actions**. This is the only repository-level Pages setting
required; the workflow owns the build and deployment. The workflow publishes
updates from `develop` and can also be started manually.

Do not select **Deploy from a branch**: that would create an alternate
publisher and bypass the generated-index check.

## Link verification

Run `just lint site-links` to check every local link and asset reference in
the generated `site/` tree. The check rejects missing targets, links that
escape the site tree, root-absolute paths, and directory links without an
`index.html`.

Rendered pages under `site/reports/` may be edited only to repair links;
runner-written JSON and envelope artifacts are never edited.
