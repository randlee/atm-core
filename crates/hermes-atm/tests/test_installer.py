from __future__ import annotations

import asyncio
import json
from pathlib import Path
import plistlib
import sys
import tempfile
import types
import unittest

from hermes_atm import HermesAtmInstallError, install_profile


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
        gateway.config = config
        sys.modules["gateway"] = gateway
        sys.modules["gateway.config"] = config
        sys.modules["gateway.run"] = run
        self.runner = GatewayRunner()
        return self

    def __exit__(self, *_args):
        for name, value in (
            ("gateway", self.original_gateway),
            ("gateway.config", self.original_config),
            ("gateway.run", self.original_run),
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
                chat_id="8991600178",
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
                    "chat_id": "8991600178",
                    "workspace_root": "/tmp/workspace",
                },
            )
            self.assertIn("gateway:startup", (hook / "HOOK.yaml").read_text())
            self.assertIn("hermes_atm.hook", (hook / "handler.py").read_text())
            self.assertFalse(
                install_profile(
                    profile_home=profile_home,
                    profile="skillrx",
                    identity="skillrx",
                    team="hermes",
                    chat_id="8991600178",
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
                    chat_id="8991600178",
                    atm_home="/tmp/atm",
                    workspace_root="/tmp/workspace",
                    launch_agent_plist=plist,
                )
            self.assertFalse((root / "profile" / "hooks").exists())

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
                        chat_id="8991600178",
                        atm_home="/tmp/atm",
                        workspace_root="/tmp/workspace",
                    )
                    config_path = root / "hooks" / "hermes-atm" / "config.json"
                    await hook_module.handle("agent:start", {"gateway_runner": gateway.runner}, config_path)
                    self.assertIsNone(hook_module._runtime)
                    await hook_module.handle("gateway:startup", {"gateway_runner": gateway.runner}, config_path)
                    self.assertIsNotNone(hook_module._runtime)
                    runtime = hook_module._runtime
                    hook_module._cleanup(runtime)
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
