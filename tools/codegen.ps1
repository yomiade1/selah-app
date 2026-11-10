[CmdletBinding()]
param()
# Regenerates every contract binding: buf lint, TypeScript (protobuf-es), Rust (prost) and C# (Grpc.Tools).
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
. "$PSScriptRoot/tool-env.ps1"
Invoke-Native 'corepack.cmd' @('pnpm', 'exec', 'buf', 'lint')
Invoke-Native 'corepack.cmd' @('pnpm', 'exec', 'buf', 'generate')
$py = (Get-Command python -ErrorAction SilentlyContinue)
if (-not $py) { $py = Get-Command py -ErrorAction Stop }
Invoke-Native $py.Source @('tools/gen_help_anchors.py')
Invoke-Native 'cargo' @('build', '-p', 'selah-proto', '--locked')
Invoke-Native 'dotnet' @('build', 'app/Selah.Client/Selah.Client.csproj', '-c', 'Release', '-p:RestoreLockedMode=true')
Write-Output 'Code generation finished.'
