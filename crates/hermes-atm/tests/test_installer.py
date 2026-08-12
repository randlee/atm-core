from __future__ import annotations

import asyncio
from contextlib import redirect_stdout
from io import StringIO
import json
from pathlib import Path
import plistlib
import runpy
import sys
import sysconfig
import tempfile
import types
import unittest

# The installer and gateway-hook tests exercise package-owned Python logic,
# not the compiled transport wheel. Augment an existing test stub rather than
# assuming import order: ``test_native_tools`` legitimately imports the same
# module first with only the names it needs.
graft = sys.modules.setdefault("atm_graft", types.ModuleType("atm_graft"))


class _PyAgentAddress:
    def __init__(self, *args):
        self.args = args


class _PyGraftSessionOptions:
    def __init__(self, *args):
        self.args = args


class _PyGraftSession:
    def __init__(self, *_args):
        raise AssertionError("test must replace PyGraftSession before runtime startup")


graft.PyAgentAddress = _PyAgentAddress
graft.PyGraftSessionOptions = _PyGraftSessionOptions
graft.PyGraftSession = _PyGraftSession

from hermes_atm import HermesAtmInstallError, install_profile
from hermes_atm.installer import main


class FakeSession:
    def __init__(self, _caller):
        self.callback = None
        self.closed = False

    def activate_receiver(self, _options, callback):
        self.callback = callback

    def close(self):
        self.closed = True


class GatewayModules:
    def __enter__(self):
        self.original_gateway = sys.modules.get("gateway")
        self.original_config = sys.modules.get("gateway.config")
        self.original_run = sys.modules.get("gateway.run")
        self.original_hermes_cli = sys.modules.get("hermes_cli")
        self.original_plugins = sys.modules.get("hermes_cli.plugins")

        class Platform:
            TELEGRAM = object()

        class GatewayRunner:
            gateway_loop = None

            async def inject_internal_message(self, **_kwargs):
                return None

        gateway = types.ModuleType("gateway")
        config = types.ModuleType("gateway.config")
        run = types.ModuleType("gateway.run")
        config.Platform = Platform
        run.GatewayRunner = GatewayRunner
        hermes_cli = types.ModuleType("hermes_cli")
        plugins = types.ModuleType("hermes_cli.plugins")

        class PluginContext:
            def register_tool(self, **_kwargs):
                return None

        plugins.PluginContext = PluginContext
        hermes_cli.plugins = plugins
        gateway.config = config
        sys.modules["gateway"] = gateway
        sys.modules["gateway.config"] = config
        sys.modules["gateway.run"] = run
        sys.modules["hermes_cli"] = hermes_cli
        sys.modules["hermes_cli.plugins"] = plugins
        self.runner = GatewayRunner()
        return self

    def __exit__(self, *_args):
        for name, value in (
            ("gateway", self.original_gateway),
            ("gateway.config", self.original_config),
            ("gateway.run", self.original_run),
            ("hermes_cli", self.original_hermes_cli),
            ("hermes_cli.plugins", self.original_plugins),
        ):
            if value is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = value


class InstallerTests(unittest.TestCase):
    def test_install_writes_standard_hook_and_is_idempotent(self):
        with tempfile.TemporaryDirectory() as temporary, GatewayModules():
            profile_home = Path(temporary) / "skillrx"
            result = install_profile(
                profile_home=profile_home,
                profile="skillrx",
                identity="skillrx",
                team="hermes",
                chat_id="100000001",
                atm_home="/tmp/atm",
                workspace_root="/tmp/workspace",
            )
            hook = profile_home / "hooks" / "hermes-atm"
            self.assertTrue(result["changed"])
            self.assertEqual(
                json.loads((hook / "config.json").read_text(encoding="utf-8")),
                {
                    "schema_version": 1,
                    "profile": "skillrx",
                    "atm_home": "/tmp/atm",
                    "identity": "skillrx",
                    "team": "hermes",
                    "chat_id": "100000001",
                    "workspace_root": "/tmp/workspace",
                },
            )
            self.assertIn("gateway:startup", (hook / "HOOK.yaml").read_text())
            self.assertIn("gateway:shutdown", (hook / "HOOK.yaml").read_text())
            handler = (hook / "handler.py").read_text()
            self.assertIn("sysconfig", handler)
            self.assertIn("hermes_atm.hook", handler)
            plugin = profile_home / "plugins" / "hermes-atm-native-tools"
            self.assertEqual(result["plugin_dir"], str(plugin))
            self.assertIn("atm_send", (plugin / "plugin.yaml").read_text())
            self.assertIn("register_tools", (plugin / "__init__.py").read_text())
            self.assertEqual(
                json.loads((plugin / "config.json").read_text(encoding="utf-8")),
                json.loads((hook / "config.json").read_text(encoding="utf-8")),
            )
            self.assertFalse(
                install_profile(
                    profile_home=profile_home,
                    profile="skillrx",
                    identity="skillrx",
                    team="hermes",
                    chat_id="100000001",
                    atm_home="/tmp/atm",
                    workspace_root="/tmp/workspace",
                )["changed"]
            )

    def test_install_rejects_a_launch_agent_for_another_interpreter(self):
        with tempfile.TemporaryDirectory() as temporary, GatewayModules():
            root = Path(temporary)
            plist = root / "gateway.plist"
            with plist.open("wb") as destination:
                plistlib.dump({"ProgramArguments": ["/not/the/active/python"]}, destination)
            with self.assertRaisesRegex(HermesAtmInstallError, "launch agent uses"):
                install_profile(
                    profile_home=root / "profile",
                    profile="skillrx",
                    identity="skillrx",
                    team="hermes",
                    chat_id="100000001",
                    atm_home="/tmp/atm",
                    workspace_root="/tmp/workspace",
                    launch_agent_plist=plist,
                )
            self.assertFalse((root / "profile" / "hooks").exists())

    def test_generated_hook_imports_from_the_gateway_interpreter_site_packages(self):
        """Reproduce Hermes' dynamic loader without package source-path help."""

        with tempfile.TemporaryDirectory() as temporary, GatewayModules():
            profile_home = Path(temporary) / "profile"
            install_profile(
                profile_home=profile_home,
                profile="skillrx",
                identity="skillrx",
                team="hermes",
                chat_id="100000001",
                atm_home="/tmp/atm",
                workspace_root="/tmp/workspace",
            )
            handler = profile_home / "hooks" / "hermes-atm" / "handler.py"
            previous_path = sys.path[:]
            previous_modules = {
                name: module
                for name, module in tuple(sys.modules.items())
                if name == "hermes_atm" or name.startswith("hermes_atm.")
            }
            try:
                for name in previous_modules:
                    sys.modules.pop(name, None)
                sys.path[:] = [str(handler.parent), sysconfig.get_paths()["stdlib"]]
                loaded = runpy.run_path(str(handler))
                self.assertTrue(asyncio.iscoroutinefunction(loaded["handle"]))
            finally:
                sys.path[:] = previous_path
                for name in tuple(sys.modules):
                    if name == "hermes_atm" or name.startswith("hermes_atm."):
                        sys.modules.pop(name, None)
                sys.modules.update(previous_modules)

    def test_cli_install_confirmation_redacts_the_local_chat_id(self):
        with tempfile.TemporaryDirectory() as temporary, GatewayModules():
            output = StringIO()
            with redirect_stdout(output):
                result = main(
                    [
                        "install",
                        "--profile",
                        "skillrx",
                        "--profile-home",
                        str(Path(temporary) / "profile"),
                        "--identity",
                        "skillrx",
                        "--team",
                        "hermes",
                        "--chat-id",
                        "local-secret-chat-id",
                        "--atm-home",
                        "/tmp/atm",
                        "--workspace-root",
                        "/tmp/workspace",
                    ]
                )

            self.assertEqual(result, 0)
            self.assertNotIn("local-secret-chat-id", output.getvalue())
            self.assertEqual(
                json.loads(output.getvalue()),
                {
                    "changed": True,
                    "hook_dir": str(Path(temporary) / "profile" / "hooks" / "hermes-atm"),
                    "plugin_dir": str(
                        Path(temporary) / "profile" / "plugins" / "hermes-atm-native-tools"
                    ),
                },
            )

    def test_installed_hook_activates_only_from_gateway_startup(self):
        async def scenario():
            import hermes_atm.hook as hook_module
            import hermes_atm.runtime as runtime_module

            original_session = runtime_module.atm_graft.PyGraftSession
            runtime_module.atm_graft.PyGraftSession = FakeSession
            hook_module._runtime = None
            try:
                with tempfile.TemporaryDirectory() as temporary, GatewayModules() as gateway:
                    root = Path(temporary)
                    install_profile(
                        profile_home=root,
                        profile="skillrx",
                        identity="skillrx",
                        team="hermes",
                        chat_id="100000001",
                        atm_home="/tmp/atm",
                        workspace_root="/tmp/workspace",
                    )
                    config_path = root / "hooks" / "hermes-atm" / "config.json"
                    await hook_module.handle("agent:start", gateway.runner, config_path)
                    self.assertIsNone(hook_module._runtime)
                    await hook_module.handle("gateway:startup", gateway.runner, config_path)
                    self.assertIsNotNone(hook_module._runtime)
                    runtime = hook_module._runtime
                    await hook_module.handle("gateway:shutdown", {}, config_path)
                    self.assertTrue(runtime.session.closed)
                    self.assertIsNone(hook_module._runtime)
                    hook_module._cleanup(runtime)
                    self.assertIsNone(hook_module._runtime)
            finally:
                hook_module._runtime = None
                runtime_module.atm_graft.PyGraftSession = original_session

        asyncio.run(scenario())


if __name__ == "__main__":
    unittest.main()
