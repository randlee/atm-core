#!/usr/bin/env python3
"""Install the repository-agnostic sc-lint adoption kit."""
from __future__ import annotations

import argparse
import difflib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
RENAMED = {Path("README.md"): Path("README.sc-lint.md")}
# Plugin components sit at the package root so Claude Code's plugin loader finds
# them; a consumer repository receives them under .claude/ as before.
PLUGIN_COMPONENTS = frozenset({"skills", "agents"})
WINDOWS_SHELL = 'set windows-shell := ["cmd.exe", "/c"]\n'
PYTHON_BINDING = 'python := if os_family() == "windows" { "python" } else { "python3" }\n'
TEMPLATES = {
    Path("templates/sc-lint.toml.tmpl"): Path("sc-lint.toml"),
    Path("templates/sc-lint-workflow.yml.tmpl"): Path(".github/workflows/sc-lint.yml"),
}
HELPER_POLICY_TABLES = ("line_counts", "identities", "version_sync", "cargo_shear", "logging", "artifacts")

TEMPLATE_PLACEHOLDERS = {
    "sc-lint.toml.tmpl": {"tool_settings", "analyzers", "profiles", "test_layers", "helper_policies"},
    "Justfile.import.tmpl": {"windows_shell", "python_binding"},
    "sc-lint-workflow.yml.tmpl": {"ci_os", "lint_steps"},
}
TEMPLATE_VARIABLE = re.compile(r"{{\s*([A-Za-z_][A-Za-z0-9_]*)\s*}}")
CATALOG_DIRECTORY = ROOT / "analyzer-catalog"
SCHEMA_DIRECTORY = ROOT / "schema.d"


def merge_schema(base: dict, overlay: dict, fragment: Path, label: str = "") -> dict:
    result = dict(base)
    for key, value in overlay.items():
        path = f"{label}.{key}" if label else key
        if key not in result:
            result[key] = value
        elif isinstance(result[key], dict) and isinstance(value, dict):
            result[key] = merge_schema(result[key], value, fragment, path)
        elif result[key] != value:
            raise ValueError(f"schema conflict in {fragment.name} at {path}")
    return result


def load_schema() -> dict:
    schema = json.loads((ROOT / "install.schema.json").read_text())
    if not SCHEMA_DIRECTORY.exists():
        return schema
    for overlay_path in sorted(SCHEMA_DIRECTORY.glob("*.json")):
        schema = merge_schema(schema, json.loads(overlay_path.read_text()), overlay_path)
    return schema


def load_catalogs() -> dict[str, dict]:
    catalogs = {}
    for path in sorted(CATALOG_DIRECTORY.glob("*.json")):
        catalog = json.loads(path.read_text())
        name = catalog.get("analyzer")
        if not isinstance(name, str) or name in catalogs:
            raise ValueError(f"invalid analyzer catalog: {path}")
        if not isinstance(catalog.get("rules"), list) or not isinstance(catalog.get("families"), dict):
            raise ValueError(f"invalid analyzer catalog: {path}")
        catalogs[name] = catalog
    return catalogs


def _schema_value(value: object, schema: dict, label: str) -> None:
    kind = schema.get("type")
    accepted_types = kind if isinstance(kind, list) else [kind]
    matches_type = {
        "object": lambda item: isinstance(item, dict),
        "string": lambda item: isinstance(item, str),
        "boolean": lambda item: type(item) is bool,
        "array": lambda item: isinstance(item, list),
    }
    if kind is not None and not any(matches_type[name](value) for name in accepted_types):
        articles = {"object": "an object", "string": "a string", "boolean": "a boolean", "array": "an array"}
        expected = " or ".join(articles[name] for name in accepted_types)
        raise ValueError(f"{label} must be {expected}")
    if "enum" in schema and value not in schema["enum"]:
        raise ValueError(f"{label} must be one of {schema['enum']}")
    if isinstance(value, str) and len(value) < schema.get("minLength", 0):
        raise ValueError(f"{label} must contain at least {schema['minLength']} characters")
    if "pattern" in schema and isinstance(value, str) and not re.fullmatch(schema["pattern"], value): raise ValueError(f"{label} does not match {schema['pattern']}")
    if "oneOf" in schema:
        errors = []
        for option in schema["oneOf"]:
            try: _schema_value(value, option, label)
            except ValueError as error: errors.append(str(error))
        if len(errors) != len(schema["oneOf"]) - 1: raise ValueError(f"{label} must match exactly one accepted shape: {'; '.join(errors)}")
    if isinstance(value, dict):
        properties = schema.get("properties", {})
        extra = set(value) - set(properties)
        if schema.get("additionalProperties") is False and extra: raise ValueError(f"{label} has unknown fields: {sorted(extra)}")
        missing = set(schema.get("required", [])) - set(value)
        if missing: raise ValueError(f"{label} has missing required fields: {sorted(missing)}")
        if "propertyNames" in schema:
            for key in value:
                _schema_value(key, schema["propertyNames"], f"{label} key")
        for key, item in value.items():
            if key in properties:
                _schema_value(item, properties[key], f"{label}.{key}")
            elif isinstance(schema.get("additionalProperties"), dict):
                _schema_value(item, schema["additionalProperties"], f"{label}.{key}")
    if isinstance(value, list) and "items" in schema:
        if len(value) < schema.get("minItems", 0):
            raise ValueError(f"{label} must contain at least {schema['minItems']} items")
        for index, item in enumerate(value): _schema_value(item, schema["items"], f"{label}[{index}]")


def values(path: Path) -> dict:
    data = json.loads(path.read_text())
    schema = load_schema()
    if not isinstance(data, dict): raise ValueError("install input must be an object")
    properties = schema["properties"]
    unknown = set(data) - set(properties)
    missing = set(schema["required"]) - set(data)
    if unknown or missing: raise ValueError(f"install input has missing or unknown fields: {sorted(missing | unknown)}")
    for key, value in data.items(): _schema_value(value, properties[key], key)
    for analyzer, settings in data["analyzers"].items():
        catalog = load_catalogs()[analyzer]
        rules = set(catalog["rules"])
        selectors = rules | set(catalog["families"])
        for key in ("exclude_rules", "fail_rules"):
            invalid = set(settings.get(key, [])) - selectors
            if invalid: raise ValueError(f"analyzers.{analyzer}.{key} has unknown rules: {sorted(invalid)}")
        invalid = set(settings.get("exclude_paths_by_rule", {})) - rules
        if invalid: raise ValueError(f"analyzers.{analyzer}.exclude_paths_by_rule has unknown exact rules: {sorted(invalid)}")
    return data


def toml_value(value: object) -> str:
    if value is None: raise ValueError("TOML configuration cannot contain null")
    if isinstance(value, dict): return "{ " + ", ".join(f"{toml_value(key)} = {toml_value(item)}" for key, item in value.items()) + " }"
    if isinstance(value, list): return "[" + ", ".join(toml_value(item) for item in value) + "]"
    if isinstance(value, bool): return str(value).lower()
    return json.dumps(value, ensure_ascii=False, allow_nan=False).replace("\x7f", "\\u007f")


def substitute_template_variables(source: str, replacements: dict[str, str]) -> str:
    return TEMPLATE_VARIABLE.sub(
        lambda match: replacements.get(match.group(1), match.group(0)), source
    )


def toml_key(name: str) -> str:
    return name if re.fullmatch(r"[A-Za-z0-9_-]+", name) else toml_value(name)


def lint_profiles_table(lint_profiles: dict) -> str:
    if not lint_profiles: return ""
    rows = "\n".join(f"{toml_key(name)} = {toml_value(members)}" for name, members in lint_profiles.items())
    return f"[tool.sc-lint.lint_profiles]\n{rows}"


def lint_step(name: str, step: object) -> str:
    """Render one lint row; cross-target and command semantics belong to sc-lint."""
    command, tags = (step, {}) if isinstance(step, list) else (step["command"], step)
    lines = [f"name = {toml_value(tags.get('name', name))}", f"command = {toml_value(command)}"]
    lines += [f"{key} = {toml_value(tags[key])}" for key in ("platform", "target", "host", "after_all", "env", "env_paths", "cwd") if key in tags]
    return "[[tool.sc-lint.lint]]\n" + "\n".join(lines)


def test_layer(name: str, step: object) -> str:
    """Renders one test row using the fields accepted by the Rust test-step model."""
    command, fields = (step, {}) if isinstance(step, list) else (step["command"], step)
    lines = [f"name = {toml_value(name)}", f"command = {toml_value(command)}"]
    if "after_all" in fields:
        lines.append(f"after_all = {toml_value(fields['after_all'])}")
    return "[[tool.sc-lint.test]]\n" + "\n".join(lines)


def lint_workflow_steps(data: dict) -> str:
    """Resolves the CI lint invocation: a declared `ci` profile runs every platform on Linux, host-only elsewhere."""
    indent = "      "
    if "ci" in data.get("lint_profiles", {}):
        return (f"- run: just lint ci all\n{indent}  if: runner.os == 'Linux'\n"
                f"{indent}- run: just lint ci\n{indent}  if: runner.os != 'Linux'")
    return "- run: just lint ci" if "ci" in data["profiles"] else "- run: just lint"


def render(template: Path, data: dict) -> str:
    source = template.read_text()
    expected = TEMPLATE_PLACEHOLDERS.get(template.name)
    if expected is None:
        raise ValueError(f"{template.name}: no declared template variables")
    observed = set(TEMPLATE_VARIABLE.findall(source))
    missing = expected - observed
    unknown = observed - expected
    if missing or unknown:
        problems = []
        if missing: problems.append(f"missing template variables: {sorted(missing)}")
        if unknown: problems.append(f"unknown template variables: {sorted(unknown)}")
        raise ValueError(f"{template.name}: {'; '.join(problems)}")
    analyzers = "\n\n".join(
        f"[tool.sc-lint.analyzers.{name}]\n" + "\n".join(f"{key} = {toml_value(item)}" for key, item in value.items())
        for name, value in data["analyzers"].items()
    )
    profiles = "\n\n".join(filter(None, [lint_profiles_table(data.get("lint_profiles", {})), *(lint_step(name, step) for name, step in data["profiles"].items())]))
    layers = "\n\n".join(test_layer(name, step) for name, step in data.get("test", {}).items())
    runners = [ {"linux":"ubuntu-latest", "macos":"macos-latest", "windows":"windows-latest"}.get(item, item) for item in data["ci"]["os"] ]
    policy_tables = [f"[{name}]\n" + "\n".join(f"{toml_key(key)} = {toml_value(value)}" for key, value in data[name].items())
                     for name in HELPER_POLICY_TABLES if name in data]
    tool_settings = {"minimum_version": data["minimum_version"]}
    if "default_lint_profile" in data:
        tool_settings["default_lint_profile"] = data["default_lint_profile"]
    replacements = {
        "windows_shell": WINDOWS_SHELL if data.get("justfile", {}).get("windows_shell", True) else "",
        "python_binding": PYTHON_BINDING,
        "helper_policies": "\n\n".join(policy_tables) + "\n\n" if policy_tables else "",
        "tool_settings": "\n".join(f"{key} = {toml_value(value)}" for key, value in tool_settings.items()),
        "analyzers": analyzers,
        "profiles": profiles,
        "test_layers": layers,
        "ci_os": json.dumps(runners),
        "lint_steps": lint_workflow_steps(data),
    }
    if observed - replacements.keys():
        raise ValueError(f"{template.name}: unresolved template variables after rendering")
    return substitute_template_variables(source, replacements)


def package_files() -> dict[Path, bytes]:
    return {
        Path("plugins/sc-lint") / source.relative_to(ROOT): source.read_bytes()
        for source in ROOT.rglob("*")
        if source.is_file() and "__pycache__" not in source.parts and source.suffix != ".pyc"
    }


def consumer_path(relative: Path) -> Path:
    if relative.parts[0] in PLUGIN_COMPONENTS:
        return Path(".claude", *relative.parts)
    return RENAMED.get(relative, relative)


def desired(data: dict, repo: Path) -> dict[Path, bytes]:
    result = package_files()
    if not data["ci"]["enabled"]:
        result = {path: content for path, content in result.items() if path.parts[2] != ".github"}
    for source in ROOT.rglob("*"):
        if not source.is_file() or "__pycache__" in source.parts or source.suffix == ".pyc" or source.relative_to(ROOT) in TEMPLATES or source.name in {"install.py", "install.schema.json"} or source.relative_to(ROOT).parts[0] in {"tests", "templates", ".claude-plugin", ".github", "schema.d", "analyzer-catalog"}: continue
        result[consumer_path(source.relative_to(ROOT))] = source.read_bytes()
    for source, target in TEMPLATES.items():
        if source.name == "sc-lint-workflow.yml.tmpl" and not data["ci"]["enabled"]: continue
        result[target] = render(ROOT / source, data).encode()
    result[Path("Justfile")] = (
        (ROOT / "templates/Justfile.default").read_text()
        + "\n" + (ROOT / "templates/test.just").read_text()
        + "\n" + render(ROOT / "templates/Justfile.import.tmpl", data)
    ).encode()
    return {relative: content for relative, content in result.items()
            if not (repo / relative).exists()}


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Seed missing sc-lint adoption files in REPO.",
        epilog=(
            "Existing files are left untouched. Exit codes: 0 seeded or already present; "
            "1 --dry-run found missing files (nothing written); 2 invalid input."
        ),
    )
    parser.add_argument("--input", type=Path, help="install input JSON; defaults to REPO/.sc-lint/install.json")
    parser.add_argument("--dry-run", action="store_true", help="show missing files without writing")
    parser.add_argument("repo", type=Path, help="consumer repository root")
    args = parser.parse_args()
    input_path = args.input or args.repo / ".sc-lint" / "install.json"
    try: targets = desired(values(input_path), args.repo)
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error: print(f"conflict: {error}", file=sys.stderr); return 2
    if args.dry_run:
        for relative, content in targets.items():
            path = args.repo / relative
            print("".join(difflib.unified_diff([], content.decode(errors="replace").splitlines(True), fromfile=str(path), tofile=str(path))))
        return 1 if targets else 0
    for relative, content in targets.items():
        path = args.repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb") as output:
            output.write(content)
    return 0


if __name__ == "__main__": raise SystemExit(main())
