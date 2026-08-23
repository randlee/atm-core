from __future__ import annotations

import importlib.util
from pathlib import Path
import subprocess
import sys
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


if __name__ == "__main__":
    unittest.main()
