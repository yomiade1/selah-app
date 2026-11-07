[CmdletBinding()]
param([switch]$CI, [switch]$Fast, [switch]$Release, [string[]]$Only)
# The quality gate for Windows (TESTING-AND-QUALITY §1). Exit 0 means green.
#   ./tools/verify.ps1 -CI | -Fast | -Release | -Only 6,14
# The stages live in tools/verify.py, shared with tools/verify.sh.
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
. "$PSScriptRoot/tool-env.ps1"
$arguments = @('tools/verify.py')
if ($CI) { $arguments += '--ci' }
if ($Fast) { $arguments += '--fast' }
if ($Release) { $arguments += '--release' }
if ($Only) { $arguments += @('--only') + $Only }
$python = (Get-Command python -ErrorAction SilentlyContinue)
if (-not $python) { $python = Get-Command py -ErrorAction Stop }
& $python.Source @arguments
exit $LASTEXITCODE
