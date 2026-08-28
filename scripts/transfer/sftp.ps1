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
& $Ssh @SshExtraArgs $HostName "umask 077 && mkdir -p '$RemoteDir'"
if ($LASTEXITCODE -ne 0) {
    [Console]::Error.WriteLine("failed to create $RemoteDir on $HostName")
    exit 1
}

foreach ($file in $Files) {
    & $Scp @SshExtraArgs -q $file "${HostName}:${RemoteDir}/"
    if ($LASTEXITCODE -ne 0) {
        [Console]::Error.WriteLine("failed to copy $file to $HostName`:$RemoteDir")
        exit 1
    }
}

# Exactly one line: the landed directory's absolute path.
Write-Output $RemoteDir
