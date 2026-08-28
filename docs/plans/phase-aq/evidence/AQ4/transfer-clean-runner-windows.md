# AQ4 cross-host transfer live evidence

Host: `clean-runner-windows`
Commit: `21f00edb1efb43bff894cfffd0bb7f2a5d1847bd`
Status: **FAIL**

## Real `atm send --attach` over a loopback `sshd`

Command: `D:\a\atm-core\atm-core\target\release\atm.exe send aq4-receiver@aq4-transfer-evidence AQ4 live transfer evidence: see attached file --host localhost --attach C:\Users\RUNNER~1\AppData\Local\Temp\aq4-evidence-yz3e2chp\attach-source\aq4-report.pdf`
Exit code: `3`

Landed path (from the receiver's real mailbox): `None`
Matches `send_to_staging_dir` convention (`.../send-to/<transfer-id>`): **False**
Landed file exists: **False**
Landed content byte-for-byte matches the source attachment: **False**

Error: `None`
