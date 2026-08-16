"""Declarative per-profile installer for the Hermes ATM gateway hook."""

from __future__ import annotations

import argparse
from importlib import import_module
import json
import os
from pathlib import Path
import plistlib
import sys
from typing import Any, Mapping, Sequence

HOOK_NAME = "hermes-atm"
HOOK_MANIFEST = """name: hermes-atm
description: Deliver ATM graft nudges through the Hermes public gateway runner API.
events:
  - gateway:startup
  - gateway:shutdown
"""

INSTALLER_DESCRIPTION = """Install the standard Hermes ATM gateway hook.

The receiver workspace root must exactly match the recipient's ATM roster
`workspace_root`.  Before resetting the gateway, publish it with:

  atm teams update-member <team> <identity> \\
    --home-dir <profile-home> \\
    --workspace-root <workspace-root> \\
    --harness hermes

The installer writes the receiver hook and the package-owned native-tools plugin.
Do not hand-edit those generated files; rerun this command after package changes.
See the distribution README for the complete, safe install and proof sequence.
"""
PYTHON_SITE_PACKAGES_BOOTSTRAP = """import sys
import sysconfig

# Hermes loads extension modules dynamically. Ensure that loader can resolve
# packages installed into the interpreter that owns this gateway process.
_site_packages = sysconfig.get_paths()["purelib"]
if _site_packages not in sys.path:
    sys.path.insert(0, _site_packages)
"""

HOOK_HANDLER = f"""from pathlib import Path
{PYTHON_SITE_PACKAGES_BOOTSTRAP}

from hermes_atm.hook import handle as _handle


async def handle(event_type, context):
    return await _handle(event_type, context, Path(__file__).with_name(\"config.json\"))
"""

TOOLS_PLUGIN_NAME = "hermes-atm-native-tools"
TOOLS_TOOLSET_NAME = "atm"
TOOLS_PLUGIN_MANIFEST = """name: hermes-atm-native-tools
version: 1
description: Native ATM mailbox tools backed by the typed atm-graft client.
hermes_version: ">=0.20"
kind: backend
provides_tools:
  - atm_send
  - atm_read
  - atm_list
"""
TOOLS_PLUGIN_HANDLER = f"""from __future__ import annotations

import json
from pathlib import Path
{PYTHON_SITE_PACKAGES_BOOTSTRAP}

from hermes_atm.native_tools import register_tools


def register(context):
    config = json.loads(Path(__file__).with_name("config.json").read_text(encoding="utf-8"))
    register_tools(
        context,
        identity=config["identity"],
        team=config["team"],
        chat_id=config["chat_id"],
    )
"""


class HermesAtmInstallError(RuntimeError):
    """The requested profile cannot be safely configured."""


def _required(value: str | None, name: str) -> str:
    normalized = (value or "").strip()
    if not normalized:
        raise HermesAtmInstallError(f"{name} is required")
    return normalized


def _read_launch_agent_python(path: Path) -> str:
    try:
        with path.open("rb") as source:
            plist = plistlib.load(source)
    except (OSError, plistlib.InvalidFileException) as error:
        raise HermesAtmInstallError(f"cannot read launch agent {path}: {error}") from error
    arguments = plist.get("ProgramArguments")
    if (
        not isinstance(arguments, list)
        or not arguments
        or not isinstance(arguments[0], str)
    ):
        raise HermesAtmInstallError(
            f"launch agent {path} has no ProgramArguments Python executable"
        )
    return arguments[0]


def _config_apis() -> tuple[Any, Any]:
    """Return Hermes' public config APIs or fail before profile mutation."""

    try:
        from hermes_cli.config import load_config, save_config
    except ImportError as error:
        raise HermesAtmInstallError(
            "the active interpreter cannot import Hermes public config APIs"
        ) from error
    return load_config, save_config


def _require_public_capability(
    module_name: str,
    class_name: str,
    member_name: str,
    *,
    member_must_be_callable: bool = True,
) -> Any:
    """Load one documented Hermes capability with consistent fail-closed errors."""

    qualified_class = f"{module_name}.{class_name}"
    qualified_member = f"{qualified_class}.{member_name}"
    try:
        module = import_module(module_name)
    except ImportError as error:
        raise HermesAtmInstallError(
            f"the active interpreter cannot import {qualified_class}"
        ) from error
    capability = getattr(module, class_name, None)
    if capability is None:
        raise HermesAtmInstallError(
            f"the active interpreter cannot import {qualified_class}"
        )
    member = getattr(capability, member_name, None)
    if member is None or (member_must_be_callable and not callable(member)):
        raise HermesAtmInstallError(
            f"Hermes gateway does not expose public {qualified_member}"
        )
    return capability


def validate_host_capability(*, launch_agent_plist: Path | None = None) -> None:
    """Fail before profile mutation unless this interpreter is a supported host."""

    _require_public_capability("gateway.run", "GatewayRunner", "inject_internal_message")
    _require_public_capability(
        "gateway.config", "Platform", "TELEGRAM", member_must_be_callable=False
    )
    _require_public_capability("hermes_cli.plugins", "PluginContext", "register_tool")
    _config_apis()
    if launch_agent_plist is not None:
        configured = Path(_read_launch_agent_python(launch_agent_plist)).resolve()
        active = Path(sys.executable).resolve()
        if configured != active:
            raise HermesAtmInstallError(
                f"launch agent uses {configured}, but installer runs under {active}"
            )


def _write_text_if_changed(path: Path, content: str) -> bool:
    if path.exists() and path.read_text(encoding="utf-8") == content:
        return False
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(content, encoding="utf-8")
    temporary.replace(path)
    return True


def _enable_native_tools_plugin(profile_home: Path) -> bool:
    """Enable the package-owned plugin and its toolset for Hermes sessions.

    Hermes discovers user plugins from ``<HERMES_HOME>/plugins`` but does not
    load them until their path-derived key is in ``plugins.enabled``.  A
    plugin-provided tool is visible to a model session only when its toolset is
    also enabled for that platform.  Match Hermes' public plugin-enable flow:
    enable the plugin and add its toolset to every configured platform (or the
    CLI platform when the profile has not saved platform choices yet).
    """

    load_config, save_config = _config_apis()

    # Hermes' config API deliberately derives the active profile from
    # HERMES_HOME.  Scope the override to this synchronous installer call so
    # the caller's process environment is restored even when validation fails.
    previous_home = os.environ.get("HERMES_HOME")
    os.environ["HERMES_HOME"] = str(profile_home)
    try:
        config = load_config()
        if not isinstance(config, dict):
            raise HermesAtmInstallError("Hermes profile config must be a mapping")
        plugins = config.setdefault("plugins", {})
        if not isinstance(plugins, dict):
            raise HermesAtmInstallError("Hermes profile plugins configuration must be a mapping")
        enabled = plugins.setdefault("enabled", [])
        if not isinstance(enabled, list) or not all(isinstance(item, str) for item in enabled):
            raise HermesAtmInstallError("Hermes profile plugins.enabled must be a list of names")
        changed = False
        if TOOLS_PLUGIN_NAME not in enabled:
            enabled.append(TOOLS_PLUGIN_NAME)
            changed = True

        platform_toolsets = config.get("platform_toolsets")
        if platform_toolsets is None:
            platform_toolsets = {}
            config["platform_toolsets"] = platform_toolsets
        if not isinstance(platform_toolsets, dict):
            raise HermesAtmInstallError(
                "Hermes profile platform_toolsets must be a mapping"
            )

        configured_platforms = [
            toolsets
            for toolsets in platform_toolsets.values()
            if isinstance(toolsets, list)
        ]
        if not configured_platforms:
            platform_toolsets["cli"] = [TOOLS_TOOLSET_NAME]
            changed = True
        else:
            for toolsets in configured_platforms:
                if TOOLS_TOOLSET_NAME not in toolsets:
                    toolsets.append(TOOLS_TOOLSET_NAME)
                    changed = True

        if changed:
            save_config(config, merge_existing=True)
        return changed
    finally:
        if previous_home is None:
            os.environ.pop("HERMES_HOME", None)
        else:
            os.environ["HERMES_HOME"] = previous_home


def install_profile(
    *,
    profile_home: Path,
    profile: str,
    identity: str,
    team: str,
    chat_id: str,
    atm_home: str,
    workspace_root: str,
    launch_agent_plist: Path | None = None,
) -> Mapping[str, Any]:
    """Validate the host then materialize the standard declarative profile hook."""

    profile = _required(profile, "profile")
    config = {
        "schema_version": 1,
        "profile": profile,
        "atm_home": _required(atm_home, "ATM_HOME"),
        "identity": _required(identity, "ATM_IDENTITY"),
        "team": _required(team, "ATM_TEAM"),
        "chat_id": _required(chat_id, "ATM_CHAT_ID"),
        "workspace_root": _required(workspace_root, "ATM_WORKSPACE_ROOT"),
    }
    validate_host_capability(launch_agent_plist=launch_agent_plist)
    hook_dir = profile_home / "hooks" / HOOK_NAME
    plugin_dir = profile_home / "plugins" / TOOLS_PLUGIN_NAME
    hook_dir.mkdir(parents=True, exist_ok=True)
    plugin_dir.mkdir(parents=True, exist_ok=True)
    changed = any(
        (
            _write_text_if_changed(hook_dir / "HOOK.yaml", HOOK_MANIFEST),
            _write_text_if_changed(hook_dir / "handler.py", HOOK_HANDLER),
            _write_text_if_changed(
                hook_dir / "config.json",
                json.dumps(config, indent=2, sort_keys=True) + "\n",
            ),
            _write_text_if_changed(plugin_dir / "plugin.yaml", TOOLS_PLUGIN_MANIFEST),
            _write_text_if_changed(plugin_dir / "__init__.py", TOOLS_PLUGIN_HANDLER),
            _write_text_if_changed(
                plugin_dir / "config.json",
                json.dumps(config, indent=2, sort_keys=True) + "\n",
            ),
        )
    )
    changed = _enable_native_tools_plugin(profile_home) or changed
    return {
        "hook_dir": str(hook_dir),
        "plugin_dir": str(plugin_dir),
        "changed": changed,
        "config": config,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="python -m hermes_atm", description=INSTALLER_DESCRIPTION
    )
    commands = parser.add_subparsers(dest="command", required=True)
    install = commands.add_parser(
        "install", help="install the standard per-profile gateway hook"
    )
    install.add_argument("--profile", required=True, help="Hermes profile name")
    install.add_argument(
        "--profile-home",
        required=True,
        type=Path,
        help="Hermes profile directory; also publish it as the roster home_dir",
    )
    install.add_argument("--identity", required=True, help="ATM member identity")
    install.add_argument("--team", required=True, help="ATM team containing the member")
    install.add_argument(
        "--chat-id", required=True, help="Hermes session chat identifier (kept in local config)"
    )
    install.add_argument(
        "--atm-home", default=os.environ.get("ATM_HOME", ""), help="ATM home directory"
    )
    install.add_argument(
        "--workspace-root",
        default=os.environ.get("ATM_WORKSPACE_ROOT", ""),
        help="canonical graft root; must equal the roster workspace_root",
    )
    install.add_argument("--launch-agent-plist", type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        result = install_profile(
            profile_home=args.profile_home,
            profile=args.profile,
            identity=args.identity,
            team=args.team,
            chat_id=args.chat_id,
            atm_home=args.atm_home,
            workspace_root=args.workspace_root,
            launch_agent_plist=args.launch_agent_plist,
        )
    except HermesAtmInstallError as error:
        print(f"hermes-atm install failed: {error}", file=sys.stderr)
        return 2
    # `config` contains the profile chat id. Installation confirmation must not
    # disclose it to shell history, CI logs, or copied troubleshooting output.
    print(
        json.dumps(
            {key: result[key] for key in ("hook_dir", "plugin_dir", "changed")},
            sort_keys=True,
        )
    )
    return 0
