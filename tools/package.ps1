# Build the Windows installer (M1.11, SEC-01): a per-user Velopack Setup.exe that needs no
# administrator rights. Layout and reasons: packaging/velopack/README.md.
#
#   tools/package.ps1 [-Channel dev] [-Version 0.0.1] [-SkipBuild]
#
# Output: artifacts/installer/Selah-win-x64-Setup.exe (uploaded by CI), plus the update feed
# (releases.<channel>.json and the .nupkg files) in artifacts/velopack/.
param(
    [string]$Channel = 'dev',
    [string]$Version = '0.0.0-local',
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root
. (Join-Path $PSScriptRoot 'tool-env.ps1')

if ($Channel -notmatch '^[a-z][a-z0-9-]{0,31}$') { throw "Channel '$Channel' must be lower-case letters, digits and dashes." }
if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') { throw "Version '$Version' must be SemVer 2, like 0.0.12." }

$packId = 'SelahApp'
$stage = Join-Path $root 'artifacts/package/stage'
$feed = Join-Path $root 'artifacts/velopack'
$installer = Join-Path $root 'artifacts/installer'

# The pinned, hash-checked third-party files (FFmpeg, Skia, fonts), as gate stage 1 does.
if (-not (Test-Path (Join-Path $root 'third_party/fonts'))) {
    Invoke-Native 'python' @('tools/third-party.py', '--extract')
    . (Join-Path $PSScriptRoot 'tool-env.ps1')
}
if (-not $env:FFMPEG_DIR) { throw 'The pinned FFmpeg is missing (python tools/third-party.py --extract).' }

if (-not $SkipBuild) {
    # The version every process reports (Status.version, logs, support bundles, the updater).
    $env:SELAH_BUILD_VERSION = $Version
    Invoke-Native 'cargo' @('build', '--release', '--locked',
        '-p', 'selah-watchdog', '-p', 'selah-core-host', '-p', 'selah-engine', '-p', 'selah-bench', '-p', 'selah-kb')
    Invoke-Native 'cargo' @('run', '--release', '--locked', '-q', '-p', 'selah-kb', '--bin', 'selah-kb', '--',
        'build', '--out', 'target/kb/kb.db')
    if (Test-Path "$stage-ui") { Remove-Item "$stage-ui" -Recurse -Force }
    Invoke-Native 'dotnet' @('publish', 'app/Selah.UI/Selah.UI.csproj', '-c', 'Release', '-r', 'win-x64',
        '--self-contained', '-p:DebugType=none', "-p:Version=$Version", '-o', "$stage-ui")
}

if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage, $feed, $installer | Out-Null

# Rust processes and the help index (core finds kb.db next to selah-core.exe).
foreach ($name in 'selah', 'selah-watchdog', 'selah-core', 'selah-engine', 'selah-bench') {
    Copy-Item "target/release/$name.exe" $stage
}
Copy-Item 'target/kb/kb.db' $stage

# The UI, in its own folder: its Selah.exe would clash with the selah.exe launcher.
Copy-Item "$stage-ui" (Join-Path $stage 'ui') -Recurse

# FFmpeg 7 LGPL shared libraries for Help > Diagnostics > Run benchmark (selah-bench), and the
# synthetic benchmark clips (tools/generate-media.py).
$ffmpegBin = Join-Path $env:FFMPEG_DIR 'bin'
foreach ($dll in 'avcodec-*.dll', 'avformat-*.dll', 'avutil-*.dll', 'swresample-*.dll') {
    Copy-Item (Join-Path $ffmpegBin $dll) $stage
}
$bench = Join-Path $stage 'bench'
New-Item -ItemType Directory -Force $bench | Out-Null
$clips = Join-Path $root 'tests/fixtures/media/generated'
if (-not (Test-Path (Join-Path $clips 'hevc-2160p60.mp4'))) {
    Invoke-Native 'python' @('tools/generate-media.py')
}
Copy-Item (Join-Path $clips 'hevc-2160p60.mp4'), (Join-Path $clips 'h264-1080p60.mp4') $bench

# The Visual C++ runtime, deployed next to the programs (app-local), because installing the
# redistributable needs administrator rights (SEC-01).
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$crt = Get-ChildItem (Join-Path $vs 'VC/Redist/MSVC/*/x64/Microsoft.VC14*.CRT') -Directory | Sort-Object FullName | Select-Object -Last 1
if (-not $crt) { throw 'The Visual C++ redistributable files were not found (Visual Studio C++ build tools).' }
foreach ($dll in 'msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll') {
    Copy-Item (Join-Path $crt.FullName $dll) $stage
}

# Licences: Selah's, FFmpeg's (LGPL) and the fonts' (SIL OFL) travel with the program.
$licences = Join-Path $stage 'licences'
New-Item -ItemType Directory -Force $licences | Out-Null
Copy-Item 'LICENSE' (Join-Path $licences 'Selah-LICENSE.txt')
Copy-Item (Join-Path $env:FFMPEG_DIR 'LICENSE.txt') (Join-Path $licences 'FFmpeg-LICENSE.txt')
Get-ChildItem 'third_party/fonts' -Recurse -File -Include 'OFL*.txt', 'LICENSE*' | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $licences "font-$($_.Directory.Name)-$($_.Name)")
}

# Velopack: per-user install into %LOCALAPPDATA%\SelahApp, Start menu shortcut, no admin.
Invoke-Native 'dotnet' @('tool', 'restore')
Invoke-Native 'dotnet' @('vpk', 'pack',
    '--packId', $packId, '--packVersion', $Version, '--packDir', $stage, '--mainExe', 'selah.exe',
    '--runtime', 'win-x64', '--packTitle', 'Selah', '--packAuthors', 'Selah', '--channel', $Channel,
    '--shortcuts', 'StartMenuRoot', '--noPortable', '--skipVeloAppCheck',
    '--releaseNotes', 'packaging/velopack/release-notes.md', '--outputDir', $feed)

$setup = Get-ChildItem $feed -Filter "$packId-$Channel-Setup.exe" | Select-Object -First 1
if (-not $setup) { $setup = Get-ChildItem $feed -Filter '*-Setup.exe' | Sort-Object LastWriteTime | Select-Object -Last 1 }
if (-not $setup) { throw 'vpk did not produce a Setup.exe.' }
Get-ChildItem $installer -File | Remove-Item -Force
Copy-Item $setup.FullName (Join-Path $installer 'Selah-win-x64-Setup.exe')
Write-Output "package: $Version ($Channel) -> artifacts/installer/Selah-win-x64-Setup.exe"
