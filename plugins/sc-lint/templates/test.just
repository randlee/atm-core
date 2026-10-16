# sc-lint's recommended `test` recipe: runs the `[[tool.sc-lint.test]]` layers in sc-lint.toml.
# This repository owns it; change or replace it as needed.
test *layer:
    {{bootstrap_command}} test --config sc-lint.toml {{layer}}
