# Testing, quality and regression policy

The aim is simple: **every milestone adds capability and nothing that already works breaks.** This document says how that is enforced. It implements ADR-016, ADR-017, ADR-023, ADR-024 and ADR-025.

## 1. The gate: `tools/verify`

`tools/verify.sh` (Linux and macOS) and `tools/verify.ps1` (Windows) run everything below, in order, and stop at the first failure. Exit code 0 means green. Milestone 1 creates them, and every later milestone extends them.

- In a cloud session, run `tools/verify.sh --cloud`.
- CI runs `tools/verify.sh --ci` (Linux, macOS) and `tools/verify.ps1 -CI` (Windows).

| Stage | Command (indicative) | Since |
|---|---|---|
| 1. Toolchain check | `tools/setup.sh --check` / `setup.ps1 -CheckOnly` | M1 |
| 2. Format | `cargo fmt --check`, `dotnet format --verify-no-changes`, `pnpm -r format:check` | M1 |
| 3. Lint | `cargo clippy --workspace --all-targets -- -D warnings`, C# analyzers (warnings as errors), `pnpm -r lint`, `tools/lint-strings` (no hard-coded strings, banned terms) | M1 |
| 4. Contracts | `buf lint`, `buf breaking --against .git#tag=m<N-1>` (from M2) | M1 |
| 5. Build | `cargo build --workspace --locked`, `dotnet build app/Selah.sln -c Release`, `pnpm -r build`; in the cloud also `cargo check --target x86_64-pc-windows-gnu` | M1 |
| 6. Unit and integration tests | `cargo test --workspace`, `dotnet test`, `pnpm -r test` | M1 |
| 7. Golden frames | `cargo test -p selah-testkit --features golden` (lavapipe on Linux, WARP on Windows; one reference set with per-test tolerance) | M1 |
| 8. UI tests | Avalonia headless: key flows at 100/150/200% text, Light and Booth, `en` and `en-XA` | M1 |
| 9. Knowledge base | `selah-kb-lint content/` + coverage tests (KNOWLEDGE-BASE §7) | M1 (coverage from M2) |
| 10. Migrations | Open every `tests/fixtures/db/m*.db` with the current build and check the data | M2 |
| 11. End-to-end | `tests/e2e`: headless core + engine run scripted services and assert frames and timings | M4 |
| 12. Web | Vitest + Playwright against a headless core | M7 |
| 13. Ledger | `tools/test-ledger --check`: test counts ≥ `tests/ledger.json` | M1 |
| 14. Security and licences | `cargo deny check`, `dotnet list package --vulnerable`, `pnpm audit --prod`, `tools/check-secrets.py`, `tools/check-workflows.py`, `tools/check-unsafe` | M1 (enforced from M1) |

Security and safety tests (docs/SECURITY.md) run inside stages 6, 8 and 11. They're tagged `sec` or `safe`, so the report lists them by SEC and SAFE ID.

Flags:
- `--fast` / `-Fast` skips stages 7, 8, 11 and 12, for use during a work package.
- `--release` / `-Release` turns on release-mode KB lint and stricter checks (M10).
- `--cloud` adds the Windows type-check and skips stages that need Windows or macOS.
- `--ci` / `-CI` prints failures as GitHub annotations and writes `target/verify-report/`.

Always run the full gate before ticking a work package.

## 2. Test layers

| Layer | Tools | What it covers |
|---|---|---|
| Unit | `cargo test`, xUnit, Vitest | Pure logic: parsers, slide generation, reference parsing, state machines |
| Property | `proptest`, FsCheck | Undo/redo round trips, slide fitting with random lyrics, sync convergence |
| Integration | `cargo test` with real SQLite in a temp dir; IPC loopback | Store, migrations, command bus, IPC |
| Golden frame | `selah-testkit` (offscreen wgpu: lavapipe, WARP, Metal) | Templates, text in 4 scripts, transitions mid-way, geometry, stage layouts |
| UI headless | Avalonia.Headless + Verify | Views render, flows work, no clipped critical labels, accessibility names present |
| End-to-end | `tests/e2e` | Real processes; kill and restart (chaos); latency asserts |
| Contract | buf breaking, OpenAPI schema tests, recorded fixtures for partner APIs | Compatibility, gated integrations (ADR-017) |
| Fuzz | `cargo fuzz`: a 30-second smoke run per target in CI; 10-minute runs in cloud sessions; never scheduled | Parsers and the IPC decoder never crash or hang |
| Performance | criterion benches, `selah-bench` | Budgets in ARCHITECTURE §12; CI flags > 10% regression |
| Security | Seeded-secret, egress, IPC access, permission-matrix and workflow-policy tests | SEC items (docs/SECURITY.md) |
| Safety | Emergency-lane chaos test, flash analyser, limiter, preview-first routing | SAFE items (docs/SECURITY.md) |
| Hardware (H) | Checklists in PROGRESS.md, run on the installed test build; **Help ▸ Diagnostics** reports | Real projectors, NDI networks, SDI cards, accounts |

## 3. Regression rules

1. **Never delete, skip or weaken a test to make the gate pass.** If behaviour changes on purpose, update the test in the same commit and explain why in the commit message. If it changes a documented contract, add an ADR.
2. **The test ledger only goes up.** `tests/ledger.json` stores the minimum count per suite (rust, dotnet, web, golden, ui, e2e, kb). Update it with `tools/test-ledger --update` at the end of each work package. A decrease needs an ADR.
3. **Golden images change only on purpose.** Use `tools/bless-golden.sh --reason "<why>"` (in the cloud, for the reference set) or the `bless-golden.yml` workflow (for a platform-specific image). Both append to `tests/golden/CHANGES.md`. Never bless to hide a bug.
4. **Data always migrates.** At the end of each milestone, save a populated database as `tests/fixtures/db/m<N>.db`. Every later build must open and migrate every saved fixture with no data loss (stage 10).
5. **Contracts don't break.** `buf breaking` against the previous milestone tag. The public API is versioned (`/api/v1`); breaking it requires `/api/v2`.
6. **Fix bugs with a failing test first.** Reproduce the bug in a test, then fix it.
7. **Start from green.** At the start of a session, run the gate. If it's red before you change anything, fix that first and record it in PROGRESS.md under "Deviations".
8. **`main` is always green.** Work happens on session branches. Only the user merges, through a PR whose required CI checks pass (ADR-025).
9. **No network in tests.** Partner APIs use recorded fixtures; downloads happen only in `tools/setup`.
10. **No flaky tests.** A test that fails intermittently is fixed or quarantined with an issue note in PROGRESS.md, never retried silently. Quarantine counts against the milestone's done criteria.
11. **No long-running CI.** Anything that takes more than a few minutes (extended fuzzing, long streams, soak tests) runs in a cloud session or as a Diagnostics run on the user's PC, never in CI (ADR-024).
12. **Security and safety tests are never weakened.** A failing SEC or SAFE test blocks the PR (ADR-026).

## 4. Definition of done

### 4.1 For every work package

- [ ] Code compiles with zero warnings in every language touched.
- [ ] New behaviour has tests (unit plus the most relevant higher layer).
- [ ] User-facing work:
  - strings are in `content/i18n/en/*.ftl`;
  - help anchors are declared and used;
  - every setting has hint text;
  - error codes are in `errors.yaml` with fix articles;
  - KB articles are written (`status: review`) and pass lint;
  - empty states and info buttons are in place;
  - keyboard and accessibility names are set;
  - it works at 200% text and in both appearances.
- [ ] `tools/verify.sh --cloud` passes (full, not `--fast`).
- [ ] The SEC and SAFE items the WP touches have tests, and none is weakened.
- [ ] `tests/ledger.json` is updated; PROGRESS.md has the box ticked with evidence (test names or commands); there's one commit `M<N>.<k>: <summary>`, and the branch is pushed.

### 4.2 For every milestone

- [ ] Every work package is ticked.
- [ ] Every acceptance criterion passes, with evidence recorded in PROGRESS.md. Criteria marked (H) are listed as open human tasks if not yet done.
- [ ] Specs for what was built are written in `docs/specs/` and the docs are updated.
- [ ] CHANGELOG.md is updated in plain language.
- [ ] The DB fixture `tests/fixtures/db/m<N>.db` is saved (from M2).
- [ ] The threat model (docs/SECURITY.md §3) is reviewed for anything the milestone added.
- [ ] The full gate passes in the cloud session. The PR's `guard`, `linux` and `windows` jobs pass, and `macos` passes on the PR that completes the milestone (label `ci:macos`).
- [ ] PROGRESS.md status is set to **Done**, or **Done, H-checks pending**, in that PR. After the user merges it, `release.yml` tags `m<N>` and publishes the installer.

## 5. Hardware and partner verification (H)

Each milestone lists (H) acceptance checks. They are written as step-by-step checklists in PROGRESS.md for a person with the equipment or account. Until they're done, the related feature flags stay off in release builds, and the milestone status says **Done, H-checks pending**.

Most (H) checks are done on the installed test build ([CLOUD-DEVELOPMENT.md §4](CLOUD-DEVELOPMENT.md#4-each-milestone)). Hardware measurements come from **Help ▸ Diagnostics** reports that the user attaches to the PR.

## 6. Reference hardware (H-23)

- **Windows:** a recent 8-core CPU, an NVIDIA RTX 4060-class GPU, 32 GB RAM, 2 HDMI/DP outputs.
- **Mac:** a base-model Apple Silicon (M-series) Mac with 16 GB RAM.

Performance budgets are signed off on this hardware in M10.

## 7. Continuous integration (ADR-024)

| Job | Runs on | What it does | Limit |
|---|---|---|---|
| `guard` | Every PR that's ready for review | Workflow policy, secret scan, and deciding which jobs to run | 5 min |
| `linux` | PRs that change more than documents | `tools/verify.sh --ci` | 30 min |
| `windows` | PRs that change code | `tools/verify.ps1 -CI`, then build and test the installer | 45 min |
| `macos` | PRs labelled `ci:macos` (the one that completes a milestone) | `tools/verify.sh --ci` | 45 min |

- No schedules. Pushing to a branch doesn't run CI. A session may start one extra run with `tools/ci.sh run`.
- A new push cancels the PR's older run.
- Read results with `tools/ci.sh status`, which shows failures as annotations with file and line.
- The user's side (settings, costs, going private) is in [CLOUD-DEVELOPMENT.md](CLOUD-DEVELOPMENT.md).
