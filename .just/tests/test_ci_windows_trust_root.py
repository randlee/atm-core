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
        self.assertIn("[Security.AccessControl.FileSystemRights]::Synchronize", script)
        self.assertIn("$readRights -bor [Security.AccessControl.FileSystemRights]::Synchronize", script)
        self.assertIn("$acl.GetOwner([Security.Principal.SecurityIdentifier])", script)
        self.assertIn("$acl.AreAccessRulesProtected", script)
        self.assertIn("$rule.IsInherited", script)

    def test_windows_read_only_acl_masks_include_synchronize_without_write_bits(self):
        # Observed exact masks from Windows CI: directory RX is 0x1200a9;
        # file R is 0x120089. Both are their read masks plus Synchronize.
        synchronize = 0x00100000
        read_execute = 0x000200A9
        read = 0x00020089
        directory_mask = read_execute | synchronize
        file_mask = read | synchronize
        self.assertEqual(directory_mask, 0x001200A9)
        self.assertEqual(file_mask, 0x00120089)

        write_authority = (
            0x00000002  # WriteData/CreateFiles
            | 0x00000004  # AppendData/CreateDirectories
            | 0x00000040  # DeleteSubdirectoriesAndFiles
            | 0x00000010  # WriteExtendedAttributes
            | 0x00000100  # WriteAttributes
            | 0x00010000  # Delete
            | 0x00040000  # ChangePermissions
            | 0x00080000  # TakeOwnership
        )
        self.assertEqual(directory_mask & write_authority, 0)
        self.assertEqual(file_mask & write_authority, 0)

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
