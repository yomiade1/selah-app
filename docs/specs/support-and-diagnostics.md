# Problem reports and Diagnostics (M1.13, extended M3-A6)

Requirements: SEC-05, SEC-06, SAFE-06, ADR-023 (hardware results come from the user's PC).

## Help ▸ Report a problem

1. The UI sends `CreateSupportBundle`. Core (`selah-core::support`) writes
   `selah-support-<UTC stamp>.zip` to the support folder (`Paths::support`:
   `%LOCALAPPDATA%\Selah\support`, `~/Library/Logs/Selah/Support`, or `$SELAH_HOME/support`).
2. The bundle holds `versions.json` (version, OS, architecture, safe mode), `settings.json`
   (the update address cut to scheme, host and path), the last 4 MiB of each of the newest 40
   log files, and the newest Diagnostics report. Every file passes through
   `selah_runtime::redact` again before it's written: registered secrets (every value that went
   through `SecretStore`), bearer tokens, `password=`-style values, stream keys, email addresses
   and the user's name in paths and as a word.
3. Core replies with the path and the problem-report form address with the version filled in
   (`…/issues/new?template=problem-report.yml&version=<v>`). The UI opens the form in the
   browser and shows a bar under the toolbar with the path, **Show the file**, **Open the form
   again** and **Close**. Nothing is uploaded: the person attaches the file.

The zip writer is minimal (deflate, UTF-8 names, no zip64); `support::read_entries` reads it
back for tests.

## Help ▸ Diagnostics

- **Details:** `GetDiagnostics` returns the version, OS and architecture, safe mode, and the
  engine's renderer (for example `wgpu Dx12 NVIDIA GeForce MX350`) and output size.
- **Run benchmark:** `RunDiagnostics` is refused while anything is live (`SEL-DIAG-0001`).
  Otherwise core runs the installed `selah-bench` across three stages:
  1. `ipc --iterations 10000` (quick run: `500`),
  2. `video --clip bench/hevc-2160p60.mp4` (quick run: `h264-1080p60.mp4`),
  3. `m3 --iterations 30 --clip bench/hevc-2160p60.mp4` (quick run: `4` iterations on `h264-1080p60.mp4`, falling back to synthetic 4K NV12 frames when the clip is under 4K) measuring dual 1080p outputs over a 4K video background (`OUTPUT_P99_BUDGET_MS = 8.0` on hardware adapters).
  Core saves `selah-diagnostics-<UTC stamp>.json` (holding `ipc`, `video`, `m3`, `summary`, and
  `context`, redacted) in the support folder and replies with `DiagnosticsReport` including
  `m3_p99_ms` and `m3_pass`. The Diagnostics window displays all three benchmark results and
  **Show the report**.

## Tests

`a_bundle_has_logs_and_versions_but_no_secrets_or_user_name` (seeded secret, email, user name,
token in the update address), `report_a_problem_saves_a_bundle_with_the_version`,
`diagnostics_shows_the_engine_and_writes_a_benchmark_report` on the real processes (asserting `ipc`,
`video`, and `m3` sections in the saved report JSON), `diagnostics::tests` (`the_summary_reads_all_three_reports`),
`DiagnosticsTests` (17 UI tests including the Diagnostics window across 100/150/200%, Light/Booth,
`en`/`en-XA`), `ContractTests` (C#), and `web/contracts` round-trip tests.
