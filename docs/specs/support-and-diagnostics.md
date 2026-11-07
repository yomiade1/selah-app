# Problem reports and Diagnostics (M1.13)

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
  Otherwise core runs the installed `selah-bench`: `ipc --iterations 10000`, then
  `video --clip bench/hevc-2160p60.mp4` (a quick run uses 500 round trips and the 1080p H.264
  clip). It saves `selah-diagnostics-<UTC stamp>.json` in the support folder (both bench
  reports, a summary and the details above, redacted) and replies with the summary. The window
  shows the results and **Show the report**.

## Tests

`a_bundle_has_logs_and_versions_but_no_secrets_or_user_name` (seeded secret, email, user name,
token in the update address), `report_a_problem_saves_a_bundle_with_the_version` and
`diagnostics_shows_the_engine_and_writes_a_benchmark_report` on the real processes,
`DiagnosticsTests` in the UI (including the Diagnostics window at every text size, appearance
and language).
