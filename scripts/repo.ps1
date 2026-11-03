[CmdletBinding()]
param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Rest
)

$ErrorActionPreference = "Stop"
$project = Split-Path -Parent $PSScriptRoot
$code = 1
Push-Location $project
try {
    & cargo run --quiet --release --bin rv-repo -- @Rest
    $code = $LASTEXITCODE
} finally {
    Pop-Location
}
exit $code
