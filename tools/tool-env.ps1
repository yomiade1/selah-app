# Add project-local SDKs to this process only. Never downloads or changes user settings.
$projectRoot = Split-Path $PSScriptRoot -Parent
$localDotnet = Join-Path $projectRoot '.tools/dotnet'
if (Test-Path (Join-Path $localDotnet 'dotnet.exe')) {
    $env:DOTNET_ROOT = $localDotnet
    $env:PATH = "$localDotnet;$env:PATH"
}
$localNode = Join-Path $projectRoot '.tools/node-v22.21.1-win-x64'
if (Test-Path (Join-Path $localNode 'node.exe')) {
    $env:PATH = "$localNode;$env:PATH"
}
$cargoBin = Join-Path $env:USERPROFILE '.cargo/bin'
if (Test-Path (Join-Path $cargoBin 'cargo.exe')) {
    $env:PATH = "$cargoBin;$env:PATH"
}
# buf format --diff needs a diff program; Git for Windows ships one. Appended, never prepended.
$gitUsrBin = Join-Path $env:ProgramFiles 'Git/usr/bin'
if ((Test-Path (Join-Path $gitUsrBin 'diff.exe')) -and -not (Get-Command diff.exe -ErrorAction SilentlyContinue)) {
    $env:PATH = "$env:PATH;$gitUsrBin"
}
$llvmBin = Join-Path $env:ProgramFiles 'LLVM/bin'
if (Test-Path (Join-Path $llvmBin 'clang.exe')) {
    $env:LIBCLANG_PATH = $llvmBin
    $env:PATH = "$llvmBin;$env:PATH"
}

# Run a native program and fail on a non-zero exit code. Windows PowerShell 5.1 turns any
# stderr line into an error record, which 'Stop' would make fatal even on success, so the
# preference is relaxed for the call and the exit code is what decides.
function Invoke-Native([string]$Program, [string[]]$Arguments) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Program @Arguments 2>&1 | ForEach-Object { "$_" } }
    finally { $ErrorActionPreference = $previous }
    if ($LASTEXITCODE -ne 0) { throw "$Program $($Arguments -join ' ') failed (exit $LASTEXITCODE)." }
}

# skia-bindings reads the pinned, hash-checked Skia build instead of downloading it (SEC-09).
$env:SKIA_BINARIES_URL = 'file://' + $projectRoot.Replace('\', '/') + '/third_party/skia/windows-x64/skia-binaries-{key}.tar.gz'
# The FFmpeg bindings (selah-media) build against the pinned FFmpeg; its DLLs must be on PATH to run.
$ffmpegShared = Join-Path $projectRoot 'third_party/ffmpeg/windows-x64/shared'
$ffmpegHome = Get-ChildItem $ffmpegShared -Directory -Filter 'ffmpeg-*' -ErrorAction SilentlyContinue | Select-Object -First 1
if ($ffmpegHome) {
    $env:FFMPEG_DIR = $ffmpegHome.FullName
    $env:PATH = "$(Join-Path $ffmpegHome.FullName 'bin');$env:PATH"
}
$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
$env:DOTNET_NOLOGO = '1'
