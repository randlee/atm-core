[CmdletBinding()]
param()

$picker = Join-Path $PSScriptRoot "picker.py"
$inputJson = [Console]::In.ReadToEnd()
$value = $inputJson | ConvertFrom-Json
if ($value.schema_version -ne 1) { throw "send-to picker: PickerInput schema_version is not supported" }

$rows = @(
    foreach ($team in $value.teams) {
        foreach ($member in $team.members) {
            [pscustomobject]@{
                Id = [string]$member.id
                Status = [string]$member.status
                Label = "$($team.name) / $($member.name) [$($member.status)]"
            }
        }
    }
)
# PRD R4: dead/idle members must be genuinely non-selectable, not merely
# labeled. Out-GridView has no notion of a disabled-but-visible row, so the
# only way to make one non-selectable is to exclude it from the grid
# entirely; the excluded members are still surfaced via a separate stderr
# notice rather than a selectable row.
$choices = @($rows | Where-Object { $_.Status -eq "active" })
$unavailable = @($rows | Where-Object { $_.Status -ne "active" })
if ($unavailable.Count -gt 0) {
    $names = ($unavailable | ForEach-Object { $_.Label }) -join ", "
    [Console]::Error.WriteLine("send-to picker: $($unavailable.Count) member(s) unavailable (dead/idle), excluded from selection: $names")
}

$selection = $env:ATM_SEND_TO_SELECTION
if ($null -ne $selection) {
    $wanted = @($selection -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    $chosen = @($choices | Where-Object { $wanted -contains $_.Id })
} elseif (Get-Command Out-GridView -ErrorAction SilentlyContinue) {
    $chosen = @( $choices | Out-GridView -Title "ATM Send-To (Ctrl-click for multiple)" -PassThru )
} else {
    [Console]::Error.WriteLine("send-to picker: Out-GridView is unavailable")
    exit 1
}
if ($chosen.Count -eq 0) { [Console]::Error.WriteLine("send-to picker: selection cancelled or empty"); exit 1 }

$output = [ordered]@{ schema_version = 1; recipients = @($chosen | ForEach-Object { $_.Id }) }
if ($env:ATM_SEND_TO_NOTE) { $output.note = $env:ATM_SEND_TO_NOTE }
$output | ConvertTo-Json -Compress
