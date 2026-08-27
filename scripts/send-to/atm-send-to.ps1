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
        # Wyvern has no `--picker <path>` flag: PickerInput travels as the
        # generated wizard command's `config` field (a wizard page's only
        # caller-data channel), and the terminal `WizardResult`'s `.data`
        # (not bare stdout) is the PickerOutput. See
        # docs/plans/phase-aq/fixtures/wyvern-pick-member-contract.md.
        $wizardDir = $null
        try {
            $atmTempRoot = if ($env:ATM_TEMP) { $env:ATM_TEMP } else { Join-Path $env:TEMP "atm" }
            $wizardDir = Join-Path (Join-Path $atmTempRoot "send-to") ("wyvern-wizard." + [System.Guid]::NewGuid().ToString("N"))
            New-Item -ItemType Directory -Force -Path (Join-Path $wizardDir "pages") | Out-Null
            Copy-Item -Path $asset -Destination (Join-Path (Join-Path $wizardDir "pages") "pick-member.html") -Force
            $wizardJson = $inputJson | & python (Join-Path $pickerDir "picker.py") --make-wizard-json
            if ($LASTEXITCODE -ne 0) { throw "picker.py --make-wizard-json failed" }
            $wizardJsonPath = Join-Path $wizardDir "wizard.json"
            Set-Content -Path $wizardJsonPath -Value $wizardJson -NoNewline -Encoding utf8
            $wyvernBin = if ($env:ATM_SEND_TO_WYVERN_BIN) { $env:ATM_SEND_TO_WYVERN_BIN } else { "wyvern" }
            $wizardResult = & $wyvernBin $wizardJsonPath --ui-root $wizardDir | Out-String
            if ($LASTEXITCODE -ne 0) { throw "wyvern exited $LASTEXITCODE" }
            $pickerOutput = $wizardResult | & python (Join-Path $pickerDir "picker.py") --unwrap-wizard-result
            if ($LASTEXITCODE -ne 0) { $pickerOutput = $null; [Console]::Error.WriteLine("send-to: Wyvern returned an incompatible PickerOutput; using native picker") }
        } catch {
            $pickerOutput = $null
            [Console]::Error.WriteLine("send-to: Wyvern picker failed; using native picker")
        } finally {
            if ($wizardDir -and (Test-Path $wizardDir)) { Remove-Item -Recurse -Force $wizardDir -ErrorAction SilentlyContinue }
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
