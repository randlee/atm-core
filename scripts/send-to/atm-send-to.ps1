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
    $pickerOutput = $inputJson | & python (Join-Path $pickerDir "run_wyvern_picker.py") --pin $wyvernPin --asset $asset | Out-String
    if ($LASTEXITCODE -eq 0) {
        try {
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
        # Test-only seam mirroring the .sh script's ATM_SEND_TO_NATIVE_PICKER:
        # lets degradation-harness coverage avoid depending on a real
        # Out-GridView session. Production launches never set this.
        $nativePicker = if ($env:ATM_SEND_TO_NATIVE_PICKER) { $env:ATM_SEND_TO_NATIVE_PICKER } else { Join-Path $pickerDir "picker-windows.ps1" }
        $pickerOutput = $inputJson | & $nativePicker | Out-String
    }
}

$null = $pickerOutput | & python (Join-Path $pickerDir "picker.py") --validate
if ($LASTEXITCODE -ne 0) { throw "atm-send-to: picker output failed the PickerOutput contract" }

$sendArgs = @("send", "--from-json")
foreach ($file in $Files) { $sendArgs += @("--attach", $file) }
$pickerOutput | & $atm @sendArgs
exit $LASTEXITCODE
