# Selah progress ledger

This file is the single source of truth for **what is done and what comes next**. Every coding session reads it first and updates it as work completes (see the protocol in [CLAUDE.md](CLAUDE.md)). Tick a box only when its evidence is real: name the test, command or document that proves it.

Status values: `Not started` · `In progress` · `Blocked` (say on what) · `Done` · `Done, H-checks pending`.

## Current status

| Milestone | Status | Branch / tag | Started | Finished | Notes |
|---|---|---|---|---|---|
| M1. Foundations and risk spikes | Done, H-checks pending | `claude/m1-finish` | 2026-11-07 | 2026-11-07 | All 13 WPs done (local Windows sessions, H-01). Open (H): M1-A7 benchmark report, M1-A12 install on your PC; M1-A1/A6 macOS from the `macos` job. |
| M2. Library, songs, scripture and the help system | Done, H-checks pending | `codex/m2-library` | 2026-11-09 | 2026-11-09 | All 9 WPs done (local Windows session, H-01). Open (H): M2-A10 / H-06 (Bible rights beyond public-domain KJV & WEB) and H-18 (2i KB review; 37 articles in `review`). |
| M3. Render engine, templates and the slide editor | Done, H-checks pending | `antigravity/m3-render-engine` | 2026-11-10 | 2026-11-10 | All 8 WPs done (local Windows session, H-01). Open (H): M3-A6 (H) Help ▸ Diagnostics 2×1080p + 4K benchmark on your PC (H-23 reference hardware in M10) and H-18 (2i KB review; 57 articles in `review`). `ci:macos` in PR CI. |
| M4. Live operation, services and stage | Not started | — | — | — | |
| M5. Screens, video and audio I/O | Not started | — | — | — | |
| M6. Importers, migration and the alpha release | Not started | — | — | — | |
| M7. Remote control, API and show control | Not started | — | — | — | |
| M8. Broadcast, creative and intelligence | Not started | — | — | — | |
| M9. Cloud, teams and enterprise | Not started | — | — | — | |
| M10. Launch readiness and v1.0 | Not started | — | — | — | |

**Next action:** start Milestone 4 (M4. Live operation, services and stage). Human checklists M1–M3 remain open below.

**Open risks carried into M4** (from the M1–M3 review on 2026-11-11):

| Risk | State | Who clears it |
|---|---|---|
| The 8 ms per-frame budget (ARCHITECTURE §12) is unmet by every measurement so far: `14.86 ms` on an MX350 laptop GPU, `57.40 ms` on lavapipe. Both machines lack a real GPU, so this is unproven rather than failed, but nothing yet shows Selah hitting 8 ms. | Open | You, on reference hardware (`H-23`, M10). `selah-bench m3` already reports `hardware_pass` honestly. |
| `selah-media` and `selah-testkit` hold Windows-only code that no cloud session can type-check (FFmpeg and Skia have no cloud-buildable Windows target). The `windows` CI job is the only thing that compiles it. | Accepted, recorded | `tools/windows-check-exclusions.txt`; `tools/check-windows-coverage.py` keeps the list honest. |
| The template editor and slide editor (M3.7) have no counterpart in `design/prototype/`, so the visual reference is behind the app. | Open | A prototype screen for each, or a note that the app is now the reference. |
| 57 help articles sit at `status: review`. | Open | `H-18` (content designer). |

## Resume notes

_When a session stops partway through a milestone, write here: the milestone, the last finished WP, anything half-done (files, branch state) and the exact next step. Clear it when the milestone is done._

(none — Milestone 3 complete)

## Milestone checklists

### M1. Foundations and risk spikes

**Work packages**

- [x] **M1.1** Repository, toolchain and conventions — evidence: commit `M1.1`; `tools/setup.ps1 -CheckOnly` passes (Rust 1.91.0, .NET 10, Node 22, pnpm 10.20; LLVM pending); `python tools/third-party.py --extract` verifies every pinned file; `tools/generate-media.py` made and decoded 4 clips; `python -m unittest discover -s tests/tools` (20 tests); `cargo deny check` ok; lockfiles committed
- [x] **M1.2** Contracts and code generation — evidence: commit `M1.2`; `buf lint` and `buf format --exit-code` clean; `tools/codegen.ps1` regenerates TS/Rust/C#; shared wire fixture decoded by `selah-proto` tests, `ContractTests` (C#) and `web/contracts` tests
- [x] **M1.3** IPC layer — evidence: commit `M1.3`; `selah-ipc` tests (`ten_thousand_round_trips_keep_correlation`, `malformed_and_oversized_frames_close_only_that_connection`, `client_reconnects_after_the_server_restarts`, `pipe_dacl_allows_only_the_current_user`, `a_second_server_cannot_take_over_a_live_endpoint`, ring seqlock tests), `fuzz_smoke`, C# `IpcClientTests`/`PreviewRingReaderTests`; `selah-bench ipc --check`: named pipe p99 0.096 ms
- [x] **M1.4** Processes and watchdog — evidence: commit `M1.4`; `selah-testkit` continuity tests (engine, core and UI kills recover in < 2 s; 3 crashes → safe mode; single instance) pass 3 runs in a row; redaction tests (SEC-06); permission matrix (SEC-14); settings/live-state tests
- [x] **M1.5** Engine windows, compositor and test patterns — evidence: commit `M1.5`; `selah-render` `gpu_test_patterns_match_the_cpu_reference_exactly` (WARP; exact match at 1920×1080, 1366×768, 17×9); `selah-testkit` `show_test_pattern_reaches_the_output_within_one_frame` (FrameShown ≤ 1 frame after the command, bars checksum, preview ring); windowed run on an NVIDIA MX350 (DX12) opened the output window (Bgra8Unorm)
- [x] **M1.6** Text spike — evidence: commit `M1.6`; `golden_text` (en, yo, ko, ar against `tests/golden/reference/`, no missing glyphs, GPU = CPU blending, paragraph cache); references reviewed by eye (tests/golden/CHANGES.md); `tools/verify.ps1` stage 7 green
- [x] **M1.7** Video spike — evidence: commit `M1.7`; `selah-media` (FFmpeg 7.1 via ffmpeg-next, D3D11VA/VideoToolbox with software fallback, NV12 out) and the `selah-render` video layer (`video.wgsl`, NV12 → RGB under text); `selah-testkit` `video` tests (`clip_plays_in_order_with_no_drops_on_a_60_fps_virtual_clock`, `text_composites_over_video_and_gpu_matches_the_cpu_reference`; WARP + software decode); SEC-04 tests in `selah-media` (`opening_an_http_url_is_refused`, `a_local_playlist_cant_reach_the_network`, protocol-prefix refusal); `selah-bench video` on this PC (MX350): 1080p60 H.264 0 drops, composite p99 5.1 ms (D3D11VA); 4K60 HEVC 9 of 180 dropped, p99 16 ms (spike report); gate stages 1–7, 13, 14 green on Windows
- [x] **M1.8** UI shell, design tokens and text size — evidence: commit `M1.8`; `app/Selah.UI` (Avalonia 12.1.5: toolbar, sidebar, preview from the engine's ring, inspector, safe-mode banner, Light/Booth, bundled Inter/Manrope/JetBrains Mono, Settings ▸ Appearance with 7-step text size, "Use my computer's text size", live preview, Ctrl/Cmd + − 0, F12 Clear everything over the emergency lane); tokens generated by `pnpm tokens` and checked by `check-generated`; Fluent strings `content/i18n/en/*.ftl` with the `en-XA` pseudo-locale; `app/Selah.UI.Tests` (41 headless tests: layout matrix 100/150/200% × Light/Booth × en/en-XA with no clipped critical labels, token contrast, strings, shortcuts, breakpoints); `tools/lint-strings.py` + `content/terms.yaml`; the real UI (headless) now runs in the M1-A2/M1-A3 process tests in place of `selah-fake-ui` (3 runs green, UI back in < 2 s); gate stage 8 added; full gate green on Windows
- [x] **M1.9** Knowledge base pipeline — evidence: commit `M1.9`; `selah-kb` (front matter, Markdown → AST, `kb.db` with FTS5 + trigram + anchors + collections + synonyms, `HelpIndex` search/article/context) and `selah-kb-lint` (R1–R4, R5/R6/R10 warnings, R7–R9, R11–R13, R16–R18, KB-SCHEMA/ID/LINK/ANCHOR/SETTING/ERROR/LENGTH/HTML/IMAGE/PLANNED/REVIEW/ORPHAN, release mode); `crates/selah-kb-lint/tests/lint.rs` (the exemplars and 2 new articles have no errors; 22 bad fixtures each fail with their rule; planned items warn until their milestone is Done; kb.db builds and finds "why are my screens black", "put lyrics up" and the typo "blnding"; the AST fixture is current); core answers SearchHelp/GetArticle/HelpForContext (`crates/selah-core/tests/help.rs`); `content/terms.yaml`, `content/help/anchors.yaml`, `content/help/settings.yaml`, `content/errors/errors.yaml` (SEL-OUT-0001/0002, SEL-APP-0001 …), `content/kb/planned.yaml`, `content/kb/_templates/` (7 types); articles `troubleshooting.selah-started-in-safe-mode` and `getting-started.report-a-problem`; the UI help window renders the AST natively (`HelpTests`, 100%/Light and 200%/Booth); gate stage 9 (`selah-kb-lint`, `selah-kb build`)
- [x] **M1.10** Quality gate and CI — evidence: commit `M1.10`; `tools/verify.ps1` green on Windows (stages 1–7, 13, 14); `tests/ledger.json` created (rust 54, dotnet 20, web 3, golden 4, e2e 6); `tests/tools/test_ledger.py`; `check-workflows` passes for ci.yml, release.yml and bless-golden.yml. Stage 8 (UI), stage 9 (KB) and `tools/lint-strings` are added by M1.8 and M1.9, which create what they check; first PR run green: guard, linux, windows (PR #1, run 42357662100)
- [x] **M1.11** Windows installer and updates — evidence: commit `M1.11`; `tools/package.ps1` (Velopack 1.2.161 `vpk` pinned in `dotnet-tools.json`; pack id `SelahApp`, per user in `%LOCALAPPDATA%\SelahApp`, Start menu shortcut, `dev` channel, version from `SELAH_BUILD_VERSION`; UI self-contained in `ui\`, app-local VC++ runtime, FFmpeg DLLs and bench clips for Diagnostics); `tools/test-installer.ps1` green on this PC (no elevation in any manifest, no service or machine-wide entry, silent install, headless self-test `pass` = Connected + SMPTE bars on the output and in the preview, clean uninstall, user data kept); updater in `selah-core::update` (`only_https_json_feeds_are_accepted`, `feeds_are_parsed_with_limits`, `a_package_with_the_wrong_hash_or_size_is_never_saved`, `update_checks_are_off_at_default_settings`, `updates_never_install_while_something_is_on_the_screens`), Settings ▸ Updates and the live guard bar (`UpdateTests`, 5); Velopack `Update.exe apply` checked by hand: 0.0.1 → 0.0.2 while running (stopped every process, swapped, restarted); article `getting-started.update-selah`, errors `SEL-UPD-0001`–`0005`; spec `docs/specs/installer-and-updates.md`; full gate green on Windows
- [x] **M1.12** Security and safety baseline — evidence: commit `M1.12`; `selah-net` (purposes, default-off policy, https-only addresses, size limits, host-only connection log; ureq + rustls/ring + OS verifier): `the_default_policy_connects_to_nothing`, `only_plain_https_addresses_get_through`, `only_selah_net_reaches_the_network` (no other crate depends on an HTTP client or opens sockets); core sets the policy from settings (`the_network_policy_follows_the_update_settings`, `update_checks_are_off_at_default_settings`); egress test on the real processes, UI included: `default_settings_make_no_outgoing_connections` (netstat / /proc / lsof); `selah-secrets` (`SecretStore`: Credential Manager with DPAPI, Keychain, memory; every value registered with the redactor): `values_round_trip_and_are_redacted_from_logs`, `the_platform_keychain_round_trips` (real Credential Manager on this PC); SAFE-03 chaos test `clear_everything_works_with_core_frozen` (core's threads suspended; cleared frame within one frame period); SAFE-06 quit guard (`LiveGuardTests`) and keep-awake while live (`selah_runtime::power`, `the_computer_stays_awake_only_while_something_is_on_the_screens`); SAFE-07 **Restart normally** (`restart_normally_leaves_safe_mode_and_keeps_the_screens`); SEC-14 matrix extended to every new command; docs/SECURITY.md §3 rows and §9 baseline table; full gate green on Windows (the chaos test was added after it and run on its own)
- [x] **M1.13** Problem reports, diagnostics and spike report — evidence: commit `M1.13`; **Help** menu in the toolbar (Help articles, Report a problem, Diagnostics); `selah-core::support` (redacted zip: versions, settings, log tails, newest Diagnostics report) and `a_bundle_has_logs_and_versions_but_no_secrets_or_user_name` (seeded `SecretStore` secret, email, user name, token in the feed address); the report bar opens the problem form with the version filled in; `selah-core::diagnostics` runs `selah-bench ipc` and `video` (refused while live) and saves `selah-diagnostics-*.json`; the engine reports its renderer and output size; stack tests `report_a_problem_saves_a_bundle_with_the_version`, `diagnostics_shows_the_engine_and_writes_a_benchmark_report`; `DiagnosticsTests` (17, incl. the Diagnostics window across the 12-case matrix); article `getting-started.run-diagnostics`, errors `SEL-SUP-0001`, `SEL-DIAG-0001`/`0002`; specs `command-bus.md`, `spike-report.md` (go), `support-and-diagnostics.md`; full gate green on Windows

**Acceptance criteria**

- [x] **M1-A1** (auto) `tools/verify.sh --cloud` exits 0 in a cloud session. — evidence: `tools/verify.sh --cloud` run in a cloud session on 2026-11-11 and **GREEN** (all stages, 13 and 14 included); this is the first time it was actually run, and it was red until the Skia fix below. M1–M3 were built in local Windows sessions (H-01) where `tools/verify.ps1` was the gate and the cross-target Windows check doesn't exist, so the stage had never executed. Fix: the Windows type-check skips the crates needing a prebuilt Skia or FFmpeg for `x86_64-pc-windows-gnu` (`tools/windows-check-exclusions.txt`), and `tools/check-windows-coverage.py` fails the gate if a crate gains `#[cfg(windows)]` code while excluded. Also green on Windows (`tools/verify.ps1`). guard/linux/windows and macos: this PR's CI (label `ci:macos`)
- [x] **M1-A2** (auto) Starting `selah` launches core, engine and UI. — evidence: `show_test_pattern_reaches_the_output_within_one_frame` (real UI headless, "Connected", bars checksum = CPU reference, FrameShown ≤ 1 frame); the installed build's self-test (`tools/test-installer.ps1`)
- [x] **M1-A3** (auto) If the UI is killed, the output frame checksum doesn't change, and the UI is relaunched and reconnected within 2 s. — evidence: `ui_killed_changes_nothing_on_screen_and_comes_back` with the real UI (checksum unchanged, back in < 2 s)
- [x] **M1-A4** (auto) If the engine is killed, it's restarted within 2 s and the previous scene is restored. — evidence: `engine_killed_is_restarted_and_the_scene_restored`
- [x] **M1-A5** (auto) If core is killed, the engine keeps the last frame; core restarts and resynchronises within 2 s. — evidence: `core_killed_leaves_the_last_frame_and_resynchronises`
- [x] **M1-A6** (auto) Golden text frames for en, yo, ko and ar pass on lavapipe. — evidence: `golden_text` (en, yo, ko, ar) green on Windows (WARP); linux job in CI; macOS in this PR's `macos` job
- [x] **M1-A7** (auto) The 1080p H.264 clip decodes and composites under text with frames in order and no drops on a 60 fps virtual clock. — evidence: `clip_plays_in_order_with_no_drops_on_a_60_fps_virtual_clock` (auto). (H) open: run **Help ▸ Diagnostics ▸ Run benchmark** on the installed build and attach the report (checklist below); hand measurements in `docs/specs/spike-report.md`
- [x] **M1-A8** (auto) (ci-win) The IPC p99 round trip is ≤ 2 ms on Unix sockets and on named pipes (bench results recorded). — evidence: named pipes p99 0.096 ms (`selah-bench ipc --check`, gate stage 6 on Windows); Unix sockets: the same check in the linux job
- [x] **M1-A9** (auto) UI tests pass at 100/150/200%, Light and Booth, `en` and `en-XA`, with no clipped critical labels. — evidence: `LayoutTests` matrix (100/150/200 × Light/Booth × en/en-XA, now with the live-guard bar), `DiagnosticsTests` matrix, token contrast test
- [x] **M1-A10** (auto) The 3 exemplar articles pass `selah-kb-lint`; each bad fixture fails with its expected rule ID; `kb.db` builds; an article renders in the UI. — evidence: gate stage 9 (`selah-kb-lint`: 7 articles, 0 errors; bad fixtures fail with their rule; `kb.db` builds); `HelpTests` render an article
- [x] **M1-A11** The spike report is written with measurements and a go/no-go. — evidence: `docs/specs/spike-report.md`: measurements against budgets, recommendation **go** (4K60 HEVC needs zero-copy textures, M3/M5)
- [x] **M1-A12** (ci-win) The `windows` job builds `Selah-win-x64-Setup.exe`, and `tools/test-installer.ps1` installs it silently, sees "Connected" and the test pattern, and un… — evidence: `tools/package.ps1` + `tools/test-installer.ps1` green on this PC (silent per-user install, self-test pass, clean uninstall); CI `windows` job runs both. (H) open: install from the PR on your PC (checklist below)
- [x] **M1-A13** (auto) Security baseline tests pass: IPC access and limits (SEC-02), the IPC fuzz smoke run (SEC-03), FFmpeg refusing network URLs in file items (SEC-04), seed… — evidence: SEC-02 `selah-ipc` tests; SEC-03 `fuzz_smoke`; SEC-04 `opening_an_http_url_is_refused`; SEC-05/06 `a_bundle_has_logs_and_versions_but_no_secrets_or_user_name`, `values_round_trip_and_are_redacted_from_logs`, redaction tests; SEC-13 `only_selah_net_reaches_the_network`, `default_settings_make_no_outgoing_connections`; SEC-14 `permission_matrix_covers_every_origin_and_command`; SEC-11/09/10 gate stage 14
- [x] **M1-A14** (auto) Safety baseline: with core frozen, **Clear everything** clears every output within 1 frame (SAFE-03); 3 crashes in 60 seconds start safe mode with its b… — evidence: SAFE-03 `clear_everything_works_with_core_frozen`; SAFE-07 `three_crashes_in_a_minute_start_safe_mode` (banner: `LayoutTests`), `restart_normally_leaves_safe_mode_and_keeps_the_screens`; SAFE-06 `LiveGuardTests`, `UpdateTests`, `updates_never_install_while_something_is_on_the_screens`
- [x] **M1-A15** (auto) **Report a problem** produces a bundle that has logs and versions but no seeded secrets and no user name. — evidence: `a_bundle_has_logs_and_versions_but_no_secrets_or_user_name`, `report_a_problem_saves_a_bundle_with_the_version`, `diagnostics_shows_the_engine_and_writes_a_benchmark_report`
- [x] **M1-A16** (auto) The session opened its PR with `tools/open-pr.sh`, the description follows the template, and `tools/ci.sh status` reads the PR's CI results. — evidence: PR opened with `tools/open-pr.sh` from the template; `tools/ci.sh status --wait` read its results

### M2. Library, songs, scripture and the help system

**Work packages**

- [x] **M2.1** Data model, store and migrations — evidence: commit `M2.1`; `proto/selah/v1/{library,service}.proto` + TS/Rust/C# generated contracts (`buf lint`, `buf breaking --against .git#tag=m1`, `check-generated` pass); `selah-core::library` (UUIDv7, SQLite STRICT/WAL/FULL/trusted_schema=0, checksummed append-only migrations + `schema-v1.db` fixture, sealed typed repositories, BLAKE3 `MediaStore`, 3-tier policy settings); `selah-core` library integration tests (17) + unit test; `docs/specs/data-model.md`, ADR-027, `docs/SECURITY.md`; full `tools/verify.ps1` gate green on Windows; ledger updated (rust 139, total 249)
- [x] **M2.2** Command bus, op log, undo/redo and autosave — evidence: commit `M2.2`; `proto/selah/v1/{envelope,library,settings}.proto` + TS/Rust/C# generated contracts (`M2CommandQueryAndEventMessagesRoundTrip`, `web/contracts` M2 round-trip test); migration `002_command_bus.sql` (`undo_entries`, `command_idempotency`, `editor_buffers.doc_type`); SEC-14 `permissions::authorize` extended for all 27 commands + `unauthorized_library_commands_are_refused_before_touching_database`; transactional command execution, inverse capture, per-document & global undo groups, idempotency replay/conflict checks, autosave `editor_buffers`, `help_feedback` persistence, `LibraryChanged` / `UndoStateChanged` broadcasts, `selah-core-host` `SELAH_LIBRARY_DIR` wiring; `selah-core` `ops::tests` (8 incl. **M2-A2**), `service::tests` (3 new library/buffer/policy tests), `selah-testkit` `library_crash` (**M2-A3**); errors `SEL-LIB-0001`–`0004` + 5 `troubleshooting` KB articles (`status: review`, lint 0 errors); `docs/specs/{command-bus,data-model}.md`; full `tools/verify.ps1` gate green on Windows; ledger updated (rust 151, dotnet 21, web 4, kb 12, total 268)
- [x] **M2.3** Search — evidence: commit `M2.3`; `proto/selah/v1/{envelope,library}.proto` + TS/Rust/C# generated contracts (`SearchSongs`/`SearchMedia`/`SearchScripture`/`SearchLibrary` queries, C# `M2CommandQueryAndEventMessagesRoundTrip`, `web/contracts` M2 search round-trip test); migration `003_search.sql` (FTS5 `unicode61 remove_diacritics 2` + `trigram` indexes for songs/media + `search_synonyms` + `search_meta`, backfilled on `schema-v1.db` upgrade); `selah-core::library::search` (diacritic/orthography folding, Damerau-Levenshtein typo tolerance, bilingual synonyms, weighted BM25+field ranking, per-translation `bible_meta`/FTS5/trigram query engine + `biblical` path confinement, `write_bible_database` hook for M2.5, SEC-03 query validation & bounds); `crates/selah-core/tests/search.rs` (10) + `search.rs` unit tests (3) + `service::tests::search_queries_round_trip_through_core_service_and_map_errors`; `selah-bench search --check` on 10,000 songs (`p50 = 5.19 ms`, `p95 = 17.07 ms <= 50 ms`, `amzing_grace_p95 = 14.68 ms`, `correctness_pass = true`) wired into gate stage 6; `docs/specs/{search,command-bus,data-model}.md`; headless engine IPC startup fix (`selah-engine/src/engine.rs`) verified 3× in `selah-testkit::continuity`; full `tools/verify.ps1` gate green on Windows; ledger updated (rust 164, dotnet 22, web 5, total 283)
- [x] **M2.4** Songs: paste-to-sections, chords and slide splitting — evidence: commit `M2.4`; `proto/selah/v1/{envelope,library}.proto` + TS/Rust/C# generated contracts (`ParsePastedSong`, `TransposeSong`, `SplitSongSlides`, `SongPasteFormat`, `CopyrightPlacement`, `AccidentalSpelling`, `GeneratedSlide`, `GeneratedSongSlides`; C# `ContractTests`, `web/contracts` round-trip test); `selah-core::songs` (`parse` across Plain/Markers/ChordsOverLyrics/ChordPro with SEC-03 size/line/section/nesting/chord bounds + `fuzz_chordpro`, `chords` key-aware 12-semitone + slash-chord transposition, `sections` DESIGN-SYSTEM colours & two-pass non-overlapping hotkeys, `split` rule-based `max_lines` + title/blank/copyright/translation slides); `fuzz/fuzz_targets/chordpro.rs` + `songs_fuzz_smoke` (`SELAH_FUZZ_SECONDS=5` green in stage 6); `service::tests::song_parse_transpose_and_split_queries_round_trip_through_core_service`; `tests/fixtures/songs/` (25 public-domain hymns × 4 formats + `PROVENANCE.md`) verified 100% by `selah-core/tests/songs.rs`; `docs/specs/songs.md`; full `tools/verify.ps1` gate green on Windows; ledger updated (rust 183, dotnet 22, web 6, total 303)
- [x] **M2.5** Scripture — evidence: commit `M2.5`; `content/bibles/{kjv,web}.tsv.gz` + `PROVENANCE.md` (H-06 public-domain KJV `31,102` & WEB `31,098` verses, SHA-256 verified, background materialisation on `selah-core-host` startup); `proto/selah/v1/{envelope,library}.proto` + TS/Rust/C# generated contracts (`ImportBible`, `DeleteBible`, `ListBibles`, `ParseScriptureReference`, `GetScripturePassages`, `SplitScriptureSlides`; C# `ContractTests`, `web/contracts` round-trip test); `selah-core::scripture` (`books` 66 canonical books × 6 languages, `reference` parser across `en`/`es`/`pt`/`fr`/`ko`/`yo`, bounded `import` for OSIS/USFM/Zefania via `quick-xml` 0.41 with `DocType`/symlink/depth/size limits, `store`, `split`); `fuzz/fuzz_targets/bible_{osis,usfm,zefania}.rs` + `scripture_fuzz_smoke` (`SELAH_FUZZ_SECONDS=5` green in stage 6); `permissions::authorize` SEC-14 matrix updated for all 29 commands + `service::tests::scripture_commands_and_queries_round_trip_through_core_service`; `docs/specs/scripture.md`; full `tools/verify.ps1` gate green on Windows; ledger updated (rust 193, dotnet 23, web 7, total 315)
- [x] **M2.6** Media bin and library exchange — evidence: commit `M2.6`; `proto/selah/v1/{envelope,library}.proto` + TS/Rust/C# generated contracts (`ImportMedia`, `GetMediaThumbnail`, `ExportSelahBundle`, `ImportSelahBundle`, `CreateLibraryBackup`, `RestoreLibraryBackup`; C# `ContractTests`, `web/contracts` round-trip test); `selah-media` (`probe_media`, `extract_image_thumbnail`, decode-only video thumbnail via temporary `selah-media-probe` subprocess under SEC-04 pixel/stream bounds, `selah-render::png` encoder); `selah-core::library::archive` (bounded ZIP/Deflate reader with entry count, size, ratio & path-traversal limits) & `library::exchange` (`.selah` bundle export/import, deterministic RFC 8785-style canonical `manifest.json` validated against `formats/selah-bundle.schema.json`, full SQLite online-backup + BLAKE3 media blob + Bible zip backup/restore); `fuzz/fuzz_targets/selah_bundle.rs` + `exchange_fuzz_smoke` (`SELAH_FUZZ_SECONDS=5` green in stage 6); `selah-core/tests/media_exchange.rs` + `tests/tools/test_bundle_schema.py` + `permissions::authorize` extended for all 33 commands; continuity orphan/seeding reliability fixes in `selah-watchdog`, `selah-testkit`, `CoreConnection.cs`, `selah-core-host`; `docs/specs/media-and-exchange.md`; full `tools/verify.ps1` gate green on Windows; ledger updated (rust 206, dotnet 24, web 8, total 330)
- [x] **M2.7** Library and song editor UI — evidence: commit `M2.7`; `ILibraryService` + `CoreConnection` IPC (`List`/`Search`/`Get`/`Save`/`Delete` songs, `ParsePastedSong`, `TransposeSong`, `SplitSongSlides`, `EditorBuffer` autosave/restore, per-doc & global `Undo`/`Redo`, `List`/`Import`/`Delete`/`Lookup`/`Search`/`SplitScriptureSlides`, `List`/`Search`/`Import`/`Delete`/`GetMediaThumbnail` media, `.selah` bundles & backups, `LibraryChanged` & `UndoStateChanged` subscription); `LibraryView` (Songs, Media bin with decoded PNG thumbnails, Bibles & Scripture lookup/search/splitting/import, bundle/backup exchange, responsive 2-col/stacked at 200%); `SongEditorView` (4-step `Paste → Sections → Arrange → Review` flow matching prototype 03, live slide preview & targets `16:9`/`LED 32:9`/`Stage`/`Stream`, Rune-safe `[Chord]` editor, transpose, inspector tabs, responsive 3-col/stacked at 200%); `CommandPaletteView` (`Ctrl/Cmd+K` below toolbar over commands/songs/scripture/media/help without obscuring `Clear everything`); `MainWindow` live pill relocated to toolbar across all modes; `content/i18n/en/{shell,library}.ftl` (`tools/lint-strings.py` OK); `Selah.UI.Tests` `LibraryAndEditorTests` (+18 cases across 100%/200%, Light/Booth, `en`/`en-XA`, 88 UI tests total); full `tools/verify.ps1` gate green on Windows; ledger updated (ui 88, total 348)
- [x] **M2.8** Contextual help components — evidence: commit `M2.8`; `content/help/anchors.yaml` (20 M2 UI anchors), `tools/gen_help_anchors.py` -> `app/Selah.UI/Services/HelpAnchors.g.cs` checked by `tools/check-generated.py` & `tests/tools/test_help_anchors.py`; `HelpProperties` (`Help.Anchor` attached property inherited by `SettingsWindow` & `DiagnosticsWindow` from owner); contextual help controls (`HintText`, `InfoButton` with `L1` popover & guide button, `HelpPanelView` F1 panel with "For this screen", search, native AST article rendering, Yes/No `RecordFeedback` persistence, "Open in window", `WhatsThisOverlay` Shift+F1 mode with Esc/Done dismissal without triggering controls, `EmptyState` across Songs/Media/Bibles, `ErrorAlert` with "How to fix" & catalog lookup); `selah-kb` & `selah-kb-lint` coverage checks 1–6 (KNOWLEDGE-BASE §7.2) in `crates/selah-kb/src/{content,lint}.rs` + `crates/selah-kb-lint/tests/lint.rs` (23 articles, 0 errors across checks 1–6); `app/Selah.UI.Tests/ContextualHelpTests.cs` (10 tests: F1 across 3 screens at 100/200% Light/Booth, search/article/feedback round-trip, What's this non-triggering inspect & Esc, live-safety F12 bypass & non-overlap across 100/200% Light/Booth `en`/`en-XA`, `InfoButton`/`EmptyState`/`ErrorAlert`/dialog anchor inheritance); full `tools/verify.ps1` gate green on Windows; ledger updated (rust 209, ui 98, kb 23, total 372)
- [x] **M2.9** Knowledge base content for M2 — evidence: commit `M2.9`; expanded `content/kb/en/` from 23 to 37 articles (`songs/` 7, `scripture/` 5, `media/` 2, `library/` 3, `glossary/` 4, `getting-started/` 8, `troubleshooting/` 8), all at `status: review` (`reviewed_by: ""`, H-18 open); `selah-kb-lint` passes 0 errors (41 existing warnings) across rules R1–R18 and coverage checks 1–6 with `PROGRESS.md` M2 set to `Done, H-checks pending` (`planned.yaml` has 0 M2 items); `selah-kb build` writes 37 articles to `target/kb/kb.db`; `selah-kb-lint` integration tests (10) pass; `library_fixture --m2` generated `tests/fixtures/db/m2.db` covering every M2 repository, policy precedence, editor buffer, feedback, synonym, ops, idempotency and undo/redo stack; `crates/selah-core/tests/migrations.rs` (2) and Stage 10 `migrations` verify both `schema-v1.db` and `m2.db`; updated `docs/SECURITY.md`, `docs/specs/README.md`, `CHANGELOG.md`, `tests/fixtures/db/README.md`; full `tools/verify.ps1` gate green on Windows; ledger updated (`rust: 211`, `dotnet: 24`, `web: 8`, `golden: 4`, `ui: 98`, `e2e: 6`, `kb: 37`, `total: 388`)

**Acceptance criteria**

- [x] **M2-A1** (auto) The 25 curated hymns, pasted in 4 formats (plain, markers, chords over lyrics, ChordPro), produce exactly the expected section JSON (100%). — evidence: `selah-core/tests/songs.rs::curated_25_hymns_match_expected_across_all_four_paste_formats` (all 25 directories in `tests/fixtures/songs/`, 100 fixture checks across `plain.txt`, `markers.txt`, `chords.txt`, `song.cho` compared 100% against `expected.json` for arrangement, labels, kinds, colours, hotkeys, Unicode lyrics, aligned chord offsets, metadata and `Library` persistence)
- [x] **M2-A2** (auto) Undo/redo property test: 10,000 random op sequences. — evidence: `selah-core` `library::ops::tests::m2_a2_ten_thousand_random_op_sequences_round_trip_undo_and_redo` (10,000 random sequences across `Song`, `Template`, `Presentation`, and `Tag` with create/update/delete and contiguous multi-op undo groups; undoing all ops restores empty baseline and redoing restores final state)
- [x] **M2-A3** (auto) Killing core during 1,000 writes leaves `PRAGMA integrity_check` = ok, loses no committed op, and the unsaved editor buffer is restored. — evidence: `selah-testkit` `library_crash::killing_core_during_one_thousand_writes_preserves_integrity_ops_and_editor_buffer` (kills real `selah-core` mid-flight during 4-way concurrent writes over 1,000 committed songs, recovers via `selah-watchdog` in < 5 s, restores unsaved `editor_buffers`, verifies paginated `ListSongs` + `ListOps` completeness and `PRAGMA integrity_check = ok`)
- [x] **M2-A4** (auto) Song search p95 ≤ 50 ms on 10,000 songs. "amzing grace" finds "Amazing Grace"; synonym tests pass. — evidence: `selah-bench search --check --json target/verify-report/bench-search.json` (10,000 songs, 200 iterations: `p50 = 5.19 ms`, `p95 = 17.07 ms`, `amzing_grace_p95 = 14.68 ms`, `correctness_pass = true`, gate stage 6) + `selah-core/tests/search.rs` (`song_search_handles_typos_short_transpositions_and_avoids_false_positives`, `song_search_pagination_and_synonyms_work`, `multilingual_diacritics_fold_for_search_and_typos`)
- [x] **M2-A5** (auto) At least 300 scripture-reference test cases pass across 6 languages; OSIS, USFM and Zefania samples import correctly. — evidence: `selah-core/tests/scripture.rs` (`canonical_book_table_covers_66_books_and_six_languages_cleanly`, `m2_a5_at_least_300_scripture_references_parse_across_six_languages` covering 66×6 book aliases + syntax variations across `en`/`es`/`pt`/`fr`/`ko`/`yo`, `m2_a5_osis_usfm_and_zefania_importers_parse_and_enforce_security_limits`, `bundled_kjv_and_web_bibles_materialize_query_search_and_delete_without_resurrection`)
- [x] **M2-A6** (auto) `.selah` export → import → export produces identical canonical JSON; backup and restore round trip. — evidence: `selah-core/tests/media_exchange.rs` (`m2_a6_selah_export_import_export_is_identical_and_supports_overwrite_and_undo`, `m2_a6_full_library_backup_and_restore_round_trips_database_bibles_and_media`, `import_media_through_worker_caches_thumbnails_and_supports_undo_redo`) + `tests/tools/test_bundle_schema.py` (validates against `formats/selah-bundle.schema.json`) + `service::tests::media_exchange_and_backup_commands_round_trip_through_core_service`
- [x] **M2-A7** (auto) Headless UI flows pass at 100% and 200% in both appearances: paste a song; edit a section; add scripture; import media; Ctrl+K finds a song, a command a… — evidence: `Selah.UI.Tests.LibraryAndEditorTests` (`Paste_a_song_detects_sections_and_generates_slides`, `Edit_section_arrange_transpose_save_and_undo`, `Add_scripture_lookup_search_and_split_slides`, `Import_media_loads_thumbnail_bitmap_in_media_bin`, `Ctrl_K_finds_song_command_and_help_article`, `Workspace_modes_and_palette_preserve_all_critical_labels_while_live` across 100%/200%, Light/Booth, `en`/`en-XA` — 88 UI tests total green in stage 8)
- [x] **M2-A8** (auto) Help coverage tests pass. — evidence: `cargo run -p selah-kb-lint` (0 errors across coverage checks 1–6: anchors ↔ UI bidirectional check, anchor context coverage, `errors.yaml` fix articles & keys, `settings.yaml` hints & articles, tour steps, bold FTL labels) + `crates/selah-kb-lint/tests/lint.rs` (10 integration tests incl. `coverage_detects_unused_and_undeclared_ui_anchors`, `coverage_detects_uncovered_anchors_settings_errors_and_tours`, `context_load_scans_axaml_cs_and_tour_files`, `kb_db_builds_and_answers_searches`) + `Selah.UI.Tests.ContextualHelpTests` (`F1_opens_help_panel_with_context_across_three_screens`, `Whats_this_inspects_without_firing_live_actions_and_esc_dismisses`, `Help_panel_searches_renders_native_article_and_records_feedback`, `Live_safety_f12_bypasses_help_search_and_help_never_covers_live_controls`, `InfoButton_EmptyState_and_ErrorAlert_open_contextual_guides`)
- [x] **M2-A9** (auto) `tests/fixtures/db/m2.db` is saved, and every M1 test still passes (ledger). — evidence: `tests/fixtures/db/m2.db` generated via `cargo run -p selah-core --example library_fixture -- --m2 tests/fixtures/db/m2.db` with `schema-v1.db` untouched; `crates/selah-core/tests/migrations.rs` (2 tests) & gate Stage 10 `migrations` verify both historical fixtures open, migrate cleanly, pass integrity checks, preserve search/buffers/feedback/ops and run undo/redo; full `tools/verify.ps1` green with `tests/ledger.json` strictly non-decreasing (`total: 231 → 388`)
- [ ] **M2-A10** (H) H-06 Bible rights are confirmed before more translations are bundled. — evidence: H-06 remains open: only public-domain KJV (31,102 verses) and WEB (31,098 verses) are bundled in `content/bibles/` (`content/bibles/PROVENANCE.md`); no additional translations bundled pending rights clearance
- [x] **M2-A11** (auto) Every M2 parser passes its fuzz smoke run; a crafted bundle with `..` paths or oversized entries is refused; every new command is in the permission matr… — evidence: `fuzz/fuzz_targets/{chordpro,bible_osis,bible_usfm,bible_zefania,selah_bundle}.rs` (`songs_fuzz_smoke`, `scripture_fuzz_smoke`, `exchange_fuzz_smoke` with `SELAH_FUZZ_SECONDS=5` green in stage 6); `m2_a11_crafted_zip_bundles_and_corrupt_backups_fail_closed` & `media_import_refuses_urls_protocols_and_dimension_bombs` (traversal `..`, deflate zip bomb, duplicate/oversized entries, symlinks/URLs, pixel limit); `permissions::tests::permission_matrix_covers_every_origin_and_command` (all 34 M1+M2 commands)

### M3. Render engine, templates and the slide editor

**Work packages**

- [x] **M3.1** Scene graph, layers and frame scheduler — evidence: commit `M3.1`; `proto/selah/v1/{render,envelope}.proto` (`OutputConfig`, `LayerSpec`, `OutputScene`, `PreloadHint`, `SetPreviewScene`, `PreloadItems`, `SetOutputConfigs`, `GetOutputConfigs`, `OutputFrameStatus`; `ContractTests` 25/25, `web/contracts` 9/9, `buf breaking --against .git#tag=m2` & `check-generated` pass); `selah-render::scene` canonical 5-layer normalization (`VideoInput`, `Media`, `Slide`, `Props`, `Messages`), `OverlayCache`, `prerasterise_hints`, `GpuCompositor`/`CpuCompositor` multi-output independent targets (`crates/selah-render/tests/layers.rs`: `canonical_five_layer_stack_renders_in_z_order_and_matches_cpu_gpu`, `multiple_outputs_render_at_independent_resolutions_simultaneously`, `prerasterise_hints_warm_caches_for_upcoming_slides`); `selah-engine::scheduler::FrameScheduler` vsync cadence & multi-output `Engine` + `selah-core` `SetPreviewScene`/`PreloadItems`/`SetOutputConfigs`/`GetOutputConfigs` (`service::tests`, `permissions::authorize` SEC-14 matrix, `selah-testkit/tests/multi_output.rs` `multi_output_layer_scenes_present_independently_and_clear_all_on_emergency`); full `tools/verify.ps1` green on Windows; ledger updated (`rust: 222`, `dotnet: 25`, `web: 9`, `total: 401`)
- [x] **M3.2** Templates and text styling — evidence: commit `M3.2`; `crates/selah-render/src/template.rs` (`TemplateStyle`, `RoleSlots` `body`/`reference`/`title`/`footer`/`copyright` alias/`chords`/`translation`, `AspectVariantStyle` resolution, `SlotTextStyle` font/size/weight/color/outline/shadow/background_box/line_height/letter_spacing/H+V align/safe_area/min_size, SEC-03 bounds, M2 shorthand compat, built-in Sanctuary/Warm Serif/Lower Third/Stage Monitor presets); `crates/selah-render/src/text.rs` (`ALL_FONTS` all 7 bundled OFL variable fonts + `wght` `100..=900` stops + Skia HarfBuzz slot layout/outline/shadow/card box/chord placement + preserved M1 `golden_text` 4/4); `crates/selah-render/src/scene.rs` (`rasterise_slide_text` + `prerasterise_hints`); `tools/{package,test-installer}.ps1` all 7 font families; `crates/selah-render/tests/templates.rs` (4/4 tests + CPU/GPU parity); unit tests 3/3 in `template.rs` & 3/3 in `text.rs`; full `tools/verify.ps1` green (`rust: 230`, `total: 409`)
- [x] **M3.3** Metric fit: words that fit every screen — evidence: commit `M3.3`; `proto/selah/v1/library.proto` + TS/Rust/C# generated contracts (`FitTargetOutput`, `OutputFitReport`, `OutputFittedSongSlides`, `OutputFittedScriptureSlides`; `ContractTests` 26/26, `web/contracts` 10/10, `buf breaking --against .git#tag=m2` & `check-generated` pass); `crates/selah-render/src/fit.rs` (Skia measurement shrink-to-`min_size` + clean word/line re-split, translation/chord/copyright safe-area guard, aligned stroke/shadow layout widths in `text.rs`, per-output `OutputFitReport`, 4/4 unit tests) & `selah-core` (`SplitSongSlides` / `SplitScriptureSlides` template resolution + per-output fit reports + backward-compatible fallback, `crates/selah-core/tests/metric_fit.rs` 2/2); full `tools/verify.ps1` gate green on Windows (`rust: 236`, `dotnet: 26`, `web: 10`, `total: 417`)
- [x] **M3.4** Backgrounds and media playback — evidence: commit `M3.4`; `crates/selah-render/src/media_bg.rs` & `blur.{rs,wgsl}` (content-addressed BLAKE3 verified image loading under `SEC-04` bounds, solid/gradient backdrops, fit/fill/stretch, separable Gaussian blur & opacity on CPU/GPU), `video.{rs,wgsl}` (`VideoLayer` multi-stream NV12 GPU sampler + cached uniform bind groups, fit/fill/stretch/opacity/blur parity); `crates/selah-media/src/{lib,audio,player}.rs` (`MediaPlayer` A/V sync via `AudioClock` master clock on default CPAL/pump sink, seamless standby loop pre-opening, in/out points, speed `0.1..=4.0`, ITU-R BS.1770 4x oversampled `-1 dBTP` master limiter & smooth 25 ms fade-out for `SAFE-05`, `SEC-04` allowlists/URL refusal, 10 unit tests in `audio` + `player`); `crates/selah-engine/src/{engine,media_playback,scheduler}.rs` (synced media loops, emergency preemption `emergency_pending`, idle frame skipping); `crates/selah-render/tests/layers.rs` (`image_background_supports_fit_fill_stretch_blur_and_opacity_with_cpu_gpu_parity`, `video_layer_supports_fit_blur_opacity_z_order_and_cpu_gpu_parity`, `background_image_loader_refuses_corrupt_hash_and_oversize_bomb`), `crates/selah-testkit/tests/backgrounds.rs` (`backgrounds_image_video_and_emergency_clear_end_to_end` `SAFE-03`/`SAFE-05`/`SEC-04`); `tests/fixtures/media/committed/flash-beep-360p60.mp4`; full `tools/verify.ps1` green on Windows (`rust: 251`, `total: 432`)
- [x] **M3.5** Transitions and effects foundation — evidence: commit `M3.5`; `proto/selah/v1/{render,envelope,library}.proto` + TS/Rust/C# generated contracts (`TransitionSpec`, `TransitionKind`, `TransitionDirection`, `TransitionEasing`, `EffectSpec`, `EffectKind`, `AnalyzeFlashRisk`, `FlashRiskReport`, `FrameShown.flash_warning`, `OutputFrameStatus.flash_warning`, `OutputFitReport.flash_warning`; `ContractTests` 27/27, `web/contracts` 11/11, `buf breaking --against .git#tag=m2` & `check-generated` pass); `crates/selah-render/src/transitions.{rs,wgsl}` & `gpu.rs`/`cpu.rs` (8 transitions `Cut`/`Fade`/`Dissolve`/`FadeThroughBlack`/`Push`/`Wipe`/`Zoom`/`Slide` × 4 directions × 4 easings with GPU/CPU parity, scene & per-layer transition animation); `crates/selah-render/src/effects.{rs,wgsl}` (closed 6 built-in effects `Blur`/`ColourAdjust`/`Vignette`/`BackgroundBlur`/`ColourKey`/`Mask` under `SEC-03` bounds, refusing custom WGSL/`shader`/`script`/`entry_point` & >8 stages / >16 KiB params); `crates/selah-render/src/flash.rs` (`FlashAnalyzer` + `analyze_media_frames` + `analyze_transition_risk` WCAG 2.3.1 general & saturated-red flash guard, `SAFE-04`); `crates/selah-core/src/{service,library/exchange}.rs` & `crates/selah-media/src/{lib,bin/selah_media_worker}.rs` (offline flash scan on `ImportMedia` with `Flashing` tag + `.selah` round-trip, `AnalyzeFlashRisk`, fit-report propagation); `crates/selah-engine/src/engine.rs` (interpolated transition scheduling & emergency clear cancellation, live `FrameShown.flash_warning`); `crates/selah-core/tests/flash_and_shaders.rs` (3/3), `crates/selah-testkit/tests/transitions_effects_flash.rs` (end-to-end `Stack` test `SAFE-03`/`SAFE-04`/`SEC-04`), `crates/selah-render` unit tests (9 in `transitions`+`effects`+`flash`); `tests/fixtures/media/committed/wcag-flash-360p60.mp4`; full `tools/verify.ps1` green on Windows (`rust: 264`, `dotnet: 27`, `web: 11`, `total: 447`)
- [x] **M3.6** Thumbnails and preview frames — evidence: commit `M3.6`; `proto/selah/v1/{render,envelope}.proto` + TS/Rust/C# generated contracts (`RenderSceneThumbnail`, `SceneThumbnail`; `ContractTests` 28/28, `web/contracts` 12/12, `buf breaking --against .git#tag=m2` & `check-generated` pass); `crates/selah-core/src/thumbnails.rs` (canonical BLAKE3 keyed offscreen scene thumbnail renderer via `CpuCompositor`, `encode_rgba_thumbnail` premultiplied→straight conversion in `selah-render/src/png.rs`, video poster extraction via `selah-media` & `selah-core/src/thumbnails.rs` `load_or_extract_video_poster`, `flash_warning` propagation, bounded LRU eviction `MAX_SCENE_THUMB_FILES`, corrupt-cache self-healing, SEC-03/SEC-04 input validation) & `service.rs` `RenderSceneThumbnail` query handler; `crates/selah-engine/src/engine.rs` & `outputs.rs` dual Program (`{ns}-preview-{n}.ring`) + Staged Preview (`{ns}-stage-0.ring`) shared-memory preview rings (`crates/selah-testkit/tests/multi_output.rs::multi_output_layer_scenes_present_independently_and_clear_all_on_emergency` verifying isolated `stage-0.ring` vs `preview-0.ring` updates and `FrameShown { is_preview }`); `app/Selah.UI` (`CoreConnection.cs`, `MainWindow.axaml(.cs)`, `MainViewModel.cs`, `LibraryView{,Model}`, `SongEditorView{,Model}`) dual live Program + Staged Next preview pane, slide-card offscreen scene thumbnails across Songs, Bibles & Song Editor targets, `Stage` preview button, and flash-warning badges; `crates/selah-core/tests/thumbnails.rs` (4/4 incl. GPU `AdapterChoice::Software` parity, corrupt-cache self-healing, bounded eviction, video poster + `SAFE-04`, malformed/unsafe rejection); `Selah.UI.Tests.LibraryAndEditorTests::Dual_preview_and_slide_staging_render_thumbnails_and_preserve_critical_labels` (103/103 UI tests green across 100%/200%, Light/Booth, `en`/`en-XA`); `content/i18n/en/{shell,library}.ftl` (`lint-strings.py` OK); full `tools/verify.ps1` gate green on Windows (`rust: 270`, `dotnet: 28`, `web: 12`, `ui: 103`, `total: 460`)
- [x] **M3.7** Template editor and slide editor UI — evidence: commit `M3.7`; `selah-render::slide_elements` (`SlideElementsDocument` v1 schema, text/clock/timer `{clock}` & `{timer}` dynamic tokens + `format_dynamic_tokens`, shape Rect/RoundedRect/Ellipse/Line, image/video elements, locked & multi-element align/group bounds, strict `SEC-03`/`SEC-04` validation, `crates/selah-render/tests/elements.rs` 3/3 CPU/GPU parity & live clock cache busting); `selah-core` policy precedence (`templates.default.song`, `templates.default.scripture`, per-output `TemplateId`) and `thumbnails.rs` custom element rendering; `app/Selah.UI` `TemplateDocumentModel.cs` & `TemplateEditorView{,Model}` (4 starter looks, role slots, fonts/weights/outlines/shadows/card boxes, aspect shapes, safe-area margins, song/scripture/screen assignment, live thumbnail preview) + `SlideElementsModel.cs` & `SlideEditorView{,Model}` (custom deck & slide editor, Text/Clock/Timer/Shape/Image/Video elements, align/group/ungroup/forward/backward/lock, background/transition/effect chain, `SAFE-04` flash acknowledgement guard blocking unconfirmed staging, per-document undo/redo); `content/i18n/en/{shell,library,templates,slides}.ftl` (`tools/lint-strings.py` OK); `content/help/anchors.yaml` + regenerated `HelpAnchors.g.cs` (24 anchors) & 4 initial M3 KB articles (`templates.{create-a-template,assign-templates}`, `slides.{create-a-custom-slide,style-and-animate-slides}`, `selah-kb-lint` 41 articles / 0 errors); `Selah.UI.Tests` 118/118 (`Template_editor_looks_assignment_and_song_fit_flow`, `Slide_editor_create_align_undo_and_redo_flow`, `Slide_editor_flash_warning_guard_blocks_unconfirmed_stage`, `ContextualHelpTests` 10/10, `LayoutTests` 12/12); `Selah.Client.Tests` 28/28; full `tools/verify.ps1` green on Windows (`rust: 275`, `dotnet: 28`, `web: 12`, `golden: 4`, `ui: 118`, `e2e: 6`, `kb: 41`, `total: 484`)
- [x] **M3.8** Golden-frame suite and knowledge base — evidence: commit `M3.8`; `crates/selah-testkit/tests/golden_m3.rs` (6 tests comparing `55` new reference PNGs in `tests/golden/reference/m3-*.png` — 9 template style/typography/aspect frames under `TOLERANCE_STYLE`, 40 transition frames across all 8 transitions × 5 steps, and 6 built-in effect frames under `TOLERANCE_SHADER`, plus GPU `AdapterChoice::Software` vs `CpuCompositor` parity; fixed chorded-body `background_box`/`outline`/`shadow` painting in `crates/selah-render/src/text.rs`; reviewed by eye & recorded in `tests/golden/CHANGES.md`; Stage 7 `golden_text` 4/4 + `golden_m3` 6/6); `crates/selah-bench/src/m3.rs` + `selah-bench m3 --check` in Gate Stage 6 + `DiagnosticsReport` (`m3_p99_ms`, `m3_pass`) wired across `proto/selah/v1/envelope.proto`, `selah-core/src/{diagnostics,service}.rs`, `selah-testkit/tests/support.rs`, `DiagnosticsViewModel.cs`, and `DiagnosticsTests.cs`; expanded `content/kb/en/` from 41 to 57 articles (+16 M3 articles across `templates/`, `slides/`, `glossary/`, `troubleshooting/`, all `status: review` with `reviewed_by: ""` for H-18; `selah-kb-lint` 57 articles / 0 errors, `selah-kb build` clean, `selah-kb-lint` integration suite 10/10); generated `tests/fixtures/db/m3.db` via `library_fixture --m3` (leaving `schema-v1.db` and `m2.db` untouched) + verified in `crates/selah-core/tests/migrations.rs` (3/3) and Stage 10; wrote `docs/specs/{slide-generation,render-pipeline}.md` and updated `docs/specs/{command-bus,support-and-diagnostics,README}.md`, `CHANGELOG.md`, and `docs/SECURITY.md` §3+§9; full `tools/verify.ps1` gate green on Windows; ledger updated (`rust: 279`, `dotnet: 28`, `web: 12`, `golden: 10`, `ui: 118`, `e2e: 6`, `kb: 57`, `total: 510`)

**Acceptance criteria**

- [x] **M3-A1** (auto) Golden frames pass for every template style feature, transition step and effect. — evidence: `crates/selah-testkit/tests/golden_m3.rs` (`style_roles_decorations_alignments_and_safe_area_match_golden`, `style_all_seven_fonts_and_weights_match_golden`, `style_aspect_ratio_variants_match_golden`, `all_eight_transitions_at_five_steps_match_golden`, `all_six_built_in_effects_match_golden`, `m3_golden_gpu_and_cpu_parity_agree`) + `golden_text` (`4 + 6 = 10` golden suite tests verifying all 59 reference PNGs under `tests/golden/reference/`, Stage 7 green)
- [x] **M3-A2** (auto) Fit property test: 5,000 random lyric sets × 3 outputs (16:9, 32:9 LED, 4:3). — evidence: `crates/selah-core/tests/metric_fit.rs::m3_a2_property_five_thousand_random_lyric_sets_across_three_outputs` (5,000 pseudorandom multilingual/chorded/translated lyric sets across Sanctuary 16:9 `1920×1080`, LED 32:9 `3840×1080` max 2 lines, and Classic 4:3 `1440×1080` — 15,000 output splits asserting zero safe-area overflow, `applied_size >= min_size`, non-shrink-before-split honesty, accurate `OutputFitReport` counts/warnings, plus sampled full-slide pixel rasterisation boundary assertions inside `safe_rect`)
- [x] **M3-A3** (auto) Thumbnails match the downscaled real output (pixel difference under the threshold). — evidence: `crates/selah-core/tests/thumbnails.rs::scene_thumbnail_matches_gpu_compositor_for_layers_and_effects` (compares offscreen `RenderSceneThumbnail` 320×180 PNG against downscaled `GpuCompositor` `1280×720` render across image backdrop, `ColourAdjust`, `Slide` typography, `Props` card, and `Vignette`, asserting mean channel abs diff `<= 3.0`), `scene_thumbnail_caches_canonically_heals_corrupt_files_and_evicts_lru`, `scene_thumbnail_renders_video_poster_and_propagates_flash_warning`, `scene_thumbnail_rejects_malformed_and_unsafe_inputs`
- [x] **M3-A4** (auto) A 1080p loop has no frame gap at the loop point, and A/V sync is within ±1 frame (flash/beep test clip). — evidence: `selah-media::player::tests::loop_1080p_has_zero_frame_gap_at_loop_point` (`h264-1080p60.mp4`, 0..120 sequence numbers with 0 skips/repeats and exact `1/60 s` seam delta across laps), `flash_and_beep_stay_synchronized_within_one_frame_at_half_one_and_two_x_speed` (`flash-beep-360p60.mp4` at `0.5x`/`1.0x`/`2.0x` within ±1 frame of audio beep), `in_and_out_points_trim_and_loop_cleanly`, `clear_fade_out_silences_and_finishes_player_cleanly`, `software_pump_drives_clock_and_frames_asynchronously`
- [x] **M3-A5** (auto) Slide editor UI flow: create a custom slide with text, an image and a shape, align them, undo and redo. — evidence: `Selah.UI.Tests.LibraryAndEditorTests::Slide_editor_create_align_undo_and_redo_flow` (creates slide deck, adds text, shape, and image elements, selects all and aligns horizontally, verifies undo restores pre-align X position and redo re-applies aligned X, saves presentation, verifies live thumbnail preview & zero clipped critical labels across `ModesMatrix` 100/200%, Light/Booth, `en`/`en-XA`) + `Template_editor_looks_assignment_and_song_fit_flow` + `Slide_editor_flash_warning_guard_blocks_unconfirmed_stage` + `crates/selah-render/tests/elements.rs` (3/3)
- [x] **M3-A6** (auto) (ci-win) Bench recorded in the cloud and the Windows job: 2 × 1080p outputs with a 4K video background, p99 frame time. — evidence: `selah-bench m3 --iterations 10 --check --json target/verify-report/bench-m3.json` wired into Gate Stage 6 (WARP software adapter CI measurement with 4K HEVC clip: `p50 = 1280.63 ms`, `p99 = 1311.46 ms <= SOFTWARE_CI_CEILING_MS = 5000.0 ms`; NVIDIA GeForce MX350 hardware GPU DX12 measurement on this PC with 4K HEVC clip: `p50 = 12.95 ms`, `p95 = 14.53 ms`, `p99 = 14.86 ms` against `OUTPUT_P99_BUDGET_MS = 8.0 ms`) + `selah_core::diagnostics` & `selah-testkit::support::diagnostics_shows_the_engine_and_writes_a_benchmark_report` + `DiagnosticsTests` (17/17); cloud session 2026-11-11 (lavapipe software adapter, synthetic 4K NV12): `p50 = 50.44 ms`, `p99 = 57.40 ms`, under `SOFTWARE_CI_CEILING_MS` so the gate passes. **The 8 ms hardware budget is not met by any measurement so far** (`14.86 ms` on an MX350, `57.40 ms` on lavapipe; `hardware_pass = false` in both reports). This criterion is ticked because it asks for the bench to be *recorded*, which it is — not because the budget is met. (H) open and **carried into M4 as an open risk**: run **Help ▸ Diagnostics ▸ Run benchmark** on target reference hardware (`H-23` in M10)
- [x] **M3-A7** (auto) KB coverage passes; all earlier tests pass; `m3.db` is saved. — evidence: `cargo run --locked -p selah-kb-lint` passes 57 articles / 0 errors across rules R1–R18 and coverage checks 1–6 (41 existing M4/M5 planned-item warnings), `selah-kb build` writes 57 articles to `target/kb/kb.db`, `crates/selah-kb-lint/tests/lint.rs` 10/10; `tests/fixtures/db/m3.db` generated via `library_fixture --m3` with `schema-v1.db` & `m2.db` untouched and verified in `crates/selah-core/tests/migrations.rs` (3/3, Gate Stage 10); full `tools/verify.ps1` green with `tests/ledger.json` strictly non-decreasing
- [x] **M3-A8** (auto) The flash analyser flags the known flashing test clip, and transition settings that would flash more than 3 times a second, and passes safe ones. — evidence: `selah-render::flash::tests` (`wcag_luminance_rate_and_area_thresholds_hold`, `transition_risk_catches_unsafe_strobe_and_passes_smooth_fade`), `crates/selah-core/tests/flash_and_shaders.rs` (`import_media_flags_flashing_clip_and_selah_round_trip_preserves_warning`, `analyze_flash_risk_and_split_slides_report_wcag_warnings`) on `tests/fixtures/media/committed/wcag-flash-360p60.mp4` vs `flash-beep-360p60.mp4`, and `crates/selah-testkit/tests/transitions_effects_flash.rs::transitions_effects_flash_guard_and_emergency_cancel_end_to_end` (live engine `FrameShown.flash_warning` under `SAFE-04` and emergency clear reset under `SAFE-03`)

### M4. Live operation, services and stage

**Work packages**

- [ ] **M4.1** Services and the planner — evidence: 
- [ ] **M4.2** Live state machine — evidence: 
- [ ] **M4.3** Looks, props, messages, timers, macros and clear groups — evidence: 
- [ ] **M4.4** Producer mode — evidence: 
- [ ] **M4.5** Simple mode — evidence: 
- [ ] **M4.6** Stage display — evidence: 
- [ ] **M4.7** Ready for Sunday, practice mode, setup and tours — evidence: 
- [ ] **M4.8** End-to-end and chaos tests — evidence: 
- [ ] **M4.9** Knowledge base content for M4 — evidence: 

**Acceptance criteria**

- [ ] **M4-A1** (auto) A scripted 12-item service (songs, scripture, video, countdown) produces every expected frame, and every slide change lands within 1 frame of the comman… — evidence: 
- [ ] **M4-A2** (auto) Ctrl+Z in Live restores the previous on-screen state exactly (frame checksum). — evidence: 
- [ ] **M4-A3** (auto) Each screen shows the right layers for each look (golden frames). — evidence: 
- [ ] **M4-A4** (auto) Timer drift is ≤ 1 frame over 60 minutes (virtual clock); overrun behaves as set. — evidence: 
- [ ] **M4-A5** (auto) 50 random kills of UI, core or engine during an e2e run all keep the continuity rules: no black screen when UI or core is killed; an engine restart with… — evidence: 
- [ ] **M4-A6** (auto) A keyboard-only Simple-mode flow runs a song. — evidence: 
- [ ] **M4-A7** (auto) Stage display golden frames pass, including chord alignment. — evidence: 
- [ ] **M4-A8** (auto) Every Ready for Sunday check has unit tests and links to an existing fix article. — evidence: 
- [ ] **M4-A9** (auto) Tours refuse to start while live content is on outputs, unless practice mode is on. — evidence: 
- [ ] **M4-A10** (auto) KB coverage passes; the volunteer handbook collection exists; the ledger has grown; `m4.db` is saved. — evidence: 
- [ ] **M4-A11** (H) 3 people new to Selah run a practice service in Simple mode after the 5-minute tour, without help. — evidence: 
- [ ] **M4-A12** (auto) Booth lock leaves only Next, Previous and Clear working. — evidence: 

### M5. Screens, video and audio I/O

**Work packages**

- [ ] **M5.1** Display profiles and screen management — evidence: 
- [ ] **M5.2** Edge blending, corner pin and masks — evidence: 
- [ ] **M5.3** Colour calibration, LED walls and test patterns — evidence: 
- [ ] **M5.4** NDI in and out — evidence: 
- [ ] **M5.5** Texture sharing and virtual camera — evidence: 
- [ ] **M5.6** Cameras and capture inputs — evidence: 
- [ ] **M5.7** Audio routing, playlists and meters — evidence: 
- [ ] **M5.8** Web pages and web video — evidence: 
- [ ] **M5.9** Outputs UI, health and knowledge base — evidence: 

**Acceptance criteria**

- [ ] **M5-A1** (auto) The blend ramp matches its analytic curve within tolerance; corner pin and mask golden frames pass. — evidence: 
- [ ] **M5-A2** (auto) Colour calibration and test pattern golden frames pass; LED spanning layouts render correctly. — evidence: 
- [ ] **M5-A3** (auto) NDI loopback keeps alpha intact (fake backend always; the real SDK when present). — evidence: 
- [ ] **M5-A4** (auto) Spout or Syphon loopback works on the current platform. — evidence: 
- [ ] **M5-A5** (auto) Audio routing with a null sink sends signal to the right channels; measured per-output delay is within ±1 ms of the setting. — evidence: 
- [ ] **M5-A6** (auto) Simulated hot-plug switches profiles; outputs recover with no engine restart. — evidence: 
- [ ] **M5-A7** (auto) A local test web page renders as a layer (golden frame). — evidence: 
- [ ] **M5-A8** (H) A 3-projector blend has no visible seam. — evidence: 
- [ ] **M5-A9** (auto) KB coverage passes; earlier tests pass; `m5.db` is saved. — evidence: 
- [ ] **M5-A10** (auto) CEF policy tests pass: no file access, downloads, pop-ups or navigation to other sites. — evidence: 

### M6. Importers, migration and the alpha release

**Work packages**

- [ ] **M6.1** Import framework and worker — evidence: 
- [ ] **M6.2** MediaShow importer — evidence: 
- [ ] **M6.3** ProPresenter 6 and 7 importers — evidence: 
- [ ] **M6.4** Song-app importers — evidence: 
- [ ] **M6.5** Presentation importers and PowerPoint live mode — evidence: 
- [ ] **M6.6** Migration assistant and import report — evidence: 
- [ ] **M6.7** Alpha release pipeline — evidence: 
- [ ] **M6.8** Import quality gate, fuzzing and knowledge base — evidence: 

**Acceptance criteria**

- [ ] **M6-A1** (auto) Golden import tests pass for every format. — evidence: 
- [ ] **M6-A2** (auto) On the import corpus, at least 98% of songs need no manual fix (measured against expected outputs). — evidence: 
- [ ] **M6-A3** (auto) Each parser has been fuzzed for 10 minutes in a cloud session (corpus committed) and passes a 30-second smoke run in CI, with no crash or hang. — evidence: 
- [ ] **M6-A4** (auto) The support bundle includes importer logs and still contains no secrets (test with seeded fake secrets). — evidence: 
- [ ] **M6-A5** (H) PowerPoint import and live mode work on a PC with PowerPoint. — evidence: 
- [ ] **M6-A6** (H) The installer installs, updates and uninstalls cleanly on clean Windows and macOS machines. — evidence: 
- [ ] **M6-A7** (auto) KB coverage passes; earlier tests pass; `m6.db` is saved. — evidence: 
- [ ] **M6-A8** (H) The closed alpha starts with 10 churches (H-22). — evidence: 
- [ ] **M6-A9** (auto) Each worker runs with reduced rights (token and job-object checks on Windows, the sandbox profile on macOS). — evidence: 

### M7. Remote control, API and show control

**Work packages**

- [ ] **M7.1** Gateway, pairing and security — evidence: 
- [ ] **M7.2** Public API v1 and OpenAPI — evidence: 
- [ ] **M7.3** Web apps — evidence: 
- [ ] **M7.4** SDKs and Companion module — evidence: 
- [ ] **M7.5** MIDI — evidence: 
- [ ] **M7.6** OSC, DMX/sACN and network triggers — evidence: 
- [ ] **M7.7** Timelines and timecode — evidence: 
- [ ] **M7.8** Multi-machine sync: backup and campus feeds — evidence: 
- [ ] **M7.9** Knowledge base content for M7 — evidence: 

**Acceptance criteria**

- [ ] **M7-A1** (auto) The OpenAPI document validates; contract tests reach every command and query; every response validates against its schema. — evidence: 
- [ ] **M7-A2** (auto) Security: no access without a token; scopes are enforced (viewer can't trigger); revoked tokens fail at once; the API is off by default. — evidence: 
- [ ] **M7-A3** (auto) Playwright tests pass for the remote, pastor view and stage apps against a headless core. — evidence: 
- [ ] **M7-A4** (auto) MIDI loopback on virtual ports triggers the mapped commands; learn mode captures a note. — evidence: 
- [ ] **M7-A5** (auto) Timecode: LTC generated in the test chases with ±1 frame accuracy; timeline cues fire on time. — evidence: 
- [ ] **M7-A6** (auto) Failover: 2 engine/core pairs on one machine. — evidence: 
- [ ] **M7-A7** (auto) The Companion module builds and passes its tests against a mock API. — evidence: 
- [ ] **M7-A8** (H) A real phone pairs by QR and runs a service; Companion works with a Stream Deck. — evidence: 
- [ ] **M7-A9** (auto) KB coverage passes; earlier tests pass; `m7.db` is saved. — evidence: 
- [ ] **M7-A10** (auto) The API fuzz smoke run finds no crash. — evidence: 

### M8. Broadcast, creative and intelligence

**Work packages**

- [ ] **M8.1** Blackmagic DeckLink — evidence: 
- [ ] **M8.2** AJA — evidence: 
- [ ] **M8.3** Recording — evidence: 
- [ ] **M8.4** Streaming — evidence: 
- [ ] **M8.5** Multiview and stream graphics — evidence: 
- [ ] **M8.6** Full transition and effect library, and the shader SDK — evidence: 
- [ ] **M8.7** Speech cues — evidence: 
- [ ] **M8.8** Translation lines and parallel Bibles — evidence: 
- [ ] **M8.9** Content Packs — evidence: 
- [ ] **M8.10** Knowledge base content for M8 — evidence: 

**Acceptance criteria**

- [ ] **M8-A1** (auto) DeckLink and AJA simulators receive correct fill and key frames and timing; an input source works. — evidence: 
- [ ] **M8-A2** (auto) Recordings pass ffprobe validation; A/V sync is within ±1 frame; the disk guard stops recording safely. — evidence: 
- [ ] **M8-A3** (auto) A 30-minute stream to a local MediaMTX server (RTMP and SRT), run in a cloud session (CI runs a 1-minute version), has no reconnect; a forced disconnect… — evidence: 
- [ ] **M8-A4** (auto) Golden frames pass for every new transition and effect, and the SDK sample loads and hot-reloads. — evidence: 
- [ ] **M8-A5** (auto) On the test audio set, explicit scripture references are detected with at least 90% precision and 80% recall; nothing triggers live automatically. — evidence: 
- [ ] **M8-A6** (auto) Golden frames pass for translation lines and parallel Bibles; a Content Pack installs, searches and builds slides. — evidence: 
- [ ] **M8-A7** (H) DeckLink and AJA fill and key are verified on a real switcher; streaming is verified to a real platform. — evidence: 
- [ ] **M8-A8** (auto) KB coverage passes; earlier tests pass; `m8.db` is saved. — evidence: 
- [ ] **M8-A9** (auto) With speech cues on, the egress test still shows no outgoing connections. — evidence: 

### M9. Cloud, teams and enterprise

**Work packages**

- [ ] **M9.1** Sync server and organisation model — evidence: 
- [ ] **M9.2** Client sync engine — evidence: 
- [ ] **M9.3** Accounts, roles, SSO and SCIM — evidence: 
- [ ] **M9.4** Admin console, policies, audit log and privacy — evidence: 
- [ ] **M9.5** CCLI SongSelect and reporting — evidence: 
- [ ] **M9.6** Planning Center, MultiTracks and Loop — evidence: 
- [ ] **M9.7** Licensing and entitlements — evidence: 
- [ ] **M9.8** Enterprise packaging and update rings — evidence: 
- [ ] **M9.9** Telemetry, crash reporting and fleet dashboard — evidence: 
- [ ] **M9.10** Church articles, handbook export and knowledge base — evidence: 

**Acceptance criteria**

- [ ] **M9-A1** (auto) Sync property test: 5 simulated clients with random offline periods and edits all converge with no lost data; concurrent lyric edits produce conflict co… — evidence: 
- [ ] **M9-A2** (auto) SSO end to end against Keycloak in Docker; the SCIM conformance suite passes; the permission matrix is tested for every role × command. — evidence: 
- [ ] **M9-A3** (auto) The audit log records every admin and API action; GDPR export and delete work end to end. — evidence: 
- [ ] **M9-A4** (auto) CCLI, Planning Center, MultiTracks and Loop contract tests pass on recorded fixtures. — evidence: 
- [ ] **M9-A5** (auto) An upgrade from every earlier milestone's DB fixture and installer build to the current one migrates without data loss. — evidence: 
- [ ] **M9-A6** (auto) Telemetry and crash upload send nothing when consent is off (network-capture test) and the expected payloads when it's on. — evidence: 
- [ ] **M9-A7** (H) Silent MSI install with ADMX policies works on a domain or Intune test machine; the notarised macOS pkg installs with a configuration profile; real partner… — evidence: 
- [ ] **M9-A8** (auto) KB coverage passes; earlier tests pass; `m9.db` is saved. — evidence: 
- [ ] **M9-A9** (H) The closed beta starts with 50 churches, including 5 multi-campus and 5 broadcast-heavy (H-22). — evidence: 
- [ ] **M9-A10** (auto) The sync server refuses unauthenticated and cross-organisation requests (a test per endpoint). — evidence: 

### M10. Launch readiness and v1.0

**Work packages**

- [ ] **M10.1** Parity and v1.0-bar checklist — evidence: 
- [ ] **M10.2** Knowledge base completion and docs website — evidence: 
- [ ] **M10.3** Localisation — evidence: 
- [ ] **M10.4** Accessibility — evidence: 
- [ ] **M10.5** Performance, soak and chaos — evidence: 
- [ ] **M10.6** Security — evidence: 
- [ ] **M10.7** Release operations and v1.0 sign-off — evidence: 

**Acceptance criteria**

- [ ] **M10-A1** Every item in `docs/v1-parity-checklist.md` has passing evidence. — evidence: 
- [ ] **M10-A2** (auto) (ci-win) `tools/verify.sh --release` and `tools/verify.ps1 -Release` exit 0: KB coverage 100%, no drafts, every article reviewed, no stale translations. — evidence: 
- [ ] **M10-A3** (auto + H) The UI and knowledge base are complete in 6 languages and reviewed by native speakers. — evidence: 
- [ ] **M10-A4** (auto + H) Automated accessibility checks pass, and there are no open AA findings from the manual audit. — evidence: 
- [ ] **M10-A5** (H) Performance budgets (ARCHITECTURE §12) pass on reference hardware; the 72-hour soak shows no leaks or drift. — evidence: 
- [ ] **M10-A6** (auto + H) The chaos suite passes; there are no open P0 or P1 bugs or security findings. — evidence: 
- [ ] **M10-A7** (H) Every product plan §7.1 row is green: 99.9% glitch-free services across ≥ 50 churches over 12 weekends; SUS ≥ 80; volunteer findability ≥ 90%; migration ≥… — evidence: 
- [ ] **M10-A8** `v1.0.0` is tagged and signed, release notes are published, and the update feed is live. — evidence: 
- [ ] **M10-A9** (auto + H) Every SEC and SAFE item in docs/SECURITY.md has passing evidence in `docs/v1-parity-checklist.md`. — evidence: 

## Human tasks

Owner: you. Coding sessions keep this table up to date, mark which feature flags are waiting on each task, and never block other work on them (ADR-017).

| ID | Task | Needed by | Status | Notes |
|---|---|---|---|---|
| H-01 | Approve installing toolchains (Rust, VS Build Tools, LLVM, .NET 10 SDK, pnpm, buf). Only needed if you also code locally; cloud sessions install their own. | M1 (optional) | Not needed for cloud sessions | |
| H-02 | Confirm the product name and choose the code licence | M1 (placeholder allowed) | Open | |
| H-03 | GitHub remote (done). Before CI can run: clean the history, apply the public-period settings and make the repository public (docs/CLOUD-DEVELOPMENT.md §2.2 and §3) | M1 | Partly done | Repository recreated with a clean history and made public (2026-11-01). Still to do: the settings in docs/CLOUD-DEVELOPMENT.md §2.2. |
| H-04 | macOS checks run in the `macos` CI job. Provide a Mac only for hands-on macOS testing. | M1 onward (optional) | Open | |
| H-05 | Legal review: use of the analysis, interoperability importers (product plan §1) | Before M6 | Open | |
| H-06 | Confirm rights for bundled Bible translations | M2 | Open | Only public-domain KJV and WEB are bundled in M2 (`content/bibles/PROVENANCE.md`). |
| H-07 | Accept the NDI SDK licence and install the NDI runtime | M5 | Open | |
| H-08 | Hardware lab: projectors or displays, an LED processor, a capture card, a camera, a MIDI controller, a Stream Deck, a DMX node | M5, M7 | Open | |
| H-09 | Legitimate sample libraries from churches, with written permission | M6 | Open | |
| H-10 | A Google Cloud OAuth client for Google Slides | M6 | Open | |
| H-11 | A Blackmagic DeckLink card and SDK download; an AJA card | M8 | Open | |
| H-12 | A CCLI partner agreement and API credentials (start talks in M1) | M9 | Open | |
| H-13 | A Planning Center OAuth application | M9 | Open | |
| H-14 | MultiTracks and Loop Community partnerships | M9 | Open | |
| H-15 | Code-signing: Windows (Azure Trusted Signing or EV) and an Apple Developer ID with notarisation | M6 (alpha), M9 | Open | |
| H-16 | Cloud hosting, domains and transactional email for the sync service and docs site | M9 | Open | |
| H-17 | A payments account (for example Stripe) and pricing | M9 | Open | |
| H-18 | A content designer for 2i reviews of knowledge base articles | M2 onward | Open | 57 M1–M3 English articles sit at `status: review` (`reviewed_by: ""`). |
| H-19 | Translators and native reviewers for es, pt, fr, ko, yo | M10 | Open | |
| H-20 | A manual accessibility audit with screen-reader users | M10 | Open | |
| H-21 | A third-party penetration test | M10 | Open | |
| H-22 | Pilot churches: 3 testers (M4), 10 alpha (M6), 50 beta (M9); usability studies | M4–M10 | Open | |
| H-23 | Reference hardware (ARCHITECTURE §12, TESTING §6) | M10 (useful from M1) | Open | |
| H-24 | Supply the private MediaShow format notes when M6.2 starts (they're kept outside the repository) | M6 | Open | |
| H-25 | Set up the Claude Code cloud environment: the setup script and network domains (docs/CLOUD-DEVELOPMENT.md §2.1) | M1 | Open | |
| H-26 | Before making the repository private again, choose how test builds and updates are delivered (docs/CLOUD-DEVELOPMENT.md §6) | When going private | Open | |

## Hardware and account checklists (H)

_Each (H) acceptance criterion gets a step-by-step checklist here when its milestone is built, for a person with the equipment or account to run._

### M1 (run on the installed test build)

Get the installer: open the M1 PR, select **Checks**, open the latest **CI** run, and download the **Selah-win-x64-Setup** artifact (unzip it). After merging, the same file is on the `m1` release.

**M1-A12: install and see the test pattern** (about 5 minutes)

- [ ] Run `Selah-win-x64-Setup.exe`. If SmartScreen warns you (it isn't signed yet, H-15), select **More info**, then **Run anyway**.
- [ ] Windows doesn't ask for administrator rights.
- [ ] Open **Selah** from the Start menu. The status in the toolbar says **Connected** within a few seconds.
- [ ] Select **Show test pattern**. Colour bars fill the output window, and the preview shows them too.
- [ ] Press **F12**. The screens clear at once.
- [ ] Select **Show test pattern** again, then close the main window. A yellow bar asks **Something is on the screens.** Select **Keep showing**: Selah stays open. Close again and select **Clear and quit**: the screens clear and Selah closes.
- [ ] Uninstall Selah from **Settings ▸ Apps ▸ Installed apps**. Your settings stay in `%APPDATA%\Selah`.
- [ ] Note the result in the PR (pass, or what went wrong).

**M1-A7: benchmark report** (about 2 minutes)

- [ ] With nothing on the screens, select **Help**, then **Diagnostics**.
- [ ] Check the details: your graphics card and the screen size.
- [ ] Select **Run benchmark** and wait about a minute.
- [ ] Select **Show the report** and attach the `selah-diagnostics-….json` file to the PR.

**Optional: Report a problem**

- [ ] Select **Help**, then **Report a problem**. A bar shows where the support file was saved, and the problem form opens in your browser with the version filled in. Open the `.zip` and check it has logs, and no passwords or your name. Close the browser tab without sending.

### M2 (open human checks)

Get the test build the same way from the M2 PR's **Checks** run (`Selah-win-x64-Setup` artifact) once CI completes.

**M2-A10 / H-06: confirm Bible translation rights**

- [ ] Keep bundled Bibles restricted to public-domain **KJV** and **WEB** for now (`content/bibles/PROVENANCE.md`), or list approved additional translations and written publisher permissions before bundling more in later milestones.

**H-18: 2i review of the M2 knowledge base articles**

- [ ] Review the 37 English help articles under `content/kb/en/` (currently `status: review`, `reviewed_by: ""`) for plain tone, bold UI label accuracy, and volunteer clarity; flip `status` to `ready` and set `reviewed_by` when approved.

**Optional hands-on check on your PC** (about 5 minutes)

- [ ] In **Library ▸ Songs**, paste a song in plain lyrics or ChordPro, watch sections and chord badges detect, transpose by +2 semitones, save, and verify **Undo** / **Redo**.
- [ ] In **Library ▸ Bibles**, look up `John 3:16` in KJV and WEB (and test a French/Spanish/Yorùbá/Korean book name), import a local OSIS/USFM/Zefania file, and generate verse slides.
- [ ] In **Library ▸ Media**, import a local image/audio/video file and check its thumbnail; export and re-import a `.selah` bundle, or create/restore a full backup `.zip`.
- [ ] Press **F1** on different screens, toggle **Shift+F1** ("What's this?"), search with **Ctrl+K**, and verify **F12** (**Clear everything**) works immediately even while a help overlay is open.

### M3 (open human checks)

Get the test build from the M3 PR's **Checks** run (**Selah-win-x64-Setup** artifact) once CI completes.

**M3-A6 (H): 2 × 1080p outputs + 4K video background benchmark on your PC** (about 2 minutes)

- [ ] With nothing on the screens, select **Help**, then **Diagnostics**.
- [ ] Select **Run benchmark** and note **M3 compositor (2×1080p + 4K video)** p99 frame time against the 8 ms hardware target (`H-23` reference hardware sign-off happens in M10).
- [ ] Select **Show the report** and attach `selah-diagnostics-….json` to the PR if you want hardware GPU numbers recorded alongside the CI logs.

**H-18: 2i review of the M3 knowledge base articles**

- [ ] Review the 20 new M3 English articles under `content/kb/en/{templates,slides,glossary,troubleshooting}/` (57 total M1–M3 articles currently `status: review`, `reviewed_by: ""`) for plain tone, bold UI label accuracy, and volunteer clarity; flip `status` to `ready` and set `reviewed_by` when approved.

**Optional hands-on check on your PC** (about 5 minutes)

- [ ] In **Templates**, pick one of the 4 starter looks (**Sanctuary**, **Warm Serif**, **Lower Third**, **Stage Monitor**), tweak a font, outline, card box, safe area, or screen shape (**16:9**, **LED 32:9**, **4:3**, **9:16**), save, and check a song split with shrink-to-fit.
- [ ] In **Slides**, create a custom slide deck with **Text**, **Clock** (`{clock}`), **Timer** (`{timer}`), a **Shape**, and an **Image** or **Video** background, align/group elements, apply a transition and a built-in effect, and test **Undo** / **Redo**.
- [ ] Trigger a flash warning (for example a fast strobe transition or flashing clip) and confirm Selah warns you and blocks staging until you confirm, then verify **Staged Next** previews separately from live **Program** until you send it live, and test **F12** (**Clear everything**).

## Test ledger history

Copied from `tests/ledger.json` at the end of each milestone. Counts may only go up (ADR-016).

| Milestone | rust | dotnet | web | golden | ui | e2e | kb | total |
|---|---|---|---|---|---|---|---|---|
| (baseline) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| M1 | 121 | 20 | 3 | 4 | 70 | 6 | 7 | 231 |
| M2 | 211 | 24 | 8 | 4 | 98 | 6 | 37 | 388 |
| M3 | 279 | 28 | 12 | 10 | 118 | 6 | 57 | 510 |

## Deviations and decisions

_Anything done differently from BUILD-PLAN.md or the docs, with the reason and the ADR number if one was added._

- 2026-11-11, M1–M3 review (no milestone work): first cloud session of the project. Reviewed M1–M3 against the code and fixed what the review found.
  - **The cloud gate was red and had never been run.** `tools/verify.sh --cloud` failed at stage 5: the Windows type-check needs a prebuilt Skia for `x86_64-pc-windows-gnu`, `tools/cloud-setup.sh` had `linux-x64` hard-coded into `SKIA_BINARIES_URL` (so it looked for a Windows tarball inside the Linux folder), and nothing is pinned for that target — rust-skia publishes `windows-msvc` only, and the source fallback needs network the session doesn't have. The Skia- and FFmpeg-linked crates now come from `tools/windows-check-exclusions.txt`, which records why each is excluded; `selah-render`, `selah-core`, `selah-core-host`, `selah-engine` and `selah-bench` have no `#[cfg(windows)]` code, so nothing is lost for them, while `selah-media` and `selah-testkit` do and are recorded as CI-only. `tools/check-windows-coverage.py` (gate stage 5, tested in `tests/tools/test_windows_coverage.py`) fails the gate if an excluded crate gains `#[cfg(windows)]` code without that being recorded. `cfg!(windows)` runtime branches deliberately don't count: both arms compile everywhere. The comment in `cloud-setup.sh` now says why the path is Linux-only.
  - **Five test names cited as evidence didn't exist.** `PROGRESS.md` M3-A8 and `docs/SECURITY.md` §9 SAFE-04 named two flash-analyser tests that were never written under those names (the real ones are `wcag_luminance_rate_and_area_thresholds_hold` and `transition_risk_catches_unsafe_strobe_and_passes_smooth_fade`); SAFE-05 named both audio-limiter tests wrongly (`master_limiter_keeps_plus_6dbfs_sine_under_minus_1_dbtp`, `clear_fade_ramps_smoothly_to_silence`); M3.6 credited a "poster_frame_from_path" in `selah-render` instead of `load_or_extract_video_poster` in `selah-core`. (A name that no longer exists is written in plain quotes, not backticks, so `tools/check-evidence.py` only checks names offered as real evidence.) Every control was genuinely tested — only the labels were wrong. `tools/check-evidence.py` (gate stage 14, tested in `tests/tools/test_evidence.py`) now checks all 128 cited test names and fails if one doesn't exist.
  - **Eight hard-coded colours in the UI**, against the tokens-only rule. New tokens `color.{light,booth}.slide-text`, `slide-text-muted` and `scrim` cover slide-preview text and overlay dimming, with contrast pairs added. The command palette's inline `0 16 40 0 #40000000` shadow also contradicted DESIGN-SYSTEM §3 twice — far heavier than the documented elevation, and present in Booth, which should have no shadows — so a `shadow.{light,booth}.elevation` token group was added (`design/tokens/build.mjs` emits `BoxShadows` resources; Booth resolves to `none`). `TokenTests` gained 3 tests, one of which fails if any view hard-codes a colour again (ui 118 → 121).
  - **M1-A1 and M3-A6 ticks were misleading** and now state what was really measured; the 8 ms budget miss is carried in an open-risks table under Current status.
  - No milestone code changed: the fixes are in `tools/`, `design/tokens/`, the generated token files, three `.axaml` views and the docs. Full gate green in this cloud session (`tools/verify.sh --cloud`).
- 2026-11-09, M2.1: this session runs on Windows, so `tools/verify.ps1` is the local full gate; Linux checks run in PR CI. The initial baseline run was interrupted by implementation edits before its test stage completed; it is not recorded as a passing baseline. The first sandboxed attempt also needed network access for pinned dependency checks and `PYTHONIOENCODING=utf-8` for Unicode diagnostics in this Windows shell.
- 2026-11-09, M2.1: ADR-027 replaces the incompatible `rusqlite_migration` dependency with an append-only transactional/checksummed runner over existing bundled `rusqlite` 0.41. Added `uuid` (MIT/Apache-2.0) for OS-random UUIDv7 identities. Dependency resolution tried to move bindgen from its existing `syn` 2 dependency to `syn` 3, breaking compilation; its original lockfile entry was retained. No toolchain or pre-existing dependency version was changed.
- 2026-11-09, M2.2: added `fastrand` `2` (MIT/Apache-2.0) as a dev-dependency on `selah-core` for the deterministic 10,000-sequence M2-A2 undo/redo property test (`cargo deny check` green); `selah-testkit::library_crash` holds the process-level M2-A3 recovery suite (`tests/ledger.json` counts it under `rust`, with `E2E_FILES` unchanged).
- 2026-11-09, M2.3: added `selah-bench search --check` to gate stage 6 (`tools/verify.py`) so every run checks the 10,000-song `M2-A4` p95 <= 50 ms budget on the release binary; also updated `selah-engine`'s `Engine::start` so `Mode::Headless` binds IPC and begins heartbeats immediately while `selah-render` initializes WARP and WGSL pipelines asynchronously on its thread, eliminating a 500 ms reconnect-backoff overshoot exposed under parallel workspace test load (`continuity::engine_killed_is_restarted_and_the_scene_restored` verified 3x + full gate).
- 2026-11-09, M2.7–M2.8: implemented library/editor/command-palette Avalonia UI and contextual help controls (`F1`, `Shift+F1` What's this, `InfoButton`, `EmptyState`, `ErrorAlert`), generating `HelpAnchors.g.cs` from `content/help/anchors.yaml` via `tools/gen_help_anchors.py` (`tools/check-generated.py` enforced) and checking all 6 bidirectional coverage rules in `selah-kb-lint`.
- 2026-11-09, M2.9: added Stage 10 `migrations` (`cargo test --locked -p selah-core --test migrations`) to `tools/verify.py` so every gate run verifies all saved fixtures (`schema-v1.db` and `m2.db`) without resetting `tests/ledger.json` counts (`migrations.rs` is also included in Stage 6 `cargo test`); ran full `tools/verify.ps1` locally on Windows (`H-01`), leaving Linux/macOS gate execution to PR CI with label `ci:macos`. M2.1–M2.9 used no network calls except initial pinned tool verification, kept `M2-A10 (H-06)` unchecked (KJV & WEB only), and left all 37 KB articles at `status: review` (`H-18`).
- 2026-11-10, M3.1–M3.8: local Windows session (`H-01`), so `tools/verify.ps1` is the full local gate while Linux/macOS validation runs in PR CI with label `ci:macos`. `selah-bench m3` keeps `hardware_pass` strictly at `OUTPUT_P99_BUDGET_MS = 8.0 ms` (`14.86 ms` measured on this PC's NVIDIA GeForce MX350 with 4K HEVC; `H-23` reference hardware sign-off is in M10) while using `SOFTWARE_CI_CEILING_MS = 5000.0 ms` for software/virtual CI adapters (`WARP`, `lavapipe`, VM Metal) and falling back to synthetic `3840×2160` NV12 frames when `hevc-2160p60.mp4` isn't generated (for example decode-only macOS FFmpeg). `golden_m3` references (`55` `m3-*.png` frames) were rendered on Windows WARP (`TOLERANCE_STYLE` 2% edge tolerance for DirectWrite vs FreeType/CoreText; `TOLERANCE_SHADER` 0.2% for geometry/transitions/effects); PR #6 run `42913050496` surfaced FreeType/CoreText glyph stem differences on `m3-style-min-size-shrink` (`tests/golden/{linux,macos}/m3-style-min-size-shrink.png` reviewed and committed with `tests/golden/CHANGES.md`), a cross-GPU `Zoom` edge ULP precision discrepancy on exact `0.0`/`1.0` rows (`y=22`/`y=157` at `320×180, p=0.5`, fixed in `transitions.wgsl` + `transitions.rs` using `@builtin(position)` pixel-center UVs and `1e-5` boundary epsilon), and loaded Windows CI frame-interval variance in Step 4 of `transitions_effects_flash` (Wipe `duration_ms` raised from `420` to `1500` ms). Fixed chorded-body slot rendering in `crates/selah-render/src/text.rs` so `background_box`, `outline`, and `shadow` match unchorded body slots. Left all 57 M1–M3 KB articles at `status: review` (`H-18`).

- 2026-10-31, before M1: the plan moved to cloud-first development (ADR-023), lean CI (ADR-024), a pull-request workflow (ADR-025, replacing ADR-019) and secure and safe by default (ADR-026). M1 grew from 10 to 13 WPs (installer, security and safety baseline, problem reports and diagnostics).

- 2026-11-07, M1 (local session): this session runs on the user's Windows PC (H-01) rather than in a cloud session, so the gate was run stage by stage on Windows plus `cargo check` for the Linux and macOS targets; the Linux gate runs in the PR's `linux` job.
- 2026-11-07, M1.1: no LGPL macOS FFmpeg binary is published, so the lockfile pins the FFmpeg 7.1.2 source and `tools/setup.sh --fetch` builds a decode-only shared FFmpeg on macOS. Without encoders the Mac can't generate test clips, so the video correctness test (M1.7) uses a small committed clip.
- 2026-11-07, M1.1: `cargo-deny` is pinned as prebuilt release binaries in `tools/third_party.lock.json` instead of being compiled in every CI run.
- 2026-11-07, M1.3: the cargo-fuzz target needs nightly, so the CI smoke run is a stable test over the same entry point (`SELAH_FUZZ_SECONDS`); coverage-guided runs use `fuzz/` in cloud sessions.
- 2026-11-07, M1.4: the UI continuity check (M1-A3) uses `selah-fake-ui`, a stand-in that heartbeats, connects and subscribes like the UI, until the Avalonia shell (M1.8) exists. The crate `selah-runtime` (paths, locks, logs, redaction, heartbeats) is new and not yet in ARCHITECTURE §1; it's added with the M1 docs update.

- 2026-11-07, M1.10 before M1.7–M1.9: the gate was built out of order so this PR's CI runs it (without `tools/verify.sh`, CI runs only the policy checks), and because M1.7 is blocked on LLVM on this PC.
- 2026-11-07, M1.10: the gate's stages live in `tools/verify.py`; `verify.sh` and `verify.ps1` are thin wrappers, so the platforms can't drift. CI jobs gained a pinned `actions/setup-node` (Node 22) and `corepack enable pnpm`, which the toolchain check needs.
- 2026-11-07, M1.10: `bless-golden.yml` renders on the chosen platform and uploads the frames as an artifact instead of pushing to the PR branch (the CI policy requires `persist-credentials: false` and read-only permissions); a session reviews and commits the image with its reason.
- 2026-11-07, M1.6: golden references were rendered on Windows (WARP) in this local session, not on lavapipe in a cloud session (ADR-023). Text is rasterised by Skia on the CPU, so the frames don't depend on the GPU; Linux and macOS compare within `Tolerance::TEXT` in CI.
- 2026-11-07, M1.6: slide fonts are pinned to weight 400 because Noto Sans KR's variable font defaults to Thin; templates choose weights from M3.
- 2026-11-07, M1.8 (started): `color.primary-fill` (Booth `#0050D8`) added and light `color.focus` changed to `#0076D6`, because the documented values failed the 4.5:1 and 3:1 contrast checks; DESIGN-SYSTEM §3 updated.

- 2026-11-07, M1.8: Avalonia 12 (latest stable, ADR-009 says "latest stable at M1"); its headless test package needs xUnit v3, so `Selah.UI.Tests` uses xUnit v3 while `Selah.Client.Tests` stays on xUnit 2. Tokens are generated by Style Dictionary (ADR-022), not `Selah.UI.Generators`; ARCHITECTURE updated. `selah-fake-ui` is replaced by the real UI run headless. New token `color.preview-bg` (DESIGN-SYSTEM §3).
- 2026-11-07, M1.9: `content/kb/planned.yaml` also takes `kind: label` for bold UI labels (R8) that later milestones add (the exemplars name M4 controls). R4 is applied as written (at most 3 sentences and 60 words), so two exemplar paragraphs and one new one were split without changing their words. The black-screen exemplar now says **Report a problem** (the M1.13 command) instead of **Export support bundle**. Settings that articles may name are listed in `content/help/settings.yaml`. New error code `SEL-HELP-0001`. `content/terms.yaml` gained `proper_nouns` (R18).
- 2026-11-07, M1.6: `tests/golden/linux/text-en.png` is a Linux-specific reference (FreeType vs DirectWrite edges on 0.53% of pixels), taken from the linux job's failure report because `bless-golden.yml` can't be dispatched until it's on `main`.
- 2026-11-07, M1.5: headless `FrameShown` is stamped at the GPU queue submit, the same moment as a swap-chain present with windows (the Windows runner's software rasteriser needs more than a frame to finish 1080p in a debug build).

- 2026-11-07, M1.11: Velopack's pack id is `SelahApp` (installs to `%LOCALAPPDATA%\SelahApp`) because Selah's logs and cache live in `%LOCALAPPDATA%\Selah` (ARCHITECTURE §4.1). The UI is published into `ui\` because `Selah.exe` and the `selah.exe` launcher are the same file name on Windows. The Visual C++ runtime ships app-local (the redistributable installer needs admin, SEC-01). The installer check uses a UI self-test (`SELAH_SELF_TEST=1`) to see "Connected" and the bars from outside the processes.
- 2026-11-07, M1.11: the updater lives in core and downloads through `selah-net` rather than Velopack's own `UpdateManager`, so every outgoing connection goes through one gate (SEC-13). Velopack's `Update.exe apply` installs the verified package; checked by hand 0.0.1 → 0.0.2 while running.
- 2026-11-07, M1.12: `deny.toml` allows CDLA-Permissive-2.0 for `webpki-root-certs` only (Mozilla's CA list, a fallback inside `rustls-platform-verifier`).
- 2026-11-07, M1.12: the SAFE-03 chaos test freezes core by suspending its threads (`selah-testkit::freeze`, Windows FFI allowlisted, `kill -STOP` elsewhere). It was added after M1.12's full gate and run on its own; the M1.13 gate ran it with everything else.
- 2026-11-07, M1 finish: `tests/fixtures/db/m1.db` isn't created, because M1 has no library database (TESTING §4.2 starts fixtures at M2; core keeps only `settings.json` and `live_state.bin`). The ledger is first recorded as a milestone row here.

- 2026-11-07, M1 finish: `tests/golden/macos/text-en.png` and `text-yo.png` are macOS references (CoreText edges), taken from the `macos` job's failure report and reviewed by eye. `bless-golden.yml` failed on macOS (run 42411409570) because it doesn't build the decode-only macOS FFmpeg that gate stage 1 builds; follow-up: add that step before the next macOS blessing.

## Quarantined tests

_Tests temporarily isolated because they're flaky, each with the reason, an owner and the milestone that must fix it. Quarantine blocks milestone completion._

(none)
