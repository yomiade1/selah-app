# Windows installer (Velopack)

`tools/package.ps1` builds `artifacts/installer/Selah-win-x64-Setup.exe` and the update feed in
`artifacts/velopack/`. CI builds it on every PR that changes code (`0.0.<run>`), and
`release.yml` publishes it for milestone tags (`0.<N>.0`). `tools/test-installer.ps1` checks it on
the Windows runner (M1-A12).

## What it does

- **Per user, no administrator rights (SEC-01).** Setup installs into `%LOCALAPPDATA%\SelahApp`,
  adds a Start menu shortcut called **Selah**, and lists Selah under Installed apps for this user.
  Nothing runs as a service or asks for elevation; the installer test checks every program's
  manifest. Velopack's pack id is `SelahApp` rather than `Selah` because Selah keeps its logs and
  cache in `%LOCALAPPDATA%\Selah` (ARCHITECTURE §4.1), and the program folder is replaced on every
  update.
- **Uninstalling keeps the user's data.** The uninstaller removes the program folder, the shortcut
  and the Installed apps entry. Settings (`%APPDATA%\Selah`) and the library (`Documents\Selah
  Library`) stay.
- **Channel.** Test builds use the `dev` channel (`releases.dev.json`).
- **Installer hooks.** Velopack runs the main program with `--veloapp-install` and similar
  arguments during install, update and uninstall. The `selah` launcher exits at once for these,
  so installing never starts Selah.

## Layout of the installed folder (`current\`)

| File | What it is |
|---|---|
| `selah.exe` | The launcher the shortcut runs. It starts the watchdog. Shift held at start means safe mode. |
| `selah-watchdog.exe`, `selah-core.exe`, `selah-engine.exe` | The processes (ADR-001). |
| `selah-bench.exe`, `bench\*.mp4` | **Help ▸ Diagnostics ▸ Run benchmark** and its synthetic clips. |
| `kb.db` | The help index, next to `selah-core.exe` where core looks for it. |
| `ui\Selah.exe` and the .NET files | The Avalonia UI, self-contained. It has its own folder because `Selah.exe` and `selah.exe` would be the same file on Windows. |
| `avcodec-*.dll`, `avformat-*.dll`, `avutil-*.dll`, `swresample-*.dll` | FFmpeg 7 LGPL shared libraries (used by `selah-bench`). |
| `msvcp140.dll`, `vcruntime140*.dll` | The Visual C++ runtime, app-local, because installing the redistributable needs administrator rights. |
| `licences\` | Selah's licence, FFmpeg's (LGPL) and the fonts' (SIL OFL). |

## Updates (SEC-12, SEC-13, SAFE-06)

- Selah never looks for updates on its own. The person turns on **Let Selah check for updates
  when I ask** and pastes an update address (an `https://` link to `releases.<channel>.json`) in
  **Settings ▸ Updates**. Nothing is built in: no address, no token.
- **Check for updates** reads the feed through `selah-net`. **Install update** downloads the full
  package next to the feed, checks its size and SHA-256 against the feed, and only then hands it
  to Velopack's `Update.exe apply`, which closes Selah, swaps the version and starts it again.
- Core refuses to install while anything is on the screens, and checks again just before handing
  the package over. The UI asks first (**Clear and update** or **Keep showing**).
- Signing waits for H-15. Until then Windows SmartScreen may warn about the unsigned Setup.exe.

The code is in `crates/selah-core/src/update.rs`; the spec is `docs/specs/installer-and-updates.md`.
