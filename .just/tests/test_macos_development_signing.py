from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from unittest import mock
import unittest


SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "macos_development_signing.py"
SPEC = importlib.util.spec_from_file_location("macos_development_signing", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
signing = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = signing
SPEC.loader.exec_module(signing)


class AppleDevelopmentSigningTests(unittest.TestCase):
    def test_parses_only_complete_identity_rows(self) -> None:
        parsed = signing.parse_identities(
            '  1) 0123456789ABCDEF0123456789ABCDEF01234567 "Apple Development: test"\n'
            '  2) invalid identity\n'
        )
        self.assertEqual(
            parsed,
            (signing.SigningIdentity("0123456789ABCDEF0123456789ABCDEF01234567", "Apple Development: test"),),
        )

    def test_duplicate_security_rows_are_one_certificate_candidate(self) -> None:
        identity = signing.SigningIdentity("A" * 40, "Apple Development: test")
        self.assertEqual(signing.unique_identities((identity, identity)), (identity,))

    def test_default_resolution_uses_apple_prefix_and_team_identifier_not_hostname(self) -> None:
        selected = signing.SigningIdentity(
            "A" * 40,
            "Apple Development: apple@example.test (N285DY5G36)",
            signing.DEFAULT_TEAM_IDENTIFIER,
        )
        wrong_team = signing.SigningIdentity("B" * 40, "Apple Development: other@example.test")
        with (
            mock.patch.object(signing, "_valid_identities", return_value=(selected, wrong_team)),
            mock.patch.object(
                signing,
                "_certificate_team_identifier",
                side_effect=[signing.DEFAULT_TEAM_IDENTIFIER, "OTHERTEAM"],
            ),
            mock.patch.object(signing, "_load_configured_identity", return_value=None),
            mock.patch.dict(signing.os.environ, {}, clear=True),
        ):
            self.assertEqual(signing.resolve_apple_development_identity(), selected)

    def test_default_resolution_rejects_ambiguous_team_candidates(self) -> None:
        identities = (
            signing.SigningIdentity("A" * 40, "Apple Development: one"),
            signing.SigningIdentity("B" * 40, "Apple Development: two"),
        )
        with (
            mock.patch.object(signing, "_valid_identities", return_value=identities),
            mock.patch.object(signing, "_certificate_team_identifier", return_value=signing.DEFAULT_TEAM_IDENTIFIER),
            mock.patch.object(signing, "_load_configured_identity", return_value=None),
            mock.patch.dict(signing.os.environ, {}, clear=True),
        ):
            with self.assertRaisesRegex(signing.SigningIdentityError, "exactly one"):
                signing.resolve_apple_development_identity()

    def test_default_resolution_deduplicates_repeated_keychain_certificate_rows(self) -> None:
        selected = signing.SigningIdentity("A" * 40, "Apple Development: test")
        with (
            mock.patch.object(signing, "_valid_identities", return_value=(selected,) * 4),
            mock.patch.object(
                signing,
                "_certificate_team_identifier",
                return_value=signing.DEFAULT_TEAM_IDENTIFIER,
            ),
            mock.patch.object(signing, "_load_configured_identity", return_value=None),
            mock.patch.dict(signing.os.environ, {}, clear=True),
        ):
            self.assertEqual(
                signing.resolve_apple_development_identity(),
                signing.SigningIdentity(
                    selected.fingerprint,
                    selected.common_name,
                    signing.DEFAULT_TEAM_IDENTIFIER,
                ),
            )

    def test_override_deduplicates_repeated_keychain_certificate_rows(self) -> None:
        selected = signing.SigningIdentity("A" * 40, "Apple Development: test")
        with (
            mock.patch.object(signing, "_valid_identities", return_value=(selected,) * 4),
            mock.patch.object(
                signing,
                "_certificate_team_identifier",
                return_value=signing.DEFAULT_TEAM_IDENTIFIER,
            ),
            mock.patch.dict(signing.os.environ, {signing.SIGNING_IDENTITY_ENVIRONMENT_VARIABLE: selected.fingerprint}),
        ):
            self.assertEqual(
                signing.resolve_apple_development_identity(),
                signing.SigningIdentity(
                    selected.fingerprint,
                    selected.common_name,
                    signing.DEFAULT_TEAM_IDENTIFIER,
                ),
            )

    def test_override_selects_the_matching_valid_identity(self) -> None:
        selected = signing.SigningIdentity("A" * 40, "Apple Development: alternate", "OTHERTEAM")
        with (
            mock.patch.object(
                signing,
                "_valid_identities",
                return_value=(signing.SigningIdentity(selected.fingerprint, selected.common_name),),
            ),
            mock.patch.object(signing, "_certificate_team_identifier", return_value=selected.team_identifier),
            mock.patch.dict(signing.os.environ, {signing.SIGNING_IDENTITY_ENVIRONMENT_VARIABLE: selected.fingerprint}),
        ):
            self.assertEqual(signing.resolve_apple_development_identity(), selected)

    def test_invalid_identity_reports_wwdr_recovery(self) -> None:
        with (
            mock.patch.object(signing, "_valid_identities", return_value=()),
            mock.patch.object(signing, "_identity_exists_but_is_not_valid", return_value=True),
            mock.patch.object(signing, "_load_configured_identity", return_value=None),
            mock.patch.dict(signing.os.environ, {}, clear=True),
        ):
            with self.assertRaisesRegex(signing.SigningIdentityError, "WWDR G3"):
                signing.resolve_apple_development_identity()

    def test_verification_requires_strict_signature_stable_identifier_and_team(self) -> None:
        verified = subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr="")
        details = subprocess.CompletedProcess(
            ["codesign"],
            0,
            stdout="",
            stderr=(
                f"Identifier={signing.CLI_IDENTIFIER}\n"
                f"TeamIdentifier={signing.DEFAULT_TEAM_IDENTIFIER}\n"
            ),
        )
        with mock.patch.object(signing, "_run", side_effect=[verified, details]):
            self.assertTrue(signing.verify_apple_signature("/candidate/atm", signing.CLI_IDENTIFIER))


@unittest.skipUnless(sys.platform == "darwin", "self-signed signing is macOS-only")
class MacosSigningIdentityTests(unittest.TestCase):
    def assert_configured_identity_error(
        self,
        config_text: str,
        identities: tuple[signing.SigningIdentity, ...],
        message: str,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            config_path = Path(temporary_directory) / "atm" / "signing-identity.json"
            config_path.parent.mkdir(parents=True)
            config_path.write_text(config_text, encoding="utf-8")
            with mock.patch.dict(
                signing.os.environ,
                {"XDG_CONFIG_HOME": temporary_directory},
                clear=True,
            ):
                with self.assertRaisesRegex(signing.SigningIdentityError, message):
                    signing._configured_identity(identities)

    def test_apple_identity_still_uses_team_identifier_verification(self) -> None:
        verified = subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr="")
        details = subprocess.CompletedProcess(
            ["codesign"],
            0,
            stdout="",
            stderr=(
                f"Identifier={signing.CLI_IDENTIFIER}\n"
                f"TeamIdentifier={signing.DEFAULT_TEAM_IDENTIFIER}\n"
            ),
        )
        with mock.patch.object(signing, "_run", side_effect=[verified, details]) as run:
            self.assertTrue(signing.verify_apple_signature("/candidate/atm", signing.CLI_IDENTIFIER))
        self.assertEqual(run.call_args_list, [
            mock.call(["codesign", "--verify", "--strict", "/candidate/atm"]),
            mock.call(["codesign", "-dvv", "/candidate/atm"]),
        ])

    def test_self_signed_identity_persists_and_reloads_from_file(self) -> None:
        identity = signing.SigningIdentity("A" * 40, "atm-daemon-dev")
        with tempfile.TemporaryDirectory() as temporary_directory:
            with (
                mock.patch.object(signing, "_valid_identities", return_value=(identity,)),
                mock.patch.dict(
                    signing.os.environ,
                    {
                        "ATM_SIGNING_IDENTITY": "atm-daemon-dev",
                        "XDG_CONFIG_HOME": temporary_directory,
                    },
                    clear=True,
                ),
            ):
                self.assertEqual(signing.resolve_apple_development_identity(), identity)
                config_path = Path(temporary_directory) / "atm" / "signing-identity.json"
                self.assertEqual(
                    json.loads(config_path.read_text(encoding="utf-8")),
                    {"common_name": "atm-daemon-dev", "fingerprint": "A" * 40},
                )
                with mock.patch.dict(
                    signing.os.environ,
                    {"XDG_CONFIG_HOME": temporary_directory},
                    clear=True,
                ):
                    self.assertEqual(signing.resolve_apple_development_identity(), identity)

    def test_configured_identity_rejects_malformed_json(self) -> None:
        self.assert_configured_identity_error(
            "{not-json",
            (),
            "unable to read signing identity configuration",
        )

    def test_configured_identity_rejects_missing_fingerprint(self) -> None:
        self.assert_configured_identity_error(
            '{"common_name":"atm-daemon-dev"}',
            (),
            "40-character fingerprint",
        )

    def test_configured_identity_rejects_invalid_fingerprint(self) -> None:
        self.assert_configured_identity_error(
            '{"common_name":"atm-daemon-dev","fingerprint":"wrong"}',
            (),
            "40-character fingerprint",
        )

    def test_configured_identity_rejects_missing_common_name(self) -> None:
        self.assert_configured_identity_error(
            '{"fingerprint":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}',
            (),
            "common_name",
        )

    def test_configured_identity_rejects_invalid_common_name(self) -> None:
        self.assert_configured_identity_error(
            '{"common_name":"","fingerprint":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}',
            (),
            "common_name",
        )

    def test_configured_identity_rejects_identity_missing_from_keychain(self) -> None:
        self.assert_configured_identity_error(
            '{"common_name":"atm-daemon-dev","fingerprint":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}',
            (signing.SigningIdentity("B" * 40, "atm-daemon-dev"),),
            "not installed exactly once",
        )

    def test_configured_identity_rejects_duplicate_keychain_identity(self) -> None:
        identity = signing.SigningIdentity("A" * 40, "atm-daemon-dev")
        self.assert_configured_identity_error(
            '{"common_name":"atm-daemon-dev","fingerprint":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}',
            (identity, identity),
            "not installed exactly once",
        )

    def test_self_signed_identity_verifies_with_matching_leaf_pin(self) -> None:
        fingerprint = "B" * 40
        verified = subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr="")
        details = subprocess.CompletedProcess(
            ["codesign"],
            0,
            stdout="",
            stderr=f"Identifier={signing.DAEMON_IDENTIFIER}\nAuthority=atm-daemon-dev\n",
        )
        pinned = subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr="")
        with mock.patch.object(signing, "_run", side_effect=[verified, details, pinned]) as run:
            self.assertTrue(
                signing.verify_apple_signature(
                    "/candidate/atm-daemon",
                    signing.DAEMON_IDENTIFIER,
                    "",
                    expected_leaf_fingerprint=fingerprint,
                    expected_common_name="atm-daemon-dev",
                )
            )
        self.assertEqual(
            run.call_args_list[-1],
            mock.call([
                "codesign",
                "--verify",
                "--strict",
                f'-R=certificate leaf = H"{fingerprint}"',
                "/candidate/atm-daemon",
            ]),
        )

    def test_self_signed_identity_rejects_wrong_leaf_pin(self) -> None:
        verified = subprocess.CompletedProcess(["codesign"], 0, stdout="", stderr="")
        details = subprocess.CompletedProcess(
            ["codesign"],
            0,
            stdout="",
            stderr=f"Identifier={signing.DAEMON_IDENTIFIER}\nAuthority=atm-daemon-dev\n",
        )
        pinned = subprocess.CompletedProcess(["codesign"], 1, stdout="", stderr="wrong leaf")
        with mock.patch.object(signing, "_run", side_effect=[verified, details, pinned]):
            self.assertFalse(
                signing.verify_apple_signature(
                    "/candidate/atm-daemon",
                    signing.DAEMON_IDENTIFIER,
                    "",
                    expected_leaf_fingerprint="C" * 40,
                    expected_common_name="atm-daemon-dev",
                )
            )


if __name__ == "__main__":
    unittest.main()
