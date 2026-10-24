"""Guard the Windows CI trust-root ACL transition and native invocation."""

from pathlib import Path
import unittest


REPOSITORY = Path(__file__).resolve().parents[2]


class WindowsTrustRootProvisioningTests(unittest.TestCase):
    def test_native_workflow_runs_provisioning_and_repeat_verification(self):
        workflow = (REPOSITORY / ".github/workflows/ci.yml").read_text(
            encoding="utf-8"
        )
        start = workflow.index("      - name: Provision protected tool-sender trust root on Windows")
        end = workflow.index("      - name: Install Rust toolchain", start)
        step = workflow[start:end]

        self.assertEqual(
            step.count("& .github/scripts/provision_windows_trust_root.ps1"), 2
        )

    def test_child_is_finalized_before_parent_restriction(self):
        script = (REPOSITORY / ".github/scripts/provision_windows_trust_root.ps1").read_text(
            encoding="utf-8"
        )
        commands = [
            line.strip()
            for line in script.splitlines()
            if "Invoke-TrustRootIcacls" in line and "function " not in line
        ]

        self.assertEqual(
            commands,
            [
                'Invoke-TrustRootIcacls $root @("/grant", "*${runnerSid}:(OI)(CI)F") "failed to grant temporary setup access on $root"',
                'Invoke-TrustRootIcacls $trustRoot @("/grant", "*${runnerSid}:F") "failed to grant temporary setup access on $trustRoot"',
                'Invoke-TrustRootIcacls $trustRoot @("/inheritance:r") "failed to disable inherited ACLs on $trustRoot"',
                'Invoke-TrustRootIcacls $trustRoot @("/setowner", "*${administratorsSid}") "failed to set owner on $trustRoot"',
                'Invoke-TrustRootIcacls $trustRoot @(',
                'Invoke-TrustRootIcacls $root @("/inheritance:r") "failed to disable inherited ACLs on $root"',
                'Invoke-TrustRootIcacls $root @("/setowner", "*${administratorsSid}") "failed to set owner on $root"',
                'Invoke-TrustRootIcacls $root @(',
            ],
        )
        self.assertIn('"*${runnerSid}:R"', script)
        self.assertIn('"*${runnerSid}:(OI)(CI)RX"', script)
        self.assertIn("$acl.GetOwner([Security.Principal.SecurityIdentifier])", script)
        self.assertIn("$acl.AreAccessRulesProtected", script)
        self.assertIn("$rule.IsInherited", script)

    def test_acl_failures_capture_token_and_pre_failure_acl_state(self):
        script = (REPOSITORY / ".github/scripts/provision_windows_trust_root.ps1").read_text(
            encoding="utf-8"
        )

        self.assertIn("Write-TrustRootAclSnapshot $stage", script)
        self.assertIn("& whoami /all", script)
        self.assertIn("Write-TrustRootFailureDiagnostics", script)
        self.assertIn("icacls exit $LASTEXITCODE", script)


if __name__ == "__main__":
    unittest.main()
