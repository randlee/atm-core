[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0, ValueFromRemainingArguments = $true)]
    [string[]] $Files
)

$ErrorActionPreference = "Stop"
$atm = if ($env:ATM_BIN) { $env:ATM_BIN } else { "atm" }
$pickerDir = $PSScriptRoot

if ($Files.Count -eq 0) { throw "atm-send-to: provide at least one file" }
$inputJson = (& $atm teams --json --members | Out-String).Trim()
$pickerOutput = $null
if ($env:ATM_SEND_TO_PICKER) {
    $pickerOutput = $inputJson | & $env:ATM_SEND_TO_PICKER | Out-String
} else {
    # AQ6 updates this exact-version constant during release preflight.
    $wyvernPin = "0.5.0"
    $asset = if ($env:ATM_SEND_TO_WYVERN_ASSET) { $env:ATM_SEND_TO_WYVERN_ASSET } else { Join-Path $pickerDir "pick-member.html" }
    $probe = & python (Join-Path $pickerDir "probe_wyvern.py") --pin $wyvernPin --asset $asset 2>$null
    if ($LASTEXITCODE -eq 0) {
        try {
            $pickerOutput = $inputJson | & (if ($env:ATM_SEND_TO_WYVERN_BIN) { $env:ATM_SEND_TO_WYVERN_BIN } else { "wyvern" }) --picker $asset | Out-String
            $null = $pickerOutput | & python (Join-Path $pickerDir "picker.py") --validate
            if ($LASTEXITCODE -ne 0) { $pickerOutput = $null; [Console]::Error.WriteLine("send-to: Wyvern returned an incompatible PickerOutput; using native picker") }
        } catch {
            $pickerOutput = $null
            [Console]::Error.WriteLine("send-to: Wyvern picker failed; using native picker")
        }
    } else {
        [Console]::Error.WriteLine("send-to: Wyvern unavailable or incompatible; using native picker")
    }
    if (-not $pickerOutput) {
        $pickerOutput = $inputJson | & (Join-Path $pickerDir "picker-windows.ps1") | Out-String
    }
}

$null = $pickerOutput | & python (Join-Path $pickerDir "picker.py") --validate
if ($LASTEXITCODE -ne 0) { throw "atm-send-to: picker output failed the PickerOutput contract" }

$sendArgs = @("send", "--from-json")
foreach ($file in $Files) { $sendArgs += @("--attach", $file) }
$pickerOutput | & $atm @sendArgs
exit $LASTEXITCODE
