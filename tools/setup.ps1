[CmdletBinding()]
param([switch]$CheckOnly, [switch]$Install)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)
. "$PSScriptRoot/tool-env.ps1"
if ($CheckOnly -and $Install) { throw 'Choose -CheckOnly or -Install.' }

function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed (exit $LASTEXITCODE)." }
}

if ($Install) {
    # H-01: run only after the person using this computer approves installation.
    $packages = @(
        'Microsoft.VisualStudio.2022.BuildTools',
        'LLVM.LLVM',
        'Microsoft.DotNet.SDK.10',
        'Rustlang.Rustup'
    )
    foreach ($package in $packages) {
        if ($package -eq 'Microsoft.VisualStudio.2022.BuildTools') {
            $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
            if (Test-Path $vswhere) {
                $installed = & $vswhere -latest -version '[17.0,18.0)' -products '*' `
                    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
                if ($installed) { continue }
            }
        }
        if ($package -eq 'LLVM.LLVM' -and (Get-Command clang -ErrorAction SilentlyContinue)) { continue }
        if ($package -eq 'Rustlang.Rustup' -and (Get-Command rustup -ErrorAction SilentlyContinue)) { continue }
        if ($package -eq 'Microsoft.DotNet.SDK.10' -and (Get-Command dotnet -ErrorAction SilentlyContinue)) {
            if ((& dotnet --list-sdks) -match '^10\.0\.\d+ ') { continue }
        }
        $arguments = @('install', '--id', $package, '--exact', '--source', 'winget',
            '--accept-source-agreements', '--accept-package-agreements', '--silent')
        if ($package -eq 'Microsoft.VisualStudio.2022.BuildTools') {
            $arguments += @('--override', '--wait --quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended')
        }
        if ($package -eq 'Microsoft.DotNet.SDK.10') {
            $arguments += @('--version', '10.0.100')
        }
        Invoke-Checked 'winget' $arguments
    }
    $env:PATH = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
        [Environment]::GetEnvironmentVariable('Path', 'User')
    . "$PSScriptRoot/tool-env.ps1"
    Invoke-Checked 'rustup' @('toolchain', 'install', '1.91.0', '--profile', 'minimal',
        '--component', 'rustfmt', '--component', 'clippy')
    # A project-local Node avoids replacing another project's system Node version.
    $nodeDirectory = Join-Path $PWD '.tools/node-v22.21.1-win-x64'
    if (-not (Test-Path (Join-Path $nodeDirectory 'node.exe'))) {
        New-Item -ItemType Directory -Force .tools | Out-Null
        $archive = Join-Path $PWD '.tools/node-v22.21.1-win-x64.zip'
        Invoke-WebRequest -Uri 'https://nodejs.org/dist/v22.21.1/node-v22.21.1-win-x64.zip' `
            -OutFile $archive -TimeoutSec 180
        $expected = '3c624e9fbe07e3217552ec52a0f84e2bdc2e6ffa7348f3fdfb9fbf8f42e23fcf'
        if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) {
            throw 'Node archive SHA-256 mismatch.'
        }
        Expand-Archive -LiteralPath $archive -DestinationPath .tools -Force
    }
    . "$PSScriptRoot/tool-env.ps1"
    Invoke-Checked 'corepack.cmd' @('enable', 'pnpm', '--install-directory', $nodeDirectory)
    Invoke-Checked 'corepack.cmd' @('pnpm', 'install')
}
Invoke-Checked 'python' @('tools/check-toolchain.py')
if ($Install) {
    Invoke-Checked 'python' @('tools/third-party.py', '--extract')
    Invoke-Checked 'python' @('tools/generate-media.py')
}
