"""Publish-order, preflight, and inventory commands for the release CLI."""

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path

from release_manifest import load_manifest
from release_workspace import workspace_dependency_names, workspace_path_dependencies


def cmd_list_publish_plan(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    for crate in manifest["crates"]:
        print(f"{crate['package']}|{crate['wait_after_publish_seconds']}")
    return 0


def cmd_list_preflight(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    for crate in manifest["crates"]:
        if crate["publish"] and crate["preflight_check"] == args.mode:
            print(crate["package"])
    return 0


def cmd_validate_preflight_checks(args: argparse.Namespace) -> int:
    workspace_toml = Path(args.workspace_toml)
    workspace_root = workspace_toml.parent
    errors = []
    for crate in load_manifest(Path(args.manifest))["crates"]:
        if crate["preflight_check"] != "full":
            continue
        dependencies = workspace_path_dependencies(
            workspace_root / crate["cargo_toml"], workspace_toml
        )
        if dependencies:
            errors.append(
                f"{crate['artifact']} has workspace path deps ({', '.join(sorted(dependencies))}) "
                "but preflight_check='full'"
            )
    if errors:
        raise SystemExit("\n".join(errors))
    print("ok: all preflight_check='full' crates are genuine leaf crates")
    return 0


def cmd_validate_publish_order(args: argparse.Namespace) -> int:
    workspace_toml = Path(args.workspace_toml)
    workspace_root = workspace_toml.parent
    crates = [crate for crate in load_manifest(Path(args.manifest))["crates"] if crate["publish"]]
    order = {crate["package"]: crate["publish_order"] for crate in crates}
    errors = []
    for crate in crates:
        for dependency in sorted(
            workspace_dependency_names(workspace_root / crate["cargo_toml"], workspace_toml)
        ):
            if dependency in order and order[crate["package"]] <= order[dependency]:
                errors.append(
                    f"{crate['package']} (publish_order={order[crate['package']]}) depends on "
                    f"{dependency} (publish_order={order[dependency]})"
                )
    if errors:
        raise SystemExit("publish_order violation(s):\n  - " + "\n  - ".join(errors))
    print("ok: publish_order matches the workspace dependency graph")
    return 0


def cmd_validate_release_binaries(args: argparse.Namespace) -> int:
    configured = {entry["name"] for entry in load_manifest(Path(args.manifest))["release_binaries"]}
    missing = sorted(set(args.required) - configured)
    if missing:
        raise SystemExit("required release binaries missing from manifest: " + ", ".join(missing))
    print("release binaries validation passed")
    return 0


def cmd_emit_inventory(args: argparse.Namespace) -> int:
    manifest = load_manifest(Path(args.manifest))
    generated_at = args.generated_at or datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    items = []
    for crate in manifest["crates"]:
        if not crate["publish"]:
            continue
        verification = [
            f"cargo search {crate['package']} --limit 1 | grep -F "
            f"'{crate['package']} = \"{args.version}\"'"
        ]
        if crate["verify_install"]:
            verification.append(
                f"cargo install {crate['package']} --version {args.version} --locked --force"
            )
        items.append(
            {
                "artifact": crate["artifact"],
                "version": args.version,
                "sourceRef": args.source_ref,
                "publishTarget": "crates.io",
                "required": crate["required"],
                "publish": crate["publish"],
                "verifyCommands": verification,
            }
        )
    payload = {
        "releaseVersion": args.version,
        "releaseTag": args.tag,
        "releaseCommit": args.commit,
        "generatedAt": generated_at,
        "items": sorted(items, key=lambda item: item["artifact"]),
    }
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    return 0
