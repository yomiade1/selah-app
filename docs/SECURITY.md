# Selah security and safety

Selah runs in front of a congregation, on church computers, often on shared Wi-Fi, operated by volunteers. A security bug can leak a church's data. A safety bug can put the wrong thing on a big screen, flash a strobe at a room full of people, or blast sound through a PA. This document sets out how Selah prevents both, **from the first milestone** (ADR-026).

Every requirement has an ID. Work packages, tests and PRs refer to these IDs. A feature that can put something on screen, make sound, open a network port, read files from outside, or store a secret isn't done until its SEC and SAFE items have passing tests.

To report a vulnerability, see [SECURITY.md](../SECURITY.md) in the repository root.

## 1. Principles

1. **Secure and safe by default.** Every risky feature (the API, web layers, speech cues, telemetry, network sources) starts **off**. Turning it on is a deliberate choice, and Selah explains what changes.
2. **Least privilege.** Selah never needs administrator rights to install or run. Each process and worker gets only what it needs.
3. **Everything from outside is untrusted.** Imported files, media, bundles, Content Packs, church articles, network messages and web pages are parsed defensively, with limits, in separate processes where possible.
4. **The screens are sacred.** Nothing reaches the audience screens or speakers without an authorised person choosing it. The emergency clear always works.
5. **Privacy by default.** Selah sends nothing over the network unless a person asks for it or turns on a feature that needs it. Audio from the microphone never leaves the computer.
6. **Fail safe.** When something breaks, Selah keeps the last good frame, stays quiet, and tells the operator in plain words what to do next.
7. **Proven, not promised.** Each requirement below has an automated test or a recorded human check.

## 2. What we protect

| Asset | Why it matters |
|---|---|
| What the room sees and hears | The audience screens, stage displays, streams and the PA. Mistakes are public and can harm people (flashing, loud sound). |
| The church's library | Songs, services, media, notes and settings: years of work. |
| Credentials | Stream keys, partner tokens (CCLI, Planning Center), API tokens, sign-in sessions. A leaked stream key lets anyone broadcast as the church. |
| Personal data | Team members' names and emails, accounts, audit logs, recordings, microphone audio. |
| The church's computer | Selah must never become a way into the machine or the church network. |
| The update channel | A tampered update would run on every church's computer. |
| The source code and CI | Especially while the repository is public (ADR-024). |

## 3. Threat model

| Boundary | Who or what is on the other side | Main threats | Controls |
|---|---|---|---|
| Local IPC between Selah's processes | Other programs and other users on the same computer | Another process injects commands or reads live state | SEC-02, SEC-14 |
| Files from outside | Imports, media, `.selah` bundles, Bibles, Content Packs, fonts, church articles | Crafted files crash, hang or exploit a parser; zip bombs; path traversal | SEC-03, SEC-04, SEC-15 |
| The local network | Phones and tablets on church Wi-Fi (often a guest network), control surfaces, NDI sources, other Selah computers | Someone on the Wi-Fi takes control or pushes content to the screens; token theft; flooding | SEC-07, SEC-14, SAFE-01, SAFE-02, SAFE-08 |
| Web content | Web pages and online video shown as layers | Malicious pages, drive-by downloads, access to local files | SEC-08, SAFE-02 |
| The internet | Partner APIs, the sync server, the update feed | Man-in-the-middle, tampered updates, data leaks | SEC-12, SEC-13, SEC-05 |
| Custom shaders and Content Packs | Third-party creators | GPU hangs, unsafe code, misleading content | SEC-03, SEC-15, SAFE-04 |
| The microphone | Speech cues | Private conversations recorded or sent away | SEC-13, SAFE-09 |
| People | Volunteers with the wrong role, accidental clicks, mischief | Wrong content on screen, settings changed mid-service | SEC-14, SAFE-01, SAFE-03, SAFE-06 |
| Build and CI | Strangers' pull requests and forks while public, compromised dependencies or actions | Stolen tokens, poisoned builds | SEC-09, SEC-10 |
| Installer and updater (M1) | The update feed and package host the person configured; Velopack's `Update.exe` running as the user | A tampered or swapped package; an update that interrupts a service; an installer that asks for admin | SEC-01, SEC-12, SEC-13, SAFE-06 |
| Support files (M1) | The public problem-report form where the person attaches a support bundle or a Diagnostics report | Secrets, names, email addresses or church data leaking into a public issue | SEC-05, SEC-06 |
| Library storage (M2.1) | Existing database pages, media paths and bytes on the user's disk | Corrupt or future databases reset without warning; partial writes; path traversal; damaged media reused | SEC-03, SEC-15: integrity checks, immutable migration history, bounded validation, transactional documents, content-addressed verified media |

The threat model is reviewed at the end of every milestone (TESTING-AND-QUALITY §4.2). New boundaries get a row before their feature is merged. The full review with a penetration test is M10.6 (H-21).

**M1 review (2026-11-07).** M1 added two boundaries (the last two rows). Residual risks: until signing (H-15), the update package is only as trustworthy as the feed it came from. HTTPS and the feed's SHA-256 stop tampering on the way, but not a compromised feed host, so feeds stay opt-in and set by the person. Support bundles are redacted by pattern and by registered secrets; free text that a person typed into a song or note could still name people, so the problem-report form tells them to check the file before attaching it.

## 4. Security requirements

**M2.1 storage review (2026-11-09).** Database failures retain the original file.
Documents and ordered child rows commit together; media bytes are synced and
published before metadata can reference them. Hashes, extensions and canonical
paths are checked, and streams are bounded. A saved schema fixture tests future
migration compatibility. No new IPC commands or outgoing connections are added.
Residual risks: another process with the same user's filesystem permissions can
change an open file; hash checks detect corruption but do not provide isolation
from that process. Media decoding, bundle verification, backups and restore are
still M2.6 work; storage does not decode or display imported bytes.

| ID | Requirement | Verified by | Milestones |
|---|---|---|---|
| **SEC-01** | **Least privilege.** Selah installs per user and runs without administrator rights. Nothing runs as a service or with elevation. Workers (imports, speech) run with reduced rights: a Windows job object with limits and a low-integrity token, and a macOS sandbox profile from M6. | Installer test checks for no elevation; worker token tests | M1, M6 |
| **SEC-02** | **Local IPC is private to the user.** Named pipes have a DACL for the current user only and `PIPE_REJECT_REMOTE_CLIENTS`. The server checks the peer process. Unix sockets live in a `0700` directory. Frames over 16 MiB, unknown message types and bad lengths are rejected without crashing. | `selah-ipc` tests; IPC fuzz target | M1 |
| **SEC-03** | **Outside input is untrusted.** Parsers run in workers (ADR-020) with limits on size, depth, item count and time. Archive paths are normalised (no `..`, no absolute paths). Every parser and the IPC decoder has a fuzz target: a 30-second smoke run in CI, longer runs in cloud sessions. | Fuzz targets; limit tests | M1 (IPC), M2 (bundles, Bibles, ChordPro), M6 (importers), M8 (packs) |
| **SEC-04** | **Media decoding is limited.** FFmpeg opens local files with `protocol_whitelist=file,pipe`. Network protocols are allowed only for stream items the user added. Demuxers and decoders come from an allowlist. Images are decoded with a pixel-count limit. | Tests: an `http://` path in a file item is refused; an oversized image is refused | M1, M3 |
| **SEC-05** | **Secrets live only in the OS keychain**, through one `SecretStore` interface (Windows Credential Manager with DPAPI, macOS Keychain, an in-memory fake for tests and Linux). Secrets never appear in SQLite, settings files, logs, the op log, support bundles, crash dumps or exports. | Seeded-secret tests scan every output | M1, M6, M8, M9 |
| **SEC-06** | **Logs and support bundles are redacted.** A `tracing` layer removes secrets, tokens, email addresses and the user's name in paths. Support bundles are built from redacted data only. Crash dumps stay on the computer unless the user chooses to send them. | Redaction tests with seeded data | M1, M6, M9 |
| **SEC-07** | **The gateway is safe on a shared network.** Off by default and LAN-only. TLS with a self-signed certificate pinned by the QR code. Pairing by QR. Scoped, expiring, revocable tokens. Rate limits, origin and CORS checks, request size limits. No default passwords. Every API action is recorded in the op log. | M7-A2; API fuzzing (M10.6) | M7, M10 |
| **SEC-08** | **Web content is sandboxed.** CEF runs with its sandbox on and its own profile. No file access, downloads, pop-ups or plugins. Navigation stays on the item's site. There's no script bridge into Selah. | CEF policy tests | M5 |
| **SEC-09** | **Supply chain.** Lockfiles are committed and enforced (`cargo --locked`, NuGet locked mode, `pnpm --frozen-lockfile`). `cargo deny` checks advisories, licences (no GPL) and sources. `dotnet list package --vulnerable` and `pnpm audit --prod` run in the gate. Third-party downloads are pinned by SHA-256 (`tools/third_party.lock.json`). Each release has a CycloneDX SBOM. Every new dependency is named and justified in the PR. | Gate stage 14 | M1, M9 (SBOM) |
| **SEC-10** | **CI is hardened** (ADR-024). Top-level `permissions: {}` and the least permission per job. Only GitHub-owned actions, pinned to full commit SHAs. `persist-credentials: false`. No `pull_request_target`, `workflow_run` or scheduled triggers. No secrets while the repository is public. Event text reaches scripts only through `env:`. | `tools/check-workflows.py` in the gate | M1 |
| **SEC-11** | **Memory safety.** Every Rust crate has `#![forbid(unsafe_code)]`, except the FFI crates and modules named in `tools/unsafe-allowlist.txt`, where each `unsafe` block has a `// SAFETY:` comment. C# projects don't allow unsafe blocks. | `tools/check-unsafe` in the gate | M1 |
| **SEC-12** | **Updates are authentic and never interrupt a service.** The updater downloads over HTTPS and checks each package's hash from the feed. Release builds are signed (H-15) and the updater checks the signature. No token or key is built into the app. Updates never install while anything is on the screens. The previous version can be restored. | Updater tests | M1 (hash, live deferral), M6 and M9 (signing) |
| **SEC-13** | **Privacy by default.** All outgoing network traffic goes through one gate (`selah-net`), which enforces policy and records each connection. With default settings, Selah makes **no** outgoing connections. Telemetry and crash upload are off until the user opts in. Speech audio never leaves the computer. People can export and delete their data. | Egress test (zero connections at default settings); M9-A6 | M1, M8, M9 |
| **SEC-14** | **Every command is authorised.** Each `Envelope` carries an `Actor`. Core checks a permission for every command before applying it. M1 has the framework (the local owner may do everything). Roles arrive in M4 (role lock), API scopes in M7 and organisation roles in M9. | Permission-matrix tests for every role × command | M1, M4, M7, M9 |
| **SEC-15** | **Data integrity.** Atomic writes, SQLite integrity checks, media verified by BLAKE3 hash, `.selah` bundles verified on import, Content Packs signed, and backups proven by restoring them. | Store and bundle tests | M2, M8 |

## 5. Safety requirements

| ID | Requirement | Verified by | Milestones |
|---|---|---|---|
| **SAFE-01** | **Only authorised people change the screens.** Every live change comes from an authorised actor (SEC-14). Remote devices need pairing and the right scope. Things sent from elsewhere (notes from the pastor view, **Message the booth**) go to the operator, never straight to the audience screens. | Permission and routing tests | M1, M4, M7 |
| **SAFE-02** | **Preview first.** Content from outside sources (web pages, NDI and camera inputs, speech suggestions, remote submissions, new imports) appears in preview first. It never goes to the screens automatically. | Tests for each source type | M5, M7, M8 |
| **SAFE-03** | **The emergency clear always works.** **Clear everything** and **Show logo** go from the UI straight to the engine through a priority lane, so they work even if core is frozen (ADR-026). F12 or holding Esc for 1 second triggers it. The engine acts on it within 1 frame. | Chaos test with core frozen | M1 (lane), M4 (UI) |
| **SAFE-04** | **Flash guard.** The engine checks output for general and red flashes above the WCAG 2.3.1 threshold (more than 3 flashes in any 1 second). Transitions, effects and videos that would exceed it are flagged in preview and in Ready for Sunday. Strobe-like effects ask for confirmation. | Analyser tests with known clips | M3, M8 |
| **SAFE-05** | **Safe sound.** A master limiter is on by default (−1 dBTP). Clearing audio fades instead of cutting. Volume changes are smoothed. A newly connected output starts at a safe level. | Audio tests with a null sink | M3, M5 |
| **SAFE-06** | **No surprises while live.** Quitting, signing out, changing screen settings or installing an update while something is on the screens asks first. The computer doesn't go to sleep while live. **Booth lock** (Ctrl/Cmd+L) leaves only Next, Previous and Clear working. | Tests | M1 (quit and update guards, sleep), M4 (Booth lock) |
| **SAFE-07** | **Safe mode.** After 3 crashes in 60 seconds, or if the user holds Shift while starting Selah, it starts with web layers, custom shaders, the gateway, network sources and hardware output drivers turned off. A banner says so and links to the fix article. | Watchdog tests | M1 |
| **SAFE-08** | **Approve new sources and devices.** A new NDI source, camera, MIDI device, remote device or network trigger must be approved once before it can appear on screens or control Selah. | Tests | M5, M7 |
| **SAFE-09** | **Capture is always visible.** A clear indicator shows while the camera, microphone (speech cues), recording or streaming is on. It uses an icon and a word, not colour alone. | UI tests | M5, M8 |

## 6. Rules for coding sessions

- **No secrets anywhere in the repository**, in tests, fixtures or CI. Use fakes and seeded fake values (for example `TEST-SECRET-1234`) that the redaction tests look for. `tools/check-secrets.py` runs in the gate.
- **No network in tests** (TESTING-AND-QUALITY §3, rule 9). The egress gate makes accidental calls fail the test.
- **Crypto:** use vetted libraries only (`rustls` with `aws-lc-rs` or `ring`, the OS keychain, BLAKE3 for content hashes). No home-made crypto. Random values come from the OS.
- **Errors** shown to users use catalogue codes and plain messages, never stack traces or internal paths.
- **Unsafe code** only in allowlisted FFI modules (SEC-11).
- **New dependency?** Check its licence and maintenance, run `cargo deny`, and say why it's needed in the PR description.
- **Limits everywhere** input is parsed: size, depth, count and time. Write the limit test with the parser.
- **Security and safety bugs** are P0 or P1. They block merging and get a failing test first (TESTING §3, rule 6).
- **PR descriptions** list the SEC and SAFE items the change touches and how they're tested (`.github/pull_request_template.md`).

## 7. While the repository is public

The repository is public for a limited time so that GitHub Actions minutes are free (ADR-024, [CLOUD-DEVELOPMENT.md](CLOUD-DEVELOPMENT.md)). During that time:

- Treat everything committed as **permanently public**. Forks and clones made while it's public stay public after it goes private.
- Never commit private notes, personal data, real church data, partner credentials or unlicensed material. Detailed competitive research stays outside the repository.
- Workflows use no secrets. Signing (H-15) and any secret-using job wait until the repository is private, or use OIDC with a protected environment.
- Pull requests from anyone other than the owner need approval before their workflows run.

## 8. Vulnerability response

1. Reports arrive privately through GitHub's private vulnerability reporting ([SECURITY.md](../SECURITY.md)).
2. Acknowledge within 7 days. Reproduce it with a failing test.
3. Fix in a PR marked as a security fix. Before v1.0, the fix ships in the next milestone build. After v1.0, a patch release goes to every update ring, and the release notes say what was fixed in plain words once users have had time to update.
4. If a release could harm churches (for example, a broken updater), pull it from the feed first, then fix it.

## 9. Where the M1 baseline lives

| ID | Implementation | Tests |
|---|---|---|
| SEC-01 | Velopack per-user install (`tools/package.ps1`, `packaging/velopack/`) | `tools/test-installer.ps1`: no elevation in any manifest, no service, no machine-wide entry |
| SEC-02 | `selah-ipc` (pipe DACL, remote clients rejected, peer check, `0700` socket folder, 16 MiB limit) | `selah-ipc` tests |
| SEC-03 | Frame decoder fuzz target; update feeds and support bundles parsed with limits | `fuzz_smoke`, `feeds_are_parsed_with_limits`, `responses_over_the_limit_are_refused` |
| SEC-04 | `selah-media` protocol whitelist and demuxer/decoder allowlist | `opening_an_http_url_is_refused`, `a_local_playlist_cant_reach_the_network` |
| SEC-05 | `selah-secrets` (`SecretStore`: Credential Manager, Keychain, memory) | `values_round_trip_and_are_redacted_from_logs`, `the_platform_keychain_round_trips`, the support-bundle seeded-secret test |
| SEC-06 | `selah_runtime::redact` on every log line and every support-bundle file | `seeded_secrets_and_identifiers_are_removed`, `redacting_writer_removes_seeded_secrets_before_writing`, `a_bundle_has_logs_and_versions_but_no_secrets_or_user_name` |
| SEC-09 | Lockfiles, `deny.toml`, pinned third-party files | Gate stage 14 |
| SEC-10 | `ci.yml`, `release.yml`, `bless-golden.yml` | `tools/check-workflows.py` |
| SEC-11 | `#![forbid(unsafe_code)]`, `tools/unsafe-allowlist.txt` | `tools/check-unsafe.py` |
| SEC-12 | `selah-core::update` (https feeds, size and SHA-256 checks, live deferral) | `a_package_with_the_wrong_hash_or_size_is_never_saved`, `updates_never_install_while_something_is_on_the_screens` |
| SEC-13 | `selah-net` (purposes, default-off policy, connection log); core sets the policy from settings | `the_default_policy_connects_to_nothing`, `only_selah_net_reaches_the_network`, `update_checks_are_off_at_default_settings`, `default_settings_make_no_outgoing_connections` (real processes) |
| SEC-14 | `selah-core::permissions` (every command, exhaustive) | `permission_matrix_covers_every_origin_and_command` |
| SAFE-01 | Same as SEC-14; only the local owner changes the screens in M1 | `refused_commands_change_nothing` |
| SAFE-03 | Emergency lane: UI → engine pipe, handled ahead of rendering | Emergency-lane chaos test (core frozen) |
| SAFE-06 | Quit and update guards in the UI; core refuses updates while live; `selah_runtime::power` keeps the computer awake while live | `LiveGuardTests`, `UpdateTests`, `the_computer_stays_awake_only_while_something_is_on_the_screens` |
| SAFE-07 | Watchdog safe mode (3 crashes in 60 s or Shift), banner, **Restart normally** | `three_crashes_in_a_minute_start_safe_mode`, `restart_normally_leaves_safe_mode_and_keeps_the_screens` |
