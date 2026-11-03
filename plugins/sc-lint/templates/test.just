# sc-lint's recommended `test` recipe: runs the selected `[tool.sc-lint.test]` selector (or its
# default) in sc-lint.toml; words after the recipe name are forwarded as
# `[--fail-fast|--list] [selector [args...]]`.
# This repository owns it; change or replace it as needed.
test *layer:
    {{python}} .sc-lint/bootstrap.py test --config sc-lint.toml {{layer}}
