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
                Label = "$($team.name) / $($member.name) [$($member.status)]"
            }
        }
    }
)
$selection = $env:ATM_SEND_TO_SELECTION
if ($null -ne $selection) {
    $wanted = @($selection -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    $chosen = @($rows | Where-Object { $wanted -contains $_.Id })
} elseif (Get-Command Out-GridView -ErrorAction SilentlyContinue) {
    $chosen = @( $rows | Out-GridView -Title "ATM Send-To (Ctrl-click for multiple)" -PassThru )
} else {
    [Console]::Error.WriteLine("send-to picker: Out-GridView is unavailable")
    exit 1
}
if ($chosen.Count -eq 0) { [Console]::Error.WriteLine("send-to picker: selection cancelled or empty"); exit 1 }

$output = [ordered]@{ schema_version = 1; recipients = @($chosen | ForEach-Object { $_.Id }) }
if ($env:ATM_SEND_TO_NOTE) { $output.note = $env:ATM_SEND_TO_NOTE }
$output | ConvertTo-Json -Compress
