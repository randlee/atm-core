# ATM cross-host transfer script -- Windows example (OpenSSH client).
#
# Install as ~/.atm/transfer/<host>.ps1 (note the .ps1 extension -- required
# on Windows, and only on Windows). ATM invokes it as:
#   pwsh -File <script> <host> <transfer-id> <file>...
# with a restricted environment (ATM_TEMP, ATM_IDENTITY, ATM_TEAM only),
# your current working directory, and closed stdin. A bounded deadline
# applies (default 60s); a wedged script is killed.
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

# Remote $ATM_TEMP resolution -- pick ONE of the following and delete the
# other.
#
# (a) Fixed value, if every host in your fleet uses the same scratch root:
$RemoteAtmTemp = "/tmp/atm-$(ssh $HostName 'id -u')"
#
# (b) Ask the remote host what it actually resolved (uncomment instead):
# $RemoteAtmTemp = (ssh $HostName 'echo "$ATM_TEMP"').Trim()

$RemoteDir = "$RemoteAtmTemp/send-to/$TransferId"

# The destination directory is created by this script, not by the daemon.
ssh $HostName "umask 077 && mkdir -p '$RemoteDir'"
if ($LASTEXITCODE -ne 0) {
    Write-Error "failed to create $RemoteDir on $HostName"
    exit 1
}

foreach ($file in $Files) {
    scp -q $file "${HostName}:${RemoteDir}/"
    if ($LASTEXITCODE -ne 0) {
        Write-Error "failed to copy $file to $HostName`:$RemoteDir"
        exit 1
    }
}

# Exactly one line: the landed directory's absolute path.
Write-Output $RemoteDir
