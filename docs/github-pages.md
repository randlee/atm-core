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
It validates the generated catalog, uploads only `site/`, and deploys that
artifact with the official GitHub Pages Actions. There is no second workflow,
branch publisher, or transient `artifacts/view` publisher.

## Repository setting

In the repository's **Settings → Pages → Build and deployment**, set **Source**
to **GitHub Actions**. This is the only repository-level Pages setting
required; the workflow owns the build and deployment. The workflow publishes
updates from `integrate/phase-ai-31-33` and can also be started manually.

Do not select **Deploy from a branch**: that would create an alternate
publisher and bypass the generated-index check.
