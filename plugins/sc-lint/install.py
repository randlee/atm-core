#!/usr/bin/env python3
"""Install the repository-agnostic sc-lint adoption kit."""
from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent
MARKER_START = "# >>> sc-lint managed integration >>>"
MARKER_END = "# <<< sc-lint managed integration <<<"
MANIFEST = Path(".sc-lint/installed.json")
INSTALL_INPUT = Path(".sc-lint/install.json")
RENAMED = {Path("README.md"): Path("README.sc-lint.md")}
WINDOWS_SHELL = 'set windows-shell := ["pwsh", "-NoLogo", "-Command"]\n'
RECIPE = r"^([A-Za-z_][\w-]*)[^:\n]*:(?!=)"
JUST_COLLISION = re.compile(r"(?:recipe|setting) `([^`]+)`", re.I)
TEMPLATES = {
    Path("templates/sc-lint.toml.j2"): Path("sc-lint.toml"),
    Path("templates/Justfile.import.j2"): Path("Justfile"),
    Path("templates/sc-lint-workflow.yml.j2"): Path(".github/workflows/sc-lint.yml"),
}
TEMPLATE_PLACEHOLDERS = {
    "sc-lint.toml.j2": {"minimum_version", "analyzers", "profiles", "test_layers"},
    "Justfile.import.j2": {"windows_shell"},
    "sc-lint-workflow.yml.j2": {"ci_os"},
}
TEMPLATE_VARIABLE = re.compile(r"{{\s*([A-Za-z_][A-Za-z0-9_]*)\s*}}")


@dataclass(frozen=True)
class Desired:
    files: dict[Path, bytes]
    merged: frozenset[Path]
    persisted: frozenset[Path]

    @property
    def special_targets(self) -> frozenset[Path]:
        return self.merged | self.persisted


def _schema_value(value: object, schema: dict, label: str) -> None:
    kind = schema.get("type")
    if kind == "object" and not isinstance(value, dict): raise ValueError(f"{label} must be an object")
    if kind == "string" and not isinstance(value, str): raise ValueError(f"{label} must be a string")
    if kind == "boolean" and type(value) is not bool: raise ValueError(f"{label} must be a boolean")
    if kind == "array" and not isinstance(value, list): raise ValueError(f"{label} must be an array")
    if "pattern" in schema and isinstance(value, str) and not re.fullmatch(schema["pattern"], value): raise ValueError(f"{label} does not match {schema['pattern']}")
    if isinstance(value, dict) and "properties" in schema:
        extra = set(value) - set(schema["properties"])
        if schema.get("additionalProperties") is False and extra: raise ValueError(f"{label} has unknown fields: {sorted(extra)}")
        for key, item in value.items():
            if key in schema["properties"]: _schema_value(item, schema["properties"][key], f"{label}.{key}")
    if isinstance(value, list) and "items" in schema:
        for index, item in enumerate(value): _schema_value(item, schema["items"], f"{label}[{index}]")


def values(path: Path) -> dict:
    data = json.loads(path.read_text())
    schema = json.loads((ROOT / "install.schema.json").read_text())
    if not isinstance(data, dict): raise ValueError("install input must be an object")
    properties = schema["properties"]
    unknown = set(data) - set(properties)
    missing = set(schema["required"]) - set(data)
    if unknown or missing: raise ValueError(f"install input has missing or unknown fields: {sorted(missing | unknown)}")
    for key, value in data.items(): _schema_value(value, properties[key], key)
    return data


def toml_value(value: object) -> str:
    if isinstance(value, dict): return "{ " + ", ".join(f"{key} = {toml_value(item)}" for key, item in value.items()) + " }"
    if isinstance(value, list): return "[" + ", ".join(toml_value(item) for item in value) + "]"
    if isinstance(value, bool): return str(value).lower()
    return json.dumps(value)


def substitute_template_variables(source: str, replacements: dict[str, str]) -> str:
    return TEMPLATE_VARIABLE.sub(
        lambda match: replacements.get(match.group(1), match.group(0)), source
    )


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
    profiles = "\n\n".join(f'[[tool.sc-lint.lint]]\nname = {json.dumps(name)}\ncommand = {json.dumps(command)}' for name, command in data["profiles"].items())
    layers = "\n\n".join(f'[[tool.sc-lint.test]]\nname = {json.dumps(name)}\ncommand = {json.dumps(command)}' for name, command in data.get("test", {}).items())
    runners = [ {"linux":"ubuntu-latest", "macos":"macos-latest", "windows":"windows-latest"}.get(item, item) for item in data["ci"]["os"] ]
    replacements = {
        "minimum_version": data["minimum_version"],
        "analyzers": analyzers,
        "profiles": profiles,
        "test_layers": layers,
        "ci_os": json.dumps(runners),
    }
    deferred = {"windows_shell"} if template.name == "Justfile.import.j2" else set()
    if observed - replacements.keys() != deferred:
        raise ValueError(f"{template.name}: unresolved template variables after rendering")
    return substitute_template_variables(source, replacements)


def unmanaged(text: str) -> str:
    return re.sub(re.escape(MARKER_START) + r".*?" + re.escape(MARKER_END), "", text, flags=re.S)


def justfile_collisions(targets: dict[Path, bytes]) -> list[str]:
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        for relative in (Path("Justfile"), Path(".sc-lint/justfile")):
            (root / relative).parent.mkdir(parents=True, exist_ok=True); (root / relative).write_bytes(targets[relative])
        result = subprocess.run(["just", "--justfile", str(root / "Justfile"), "--working-directory", str(root), "--list"], text=True, capture_output=True, check=False)
    if result.returncode == 0: return []
    kit = set(re.findall(RECIPE, targets[Path(".sc-lint/justfile")].decode(), re.M))
    consumer = set(re.findall(RECIPE, unmanaged(targets[Path("Justfile")].decode()), re.M))
    named = set(JUST_COLLISION.findall(result.stderr))
    return sorted(named | (kit & consumer))


def package_files() -> dict[Path, bytes]:
    return {Path("plugins/sc-lint") / source.relative_to(ROOT): source.read_bytes() for source in ROOT.rglob("*") if source.is_file()}


def desired(data: dict, repo: Path, input_path: Path) -> Desired:
    result = package_files()
    if not data["ci"]["enabled"]:
        result = {path: content for path, content in result.items() if path.parts[2] != ".github"}
    for source in ROOT.rglob("*"):
        if not source.is_file() or source.relative_to(ROOT) in TEMPLATES or source.name in {"install.py", "install.schema.json"} or source.relative_to(ROOT).parts[0] in {"templates", ".claude-plugin", ".github"}: continue
        result[RENAMED.get(source.relative_to(ROOT), source.relative_to(ROOT))] = source.read_bytes()
    for source, target in TEMPLATES.items():
        if source.name == "sc-lint-workflow.yml.j2" and not data["ci"]["enabled"]: continue
        result[target] = render(ROOT / source, data).encode()
    result[INSTALL_INPUT] = json.dumps(data, indent=2, sort_keys=True).encode() + b"\n"
    justfile = repo / "Justfile"; original = justfile.read_text() if justfile.exists() else (ROOT / "templates/Justfile.default").read_text()
    if not re.search(r"^test(?![\w-])[^:\n]*:(?!=)", unmanaged(original), re.M): original = (original.rstrip() + "\n\n" + (ROOT / "templates/test.just").read_text()).lstrip("\n")
    if original.count(MARKER_START) != original.count(MARKER_END) or original.count(MARKER_START) > 1: raise RuntimeError(f"{justfile}: marker conflict")
    shell = data.get("justfile", {}).get("windows_shell", True) and not re.search(r"^set\s+windows-shell\b", unmanaged(original), re.M)
    block = substitute_template_variables(
        render(ROOT / Path("templates/Justfile.import.j2"), data),
        {"windows_shell": WINDOWS_SHELL if shell else ""},
    ).strip()
    result[Path("Justfile")] = (re.sub(re.escape(MARKER_START) + r".*?" + re.escape(MARKER_END), block, original, flags=re.S) if MARKER_START in original else (original.rstrip() + "\n\n" + block).lstrip("\n")).encode()
    persisted = frozenset({INSTALL_INPUT}) if input_path.resolve() == (repo / INSTALL_INPUT).resolve() else frozenset()
    return Desired(result, frozenset({Path("Justfile")}), persisted)


def digest(content: bytes) -> str: return hashlib.sha256(content).hexdigest()


def load_manifest(repo: Path) -> dict[str, str] | None:
    path = repo / MANIFEST
    if not path.exists(): return None
    data = json.loads(path.read_text())
    if not isinstance(data.get("managed"), dict): raise RuntimeError(f"{path}: invalid managed-file manifest")
    return data["managed"]


def main() -> int:
    parser = argparse.ArgumentParser(); parser.add_argument("--input", type=Path); parser.add_argument("--dry-run", action="store_true"); parser.add_argument("repo", type=Path); args = parser.parse_args()
    input_path = args.input or args.repo / ".sc-lint" / "install.json"
    try: data = values(input_path); targets = desired(data, args.repo, input_path); prior = load_manifest(args.repo)
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError) as error: print(f"conflict: {error}", file=sys.stderr); return 2
    previously_managed = prior or {}
    clashes = []
    for relative, expected in targets.files.items():
        path = args.repo / relative
        if (
            relative in targets.special_targets
            or relative.as_posix() in previously_managed
            or not path.exists()
        ):
            continue
        if not path.is_file() or path.read_bytes() != expected:
            clashes.append(path)
    if clashes:
        print(
            "conflict: install would overwrite pre-existing files:\n"
            + "\n".join(f"  {path}" for path in clashes),
            file=sys.stderr,
        )
        return 2
    changes = []
    for relative, expected in targets.files.items():
        path = args.repo / relative; current = path.read_bytes() if path.exists() else b""
        if current != expected:
            if not args.dry_run and prior and relative not in targets.special_targets and relative.as_posix() in prior and digest(current) != prior[relative.as_posix()]: print(f"conflict: {path} was modified outside sc-lint", file=sys.stderr); return 2
            changes.append((path, current, expected))
    if args.dry_run:
        try: collisions = justfile_collisions(targets.files)
        except OSError as error: print(f"conflict: --dry-run needs `just` on PATH to check Justfile collisions: {error}", file=sys.stderr); return 2
        if collisions: print(f"conflict: Justfile collisions: {', '.join(collisions)}", file=sys.stderr); return 2
        for path, old, new in changes: print("".join(difflib.unified_diff(old.decode(errors="replace").splitlines(True), new.decode(errors="replace").splitlines(True), fromfile=str(path), tofile=str(path))))
        return 1 if changes else 0
    manifest = {relative.as_posix(): digest(content) for relative, content in targets.files.items()}
    changes.append((args.repo / MANIFEST, (args.repo / MANIFEST).read_bytes() if (args.repo / MANIFEST).exists() else b"", json.dumps({"managed": manifest}, indent=2, sort_keys=True).encode() + b"\n"))
    for path, _, content in changes:
        path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(content)
        if path.relative_to(args.repo) == Path(".sc-lint/bootstrap"): path.chmod(0o755)
    return 0


if __name__ == "__main__": raise SystemExit(main())
