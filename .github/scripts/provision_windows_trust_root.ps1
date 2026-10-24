param(
  [string]$RootPath = (Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::CommonApplicationData)) "ATM"),
  [string]$SourcePath = ".github/fixtures/tool-sender-trust-root-ci.json"
)

$ErrorActionPreference = "Stop"
$root = [IO.Path]::GetFullPath($RootPath)
$trustRoot = Join-Path $root "tool-sender-trust-root.json"
$runnerSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$systemSid = "S-1-5-18"
$administratorsSid = "S-1-5-32-544"
$stage = "initialization"

function Write-TrustRootAclSnapshot {
  param([string]$Label)

  Write-Host "Trust-root ACL snapshot before ${Label}:"
  foreach ($path in @($root, $trustRoot)) {
    if (Test-Path -LiteralPath $path) {
      try {
        $acl = Get-Acl -LiteralPath $path
        Write-Host "$path owner=$($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value) protected=$($acl.AreAccessRulesProtected) sddl=$($acl.Sddl)"
      }
      catch {
        Write-Host "$path ACL snapshot failed: $($_.Exception.Message)"
      }
    }
    else {
      Write-Host "$path does not exist"
    }
  }
}

function Write-TrustRootFailureDiagnostics {
  Write-Host "Windows token at failure during '$stage':"
  & whoami /all
  Write-TrustRootAclSnapshot "failure in $stage"
}

function Invoke-TrustRootIcacls {
  param(
    [string]$Path,
    [string[]]$Arguments,
    [string]$FailureMessage
  )

  Write-TrustRootAclSnapshot $stage
  & icacls $Path @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "$FailureMessage (icacls exit $LASTEXITCODE)"
  }
}

function Assert-ProtectedTrustRootAcl {
  param([string]$Path, [bool]$IsDirectory)

  $acl = Get-Acl -LiteralPath $Path
  $ownerSid = $acl.GetOwner([Security.Principal.SecurityIdentifier]).Value
  if ($ownerSid -ne $administratorsSid) { throw "unexpected owner on $Path`: $ownerSid" }
  if (-not $acl.AreAccessRulesProtected) { throw "inherited ACLs remain enabled on $Path" }

  $expectedInheritance = [Security.AccessControl.InheritanceFlags]::None
  if ($IsDirectory) {
    $expectedInheritance = [Security.AccessControl.InheritanceFlags]::ObjectInherit -bor `
      [Security.AccessControl.InheritanceFlags]::ContainerInherit
  }
  $rules = @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
  if ($rules.Count -ne 3) { throw "unexpected ACE count on $Path`: $($rules.Count)" }
  $seen = @{}
  foreach ($rule in $rules) {
    $sid = $rule.IdentityReference.Value
    if ($rule.IsInherited -or $rule.AccessControlType -ne [Security.AccessControl.AccessControlType]::Allow) {
      throw "unexpected inherited or deny ACE on $Path for $sid"
    }
    $expectedRights = switch -Exact ($sid) {
      $systemSid { [Security.AccessControl.FileSystemRights]::FullControl; break }
      $administratorsSid { [Security.AccessControl.FileSystemRights]::FullControl; break }
      $runnerSid {
        if ($IsDirectory) { [Security.AccessControl.FileSystemRights]::ReadAndExecute }
        else { [Security.AccessControl.FileSystemRights]::Read }
        break
      }
      default { throw "unexpected trustee on $Path`: $sid" }
    }
    if ([int]$rule.FileSystemRights -ne [int]$expectedRights) {
      throw "unexpected rights on $Path for $sid`: $($rule.FileSystemRights)"
    }
    if ($rule.InheritanceFlags -ne $expectedInheritance -or
        $rule.PropagationFlags -ne [Security.AccessControl.PropagationFlags]::None) {
      throw "unexpected ACE inheritance flags on $Path for $sid"
    }
    if ($seen.ContainsKey($sid)) { throw "duplicate ACE on $Path for $sid" }
    $seen[$sid] = $true
  }
  if (-not $seen.ContainsKey($systemSid) -or
      -not $seen.ContainsKey($administratorsSid) -or
      -not $seen.ContainsKey($runnerSid)) {
    throw "required SYSTEM, Administrators, or runner ACE missing on $Path"
  }
}

function Assert-ProtectedTrustRoot {
  Assert-ProtectedTrustRootAcl $root $true
  Assert-ProtectedTrustRootAcl $trustRoot $false
}

try {
  # A second invocation verifies the exact final state and returns without
  # reopening the trust root for writes.
  if ((Test-Path -LiteralPath $root -PathType Container) -and
      (Test-Path -LiteralPath $trustRoot -PathType Leaf)) {
    $stage = "idempotent final-state verification"
    Write-TrustRootAclSnapshot $stage
    Assert-ProtectedTrustRoot
    Write-Host "Trust root is already provisioned with the expected protected ACL."
    return
  }

  $stage = "create trust-root directory"
  New-Item -ItemType Directory -Force -Path $root | Out-Null
  # Keep full setup access explicit on both objects while inheritance and
  # ownership change. The child is finalized before the parent so its DACL
  # operations retain both file access and parent traversal throughout.
  $stage = "grant temporary setup access on trust-root directory"
  Invoke-TrustRootIcacls $root @("/grant", "*${runnerSid}:(OI)(CI)F") "failed to grant temporary setup access on $root"

  $stage = "copy trust-root fixture"
  Copy-Item -LiteralPath $SourcePath -Destination $trustRoot -Force
  $stage = "grant temporary setup access on trust-root file"
  Invoke-TrustRootIcacls $trustRoot @("/grant", "*${runnerSid}:F") "failed to grant temporary setup access on $trustRoot"

  $stage = "remove trust-root file inheritance"
  Invoke-TrustRootIcacls $trustRoot @("/inheritance:r") "failed to disable inherited ACLs on $trustRoot"
  $stage = "set trust-root file owner"
  Invoke-TrustRootIcacls $trustRoot @("/setowner", "*${administratorsSid}") "failed to set owner on $trustRoot"
  $stage = "finalize trust-root file ACL"
  Invoke-TrustRootIcacls $trustRoot @(
    "/grant:r",
    "*${systemSid}:F",
    "*${administratorsSid}:F",
    "*${runnerSid}:R"
  ) "failed to grant protected ACL on $trustRoot"

  $stage = "remove trust-root directory inheritance"
  Invoke-TrustRootIcacls $root @("/inheritance:r") "failed to disable inherited ACLs on $root"
  $stage = "set trust-root directory owner"
  Invoke-TrustRootIcacls $root @("/setowner", "*${administratorsSid}") "failed to set owner on $root"
  $stage = "finalize trust-root directory ACL"
  Invoke-TrustRootIcacls $root @(
    "/grant:r",
    "*${systemSid}:(OI)(CI)F",
    "*${administratorsSid}:(OI)(CI)F",
    "*${runnerSid}:(OI)(CI)RX"
  ) "failed to grant protected ACL on $root"

  $stage = "verify final owner and DACL"
  Write-TrustRootAclSnapshot $stage
  Assert-ProtectedTrustRoot
  Write-Host "Trust-root provisioning completed with the expected protected ACL."
}
catch {
  Write-TrustRootFailureDiagnostics
  throw "Windows trust-root provisioning failed during '$stage': $($_.Exception.Message)"
}
