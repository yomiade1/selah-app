# Installer and updates (M1.11)

How Selah is packaged for Windows, how the package is checked, and how updates work. Requirements:
SEC-01, SEC-12, SEC-13, SAFE-06. Packaging details: [packaging/velopack/README.md](../../packaging/velopack/README.md).

## Building the installer

`tools/package.ps1 [-Channel dev] [-Version 0.0.N] [-SkipBuild]`

1. Fetches the pinned third-party files if they're missing (`tools/third-party.py --extract`, as gate stage 1 does).
2. Builds the Rust processes in release mode with `SELAH_BUILD_VERSION=<version>`, so every process reports the installer's version (`selah_runtime::VERSION`: Status, logs, support bundles, the updater).
3. Builds `kb.db` (`selah-kb build`).
4. Publishes the UI self-contained for `win-x64` (`dotnet publish`, NuGet locked mode; the projects list `RuntimeIdentifiers`).
5. Stages the layout below and runs Velopack (`vpk`, pinned in `dotnet-tools.json`): pack id `SelahApp`, channel `dev`, Start menu shortcut, no portable zip.
6. Copies the setup to `artifacts/installer/Selah-win-x64-Setup.exe`; the feed (`releases.dev.json`, `.nupkg`) stays in `artifacts/velopack/`.

Versions: `0.0.<run number>` for PR builds (`ci.yml`), `0.<N>.0` for milestone builds (`release.yml`).

| Staged path | Source |
|---|---|
| `selah.exe`, `selah-watchdog.exe`, `selah-core.exe`, `selah-engine.exe`, `selah-bench.exe` | `target/release/` |
| `kb.db` | `target/kb/kb.db` |
| `ui/` | `dotnet publish app/Selah.UI` (the UI's `Selah.exe` can't sit next to the `selah.exe` launcher) |
| `avcodec`, `avformat`, `avutil`, `swresample` DLLs | the pinned FFmpeg 7.1 LGPL shared build |
| `bench/hevc-2160p60.mp4`, `bench/h264-1080p60.mp4` | `tests/fixtures/media/generated/` (synthetic) |
| `fonts/notosans/`, `fonts/notosanskr/`, `fonts/notonaskharabic/` | `third_party/fonts/` (the slide fonts the engine and `selah-bench video` draw text with) |
| `msvcp140.dll`, `vcruntime140.dll`, `vcruntime140_1.dll` | the build machine's Visual C++ redistributable folder (app-local; the redistributable installer needs admin) |
| `licences/` | `LICENSE`, FFmpeg's `LICENSE.txt`, the fonts' `OFL.txt` |

The watchdog finds the UI at `ui/Selah.exe` next to itself (or `SELAH_UI_PATH`).

## Checking the installer (M1-A12)

`tools/test-installer.ps1` runs on the Windows CI runner after packaging:

1. **No elevation (SEC-01).** Setup.exe and every installed program have no `requireAdministrator` or `highestAvailable` manifest. There's no machine-wide uninstall entry and no service.
2. **Silent per-user install.** `Setup.exe --silent`; the files, the Start menu shortcut and the per-user Installed apps entry exist.
3. **Self-test.** With `SELAH_HOME` (a fresh folder), `SELAH_HEADLESS=1` and `SELAH_SELF_TEST=1`, it starts `current\selah.exe --wait --headless`. When the real UI first shows "Connected" (core and engine connected), it asks for SMPTE bars through core, waits until the engine reports that scene with a frame checksum, and checks the preview ring's first pixel is the 75% grey bar. It writes `self-test.txt`: `pass <checksum>` or `fail <reason>` (`app/Selah.UI/Services/SelfTest.cs`).
4. **Clean uninstall.** `Update.exe uninstall --silent` removes the program folder, the shortcut and the Installed apps entry; the user's `config` and `logs` remain.

## Updates

Code: `crates/selah-core/src/update.rs` (logic), `crates/selah-core-host` (runs it), `app/Selah.UI/ViewModels/UpdatesViewModel.cs` (Settings ▸ Updates).

| Command (`envelope.proto`) | What core does |
|---|---|
| `SetUpdateSettings { update_feed_url, allow_update_check }` | Validates the feed (`https://`, a `.json` file, no credentials in the address) and saves it in `settings.json`. |
| `CheckForUpdate {}` | Refuses with `SEL-UPD-0001` unless checks are on and a feed is set (SEC-13). Otherwise reads the feed (≤ 1 MiB, ≤ 2000 assets) and replies `UpdateCheck { available, version, notes_markdown, current_version }`. |
| `InstallUpdate { version }` | Refuses with `SEL-UPD-0004` while anything is live. Otherwise downloads the full package next to the feed (limited to the size the feed states), checks size and SHA-256 (`SEL-UPD-0003` on mismatch; nothing is saved), checks that nothing went live meanwhile, then starts `Update.exe apply --silent --package <file>` and replies. Velopack closes every program in the install folder, swaps the version and starts Selah again. |

Feed parsing keeps only `Type: Full` assets for `SelahApp` with a plain `.nupkg` file name (no `..`, slashes or drive letters), a 64-hex-digit SHA-256 and a size between 1 byte and 1 GiB. The newest version above the running one (SemVer) is offered.

Slow work (`Work::CheckForUpdate`, `Work::InstallUpdate`) runs off the request path: `CoreService::handle` returns the work, the host runs it on a blocking thread through `Fetch` (implemented by `selah-net`), then calls `finish_update_check` or `finish_update_install` for the reply.

The UI asks first when something is on the screens: a bar under the toolbar says **Something is on the screens.** with **Keep showing** and **Clear and update**. Clearing goes over the emergency lane (SAFE-03).

Error codes: `SEL-UPD-0001` (off), `0002` (feed unreadable), `0003` (verification failed), `0004` (live), `0005` (installer didn't start). Fix article: `getting-started.update-selah`.

Not yet: signing and signature checks (H-15, M6/M9), delta packages, macOS packaging (M6).
