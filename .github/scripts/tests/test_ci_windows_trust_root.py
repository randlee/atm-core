"""Guard the Windows CI trust-root ACL transition order."""

from pathlib import Path
import unittest


REPOSITORY = Path(__file__).resolve().parents[3]


class WindowsTrustRootProvisioningTests(unittest.TestCase):
    def test_write_owner_is_granted_before_inheritance_is_removed(self):
        workflow = (REPOSITORY / ".github/workflows/ci.yml").read_text(
            encoding="utf-8"
        )
        start = workflow.index("      - name: Provision protected tool-sender trust root on Windows")
        end = workflow.index("      - name: Install Rust toolchain", start)
        step = workflow[start:end]
        commands = [line.strip() for line in step.splitlines() if line.strip().startswith("& icacls ")]

        self.assertEqual(
            commands,
            [
                '& icacls $root /grant "*${runnerSid}:(WO)"',
                '& icacls $trustRoot /grant "*${runnerSid}:(WO)"',
                "& icacls $root /inheritance:r",
                "& icacls $trustRoot /inheritance:r",
                '& icacls $root /setowner "*${administratorsSid}"',
                '& icacls $trustRoot /setowner "*${administratorsSid}"',
                '& icacls $root /grant:r "*${systemSid}:(OI)(CI)F" "*${administratorsSid}:(OI)(CI)F" "*${runnerSid}:(OI)(CI)RX"',
                '& icacls $trustRoot /grant:r "*${systemSid}:F" "*${administratorsSid}:F" "*${runnerSid}:R"',
            ],
        )
        self.assertIn('if ($LASTEXITCODE -ne 0) { throw "failed to grant provisioning WRITE_OWNER', step)
        self.assertIn('throw "failed to set owner on $root"', step)
        self.assertIn('throw "failed to set owner on $trustRoot"', step)
        self.assertEqual(step.count("whoami /all"), 2)
        self.assertGreater(
            step.index("Assert-ProtectedTrustRootAcl $root $true"),
            step.index('& icacls $trustRoot /grant:r'),
        )
        self.assertIn("if (-not $acl.AreAccessRulesProtected)", step)
        self.assertIn("$acl.GetOwner([Security.Principal.SecurityIdentifier])", step)
        self.assertIn("$rule.FileSystemRights -ne [int]$expectedRights", step)
        self.assertIn("$rule.IsInherited", step)


if __name__ == "__main__":
    unittest.main()
