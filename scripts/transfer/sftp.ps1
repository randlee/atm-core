# ATM cross-host transfer script -- Windows example (OpenSSH client).
#
# Install as ~/.atm/transfer/<host>.ps1 (note the .ps1 extension -- required
# on Windows, and only on Windows). ATM invokes it as:
#   pwsh -File <script> <host> <transfer-id> <file>...
# with a restricted environment (ATM_TEMP, ATM_IDENTITY, ATM_TEAM, plus an
# opt-in ATM_TRANSFER_SSH_CONFIG -- unset for every ordinary install; see
# below), your current working directory, and closed stdin. A bounded
# deadline applies (default 60s); a wedged script is killed.
#
# Contract: on success, print EXACTLY ONE LINE to stdout -- the absolute
# path of the directory the files now live in on <host> -- and exit 0. On
# any failure, print a short diagnostic to stderr (Write-Error) and exit
# non-zero. ATM treats stdout as untrusted input and rejects multi-line,
# relative, or control-character output as a transfer failure.
#
# Setup: this example uses the OpenSSH client bundled with modern Windows
# (Settings > Optional Features > OpenSSH Client) and assumes passwordless
# key-based SSH to <host> is already configured, matching the fleet's usual
# baseline. Set the file's permissions so it is not writable by anyone but
# you (equivalent intent to `chmod 700` on Unix); on an NTFS volume this
# means removing inherited write ACEs for other users/groups.

param(
    [Parameter(Mandatory = $true)][string]$HostName,
    [Parameter(Mandatory = $true)][string]$TransferId,
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)][string[]]$Files
)

$ErrorActionPreference = "Stop"

# pwsh 7.3+ defaults `$PSNativeCommandUseErrorActionPreference` to `$true`:
# a native command that exits non-zero is turned into a terminating
# `NativeCommandExitException` governed by `$ErrorActionPreference` above.
# That preference is meant for this script's OWN logic, not for ssh/scp's
# exit codes, which are inspected manually via `$LASTEXITCODE` immediately
# below each call -- left at its default, the exception fires between the
# native call and that `$LASTEXITCODE` read, so the assignment that was
# supposed to capture ssh/scp's output never completes and `$LASTEXITCODE`
# is never populated (run 33142976493 @ dcd3130f1: "(ssh exit ): (no
# output)", both blank). Disabling it here restores this script's own
# manual, non-throwing exit-code handling regardless of the hosting pwsh
# version; guarded by `Get-Variable` because the setting does not exist on
# Windows PowerShell 5.1.
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

# Resolve ssh/scp explicitly rather than relying on bare `ssh`/`scp` calls
# to be found on whatever `PATH` this process happens to start with: ATM
# invokes this script's `pwsh` host with a deliberately minimal, synthesized
# `PATH` (ADR-055 decision (c) amendment), never the caller's own, so a
# bare unqualified call is not guaranteed to resolve even when OpenSSH is
# installed in its usual location. `Get-Command` covers PATH (including the
# synthesized OpenSSH directory ATM already includes) and any developer
# override; the explicit `%SystemRoot%\System32\OpenSSH` fallback below
# covers the case where neither this process's PATH is set up and OpenSSH's
# canonical install location itself is what actually has the binary.
function Resolve-TransferBinary {
    param(
        [Parameter(Mandatory = $true)][string]$Name
    )
    $command = Get-Command -Name $Name -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($command) {
        return $command.Source
    }
    if ($env:SystemRoot) {
        $fallback = Join-Path $env:SystemRoot "System32\OpenSSH\$Name.exe"
        if (Test-Path -LiteralPath $fallback -PathType Leaf) {
            return $fallback
        }
    }
    [Console]::Error.WriteLine(
        "$Name not found on PATH and no fallback at `$env:SystemRoot\System32\OpenSSH\$Name.exe`; " +
        "install the OpenSSH client (Settings > Optional Features > OpenSSH Client) or add it to PATH."
    )
    exit 1
}

$Ssh = Resolve-TransferBinary -Name "ssh"
$Scp = Resolve-TransferBinary -Name "scp"

# Optional: route ssh/scp through a scratch config (`ssh -F <path>`)
# instead of the real `~/.ssh/config`. Ordinary installs never set
# ATM_TRANSFER_SSH_CONFIG, so $SshExtraArgs stays empty and ssh/scp below
# behave exactly as before; test/tooling harnesses (for example
# `scripts/phase-aq/run_aq4_transfer_evidence.py`) export it to point at a
# throwaway config for a loopback sshd without ever touching the real
# `~/.ssh/config`.
$SshExtraArgs = @()
if ($env:ATM_TRANSFER_SSH_CONFIG) {
    $SshExtraArgs = @("-F", $env:ATM_TRANSFER_SSH_CONFIG)
}

# Remote $ATM_TEMP resolution -- pick ONE of the following and delete the
# other.
#
# (a) Fixed value, if every host in your fleet uses the same scratch root.
#     Unlike sftp.sh's `id -u` (which runs *locally* on a Unix sender, and
#     only works because the fleet's local and remote uids match), a
#     Windows sender has no local uid to read -- so this is a literal
#     constant you fill in once, not a computed value. Determine it ahead
#     of time with `ssh <host> id -u` and hardcode the result below; this
#     keeps the script local-only (no extra network round trip) and
#     matches sftp.sh's single-ssh-call shape (mkdir, then scp). The
#     placeholder below intentionally avoids `<`/`>`: both are legal in a
#     POSIX filename on the real (Unix) receiver this variable names, but
#     they are reserved Win32/NTFS characters, and this exact string is
#     also what `.just/tests/test_transfer_scripts.py`'s `SftpPs1Tests`
#     asks its local fake-ssh/fake-scp harness to `mkdir` on the sender's
#     own filesystem to simulate the remote copy -- on a Windows sender
#     that local simulation is real NTFS, so a placeholder containing
#     `<`/`>` fails there even though the genuine remote `mkdir` (a Unix
#     shell command run over `ssh`) would have accepted it:
$RemoteAtmTemp = "/tmp/atm-REPLACE_WITH_DESTINATION_UID"
#
# (b) Ask the remote host what it actually resolved (uncomment instead --
#     this adds one extra `ssh` round trip beyond the mkdir call below):
# $RemoteAtmTemp = (& $Ssh $HostName 'echo "$ATM_TEMP"').Trim()

$RemoteDir = "$RemoteAtmTemp/send-to/$TransferId"

# The destination directory is created by this script, not by the daemon.
# Diagnostics below write directly to the stderr stream rather than via
# `Write-Error`: under `$ErrorActionPreference = "Stop"`, `Write-Error`
# raises a terminating exception whose default rendering (call stack,
# `+ CategoryInfo`, `+ FullyQualifiedErrorId`) is not the "short
# diagnostic" the contract at the top of this file promises.
#
# `2>&1` merges ssh/scp's own stderr into this call's captured output
# instead of letting it stream straight through: an unredirected native
# stderr write here can itself become a terminating error under
# `$ErrorActionPreference = "Stop"`, aborting the script before the
# `$LASTEXITCODE` check below ever runs -- and even when it doesn't, the
# failure messages below previously reported only a generic "failed to
# create"/"failed to copy" line with none of ssh/scp's own diagnostic
# text (run 33141941621 @ 21f00edb1: the real remote failure reason was
# never recorded anywhere reachable from the caller). Capturing it here
# keeps the ssh/scp argv itself unchanged; it only changes what happens
# to their output.
function Format-CapturedOutput {
    param([object[]]$Captured)
    if (-not $Captured) {
        return "(no output)"
    }
    $lines = $Captured | ForEach-Object { $_.ToString() }
    $joined = ($lines -join "; ").Trim()
    if ([string]::IsNullOrEmpty($joined)) {
        return "(no output)"
    }
    return $joined
}

# Runs `$Binary @Arguments`, capturing its merged stdout+stderr and exit
# code (`$LASTEXITCODE` is read on the statement immediately after the
# call assigns `Output` -- pwsh can reset/clobber it on any later
# statement, including a failed comparison, so nothing else may run
# in between). Wrapped in try/catch because a native-command invocation
# can also fail at the PowerShell level rather than exiting non-zero on
# its own terms -- e.g. a `NativeCommandExitException`
# (`$PSNativeCommandUseErrorActionPreference`, disabled above but kept
# defensive here for whatever pwsh version this script ends up running
# under) or the binary failing to start at all -- in which case
# `$LASTEXITCODE` was never (reliably) populated by this call and the
# exception's own message is the only diagnostic available; surfacing it
# instead of silently falling through to a blank "(ssh exit ): (no
# output)" is the whole point of this wrapper (run 33142976493 @
# dcd3130f1).
function Invoke-Transfer {
    param(
        [Parameter(Mandatory = $true)][string]$Binary,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )
    try {
        $output = & $Binary @Arguments 2>&1
        $exitCode = $LASTEXITCODE
        return [pscustomobject]@{ Output = $output; ExitCode = $exitCode; Exception = $null }
    } catch {
        return [pscustomobject]@{ Output = $null; ExitCode = $LASTEXITCODE; Exception = $_.Exception.Message }
    }
}

function Format-TransferFailure {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Result
    )
    if ($Result.Exception) {
        $exitCodeText = if ($null -ne $Result.ExitCode) { "$($Result.ExitCode)" } else { "unknown" }
        return "$($Result.Exception) (exit $exitCodeText)"
    }
    return Format-CapturedOutput $Result.Output
}

# `-n` (ssh only; scp has no equivalent flag) redirects ssh's stdin from
# a null source instead of leaving it on whatever handle this script's own
# closed stdin (ATM's contract, see the top of this file) left the child
# process with. Windows OpenSSH's client polls stdin as part of its own
# I/O multiplexing from the very start of a connection -- before identity
# exchange even completes -- so an invalid/closed stdin handle there can
# abort the TCP connection outright (`kex_exchange_identification: read:
# Connection aborted`, `WSARecv() ERROR 10053`: run 33142976493 @
# dcd3130f1's live Windows evidence) rather than merely fail cleanly. The
# mkdir call needs no stdin at all, so `-n` is always safe here.
$mkdirResult = Invoke-Transfer -Binary $Ssh -Arguments ($SshExtraArgs + @("-n", $HostName, "umask 077 && mkdir -p '$RemoteDir'"))
if ($mkdirResult.ExitCode -ne 0 -or $mkdirResult.Exception) {
    $detail = Format-TransferFailure $mkdirResult
    $exitCodeText = if ($null -ne $mkdirResult.ExitCode) { "$($mkdirResult.ExitCode)" } else { "unknown" }
    [Console]::Error.WriteLine("failed to create $RemoteDir on $HostName (ssh exit ${exitCodeText}): $detail")
    exit 1
}

foreach ($file in $Files) {
    $copyResult = Invoke-Transfer -Binary $Scp -Arguments ($SshExtraArgs + @("-q", $file, "${HostName}:${RemoteDir}/"))
    if ($copyResult.ExitCode -ne 0 -or $copyResult.Exception) {
        $detail = Format-TransferFailure $copyResult
        $exitCodeText = if ($null -ne $copyResult.ExitCode) { "$($copyResult.ExitCode)" } else { "unknown" }
        [Console]::Error.WriteLine("failed to copy $file to $HostName`:$RemoteDir (scp exit ${exitCodeText}): $detail")
        exit 1
    }
}

# Exactly one line: the landed directory's absolute path.
Write-Output $RemoteDir
