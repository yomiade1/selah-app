# Installer check (M1.11, M1-A12, SEC-01). Runs on the Windows CI runner after tools/package.ps1:
#
#   1. installs Selah-win-x64-Setup.exe silently, for this user only, with no elevation;
#   2. starts the installed Selah headless and waits for the UI's self-test: "Connected", then the
#      test pattern on the engine's output and in the preview (app/Selah.UI/Services/SelfTest.cs);
#   3. uninstalls it and checks that nothing is left except the user's data.
#
#   tools/test-installer.ps1 [-Setup artifacts/installer/Selah-win-x64-Setup.exe] [-TimeoutSeconds 120]
param(
    [string]$Setup = 'artifacts/installer/Selah-win-x64-Setup.exe',
    [int]$TimeoutSeconds = 120
)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)

$root = Join-Path $env:LOCALAPPDATA 'SelahApp'
$shortcut = Join-Path $env:APPDATA 'Microsoft/Windows/Start Menu/Programs/Selah.lnk'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\SelahApp'

function Fail([string]$message) {
    Write-Output "::error file=tools/test-installer.ps1::$message"
    throw $message
}

function Wait-Until([scriptblock]$condition, [int]$seconds, [string]$what) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if (& $condition) { return }
        Start-Sleep -Milliseconds 250
    }
    Fail "Timed out after $seconds s waiting for $what."
}

function Get-SelahProcesses {
    Get-Process | Where-Object { $_.Path -and $_.Path.StartsWith($root, [StringComparison]::OrdinalIgnoreCase) }
}

# SEC-01: no program may ask Windows for administrator rights.
function Assert-NoElevation([string]$file) {
    $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($file))
    if ($text -match 'requireAdministrator|highestAvailable') {
        Fail "$file asks for administrator rights (SEC-01)."
    }
}

if (-not (Test-Path $Setup)) { Fail "$Setup doesn't exist (run tools/package.ps1 first)." }
if (Test-Path (Join-Path $root 'Update.exe')) { Fail "Selah is already installed for this user ($root). Uninstall it first." }

$userData = Join-Path ([IO.Path]::GetTempPath()) "selah-installer-test-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force $userData | Out-Null
# User data goes to a fresh folder; the processes run with no display; the UI runs its self-test.
$env:SELAH_HOME = $userData
$env:SELAH_HEADLESS = '1'
$env:SELAH_SELF_TEST = '1'

try {
    Assert-NoElevation (Resolve-Path $Setup)

    Write-Output 'installer: installing silently'
    # Start-Process -Wait would also wait for any program Setup starts; wait for Setup alone.
    $install = Start-Process $Setup -ArgumentList '--silent' -PassThru
    if (-not $install.WaitForExit($TimeoutSeconds * 1000)) { Fail 'Setup.exe did not finish.' }
    if ($install.ExitCode -ne 0) { Fail "Setup.exe exited with $($install.ExitCode)." }

    $current = Join-Path $root 'current'
    foreach ($file in 'selah.exe', 'selah-watchdog.exe', 'selah-core.exe', 'selah-engine.exe', 'selah-bench.exe', 'kb.db', 'ui/Selah.exe', 'vcruntime140.dll', 'fonts/notosans/NotoSans[wdth,wght].ttf', 'bench/h264-1080p60.mp4') {
        if (-not (Test-Path -LiteralPath (Join-Path $current $file))) { Fail "The installed copy has no $file." }
    }

    # Help > Diagnostics > Run benchmark uses the installed selah-bench, fonts and clips; run its
    # quick form here so a missing file fails the check, not the person's benchmark.
    Write-Output 'installer: running the installed benchmark (ipc, then 1080p video)'
    foreach ($benchArgs in @(@('ipc', '--iterations', '500'), @('video', '--clip', (Join-Path $current 'bench/h264-1080p60.mp4')))) {
        $ErrorActionPreference = 'Continue' # its progress goes to stderr
        $benchOut = & (Join-Path $current 'selah-bench.exe') @benchArgs 2>&1
        $ErrorActionPreference = 'Stop'
        if ($LASTEXITCODE -ne 0) { Fail "The installed selah-bench $($benchArgs[0]) failed: $($benchOut | Select-Object -Last 1)" }
    }
    if (-not (Test-Path $shortcut)) { Fail 'There is no Start menu shortcut.' }
    if (-not (Test-Path $uninstallKey)) { Fail 'Selah is not listed in Installed apps for this user.' }
    if (Test-Path 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\SelahApp') { Fail 'Setup wrote a machine-wide entry (SEC-01).' }
    if (Get-Service | Where-Object { $_.Name -like '*selah*' }) { Fail 'Setup installed a service (SEC-01).' }
    Get-ChildItem $current -Recurse -Filter '*.exe' | ForEach-Object { Assert-NoElevation $_.FullName }

    # Setup doesn't start Selah when it's silent; stop any copy in case that changes.
    Get-SelahProcesses | Stop-Process -Force

    Write-Output 'installer: starting the installed Selah (headless)'
    $app = Start-Process (Join-Path $current 'selah.exe') -ArgumentList '--wait', '--headless' -PassThru
    $result = Join-Path $userData 'self-test.txt'
    Wait-Until { (Test-Path $result) -or $app.HasExited } $TimeoutSeconds 'the self-test (Connected and the test pattern)'
    if (-not (Test-Path $result)) { Fail "Selah stopped before its self-test (exit $($app.ExitCode))." }
    $outcome = (Get-Content $result -Raw).Trim()
    if (-not $outcome.StartsWith('pass ')) { Fail "The self-test failed: $outcome" }
    if (-not (Test-Path (Join-Path $userData 'ui.txt'))) { Fail 'The UI never connected to core.' }
    Write-Output "installer: self-test $outcome"

    Get-SelahProcesses | Stop-Process -Force
    Wait-Until { -not (Get-SelahProcesses) } 30 'Selah to stop'

    Write-Output 'installer: uninstalling'
    $uninstall = Start-Process (Join-Path $root 'Update.exe') -ArgumentList 'uninstall', '--silent' -PassThru
    if (-not $uninstall.WaitForExit($TimeoutSeconds * 1000)) { Fail 'The uninstaller did not finish.' }
    if ($uninstall.ExitCode -ne 0) { Fail "The uninstaller exited with $($uninstall.ExitCode)." }
    # Update.exe removes itself just after it exits.
    Wait-Until { -not (Test-Path $root) -or -not (Get-ChildItem $root -Recurse -File -ErrorAction SilentlyContinue) } 60 'the program folder to be removed'
    if (Test-Path $shortcut) { Fail 'The Start menu shortcut is still there.' }
    if (Test-Path $uninstallKey) { Fail 'Selah is still listed in Installed apps.' }

    # The user's data stays: settings and the live-state snapshot, and the logs.
    foreach ($kept in 'config', 'logs') {
        if (-not (Get-ChildItem (Join-Path $userData $kept) -File -ErrorAction SilentlyContinue)) { Fail "Uninstalling removed the user's $kept." }
    }
    Write-Output 'installer: GREEN (installed per user, self-test passed, uninstalled cleanly, user data kept)'
}
finally {
    Get-SelahProcesses | Stop-Process -Force -ErrorAction SilentlyContinue
    Remove-Item $userData -Recurse -Force -ErrorAction SilentlyContinue
}
