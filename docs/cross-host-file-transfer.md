# Cross-Host File Transfer Setup

Send-To (the file-manager-to-agent-mailbox feature) lands files on the
*receiving* host's `$ATM_TEMP` scratch root. When the sender and the
recipient are on the same host, that's a local copy and there's nothing to
configure. When they're on different hosts, ATM needs a way to move bytes
across that boundary -- and per
[ADR-055](./adr/ADR-055-atm-temp-and-transfer-seam.md), that is **your**
script, not daemon machinery. There is no fetch/push endpoint, no transfer
state machine, and no envelope change: the daemon only ever carries ordinary
messages.

If a destination host has no script configured, the send fails closed with
exactly this error:

```text
File transfer to <host> not enabled. Read docs/cross-host-file-transfer.md
to set up cross-host file transfer.
```

You're reading that doc. Here's how to make the error go away.

## The contract in one paragraph

Create an executable file at `~/.atm/transfer/<host>` (Windows:
`~/.atm/transfer/<host>.ps1`), where `<host>` is exactly the `HostName` you'd
address that recipient with. ATM invokes it directly -- never through a
shell -- as `<script> <host> <transfer-id> <file>...` (Windows: `pwsh -File
<script> <host> <transfer-id> <file>...`), with a restricted environment
(`ATM_TEMP`, `ATM_IDENTITY`, and `ATM_TEAM`, plus an opt-in
`ATM_TRANSFER_SSH_CONFIG` left unset by every ordinary install -- see the
table below), your current working directory, and closed stdin. On success,
the script prints **exactly one
line** to stdout: the absolute path of the directory the files now live in
on `<host>`. On failure, it prints a short message to stderr and exits
non-zero.

## Setup: copy, chmod, adapt, verify

1. **Copy** one of the examples in [`scripts/transfer/`](../scripts/transfer/)
   to `~/.atm/transfer/<host>`, naming it for the exact destination host:

   ```sh
   cp scripts/transfer/sftp.sh ~/.atm/transfer/rand-m5
   ```

2. **`chmod`** it so only you can read, write, and execute it:

   ```sh
   chmod 700 ~/.atm/transfer/rand-m5
   ```

   This is not optional politeness -- ATM refuses to run a script that is
   not owner-executable, not owned by your uid, or has **any** group or
   other permission bit set (`mode & 0o077 != 0` -- readable, writable, *or*
   executable by group or other, not just writable) (`TransferScriptUnsafe`,
   distinct from the "not configured" error above). On a shared machine,
   `0700` and correct ownership are what keep another local user from
   planting or reading a script that runs with your identity.

3. **Adapt** the script to your fleet. The two things every example needs
   you to choose:
   - **Remote `$ATM_TEMP` resolution.** Either hardcode the value (fast,
     but only correct if every host resolves the same default, which
     requires the same uid on both ends), or ask the remote host directly
     with `ssh <host> 'echo "$ATM_TEMP"'` (always correct, one extra round
     trip). Both forms are shown, commented, in every example.
   - **The transport itself.** The examples use `scp`/`ssh` over the
     fleet's existing passwordless SSH. If your fleet uses something else
     (rsync, a different remote-copy tool, a company-specific bastion),
     adapt the two or three lines that do the actual copy -- the argv
     contract, environment restriction, and stdout convention above don't
     change.

4. **Verify** with a small file before relying on it:

   ```sh
   atm send someone@team --host rand-m5 --attach ./smoke-test.txt "testing transfer"
   ```

   A working script lands the file under `$ATM_TEMP/send-to/<transfer-id>/`
   on the destination and the message names that path. A failing script
   surfaces its stderr (bounded) and the send exits non-zero with **zero
   messages sent** -- transfer failures never leave a half-delivered state.

## Examples

| File | When to use it |
|---|---|
| [`scripts/transfer/sftp.sh`](../scripts/transfer/sftp.sh) | Default: plain SSH/`scp` over the fleet's existing passwordless SSH. Start here. |
| [`scripts/transfer/tailscale.sh`](../scripts/transfer/tailscale.sh) | The destination is only reachable by its Tailscale MagicDNS name. |
| [`scripts/transfer/sftp.ps1`](../scripts/transfer/sftp.ps1) | Windows destination or Windows-launched transfer, using the OpenSSH client bundled with modern Windows. |

Every example is short, commented, and meant to be edited -- they are
starting points, not a supported product surface. If your fleet's transport
doesn't fit any of them, write your own script honoring the same contract:
argv-array invocation (never a shell string), the environment allow-list,
closed stdin, and the one-line-absolute-path-on-success convention.

## What ATM does and does not do here

- **Does:** resolve `~/.atm/transfer/<host>`, check it is safe to run
  (owner-executable, owned by you, and no group/other permission bit set at
  all -- not just not-writable), invoke it
  with a bounded deadline (default 60 seconds; a wedged script is killed),
  capture bounded stdout/stderr, and validate the success line as untrusted
  input (one line, absolute, no control characters).
- **Does not:** manage SSH keys, configure Tailscale, provision accounts, or
  retry a failed transfer. Managed SSH/Tailscale enrollment is an
  environment/IT concern, not something this feature automates.
- **Does not roll back** a transfer that already landed remotely if a later
  step in a multi-recipient send fails. Orphaned staged files age out under
  the ordinary 30-day `$ATM_TEMP` sweep, the same as any other unread
  attachment.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| `File transfer to <host> not enabled...` | No file exists at `~/.atm/transfer/<host>` (or `<host>.ps1` on Windows). Confirm the exact spelling of `<host>` matches the roster's `HostName`. |
| Transfer refused as unsafe | The script fails the safety check. Run `chmod 700 ~/.atm/transfer/<host>` and confirm you own the file (`ls -l` should show your username, not root or another account). |
| Script runs manually but ATM's invocation fails | The child process only inherits `ATM_TEMP`, `ATM_IDENTITY`, `ATM_TEAM`, and (only if you export it yourself) `ATM_TRANSFER_SSH_CONFIG` -- if your script depends on another environment variable (an SSH agent socket, a custom `PATH` entry), it needs to be self-contained instead (an absolute path to the SSH binary, a dedicated key file, and so on). |
| Send hangs, then fails | The script exceeded its bounded deadline (default 60 seconds) and was killed. A remote host prompting for a password interactively is the usual cause -- confirm passwordless SSH actually works from an ordinary shell first. |
