# Selah: instructions for coding sessions

Selah is a cross-platform (Windows + macOS) church presentation app that must beat ProPresenter, Presenter (WorshipTools) and MediaShow at v1.0 on power, reliability and ease of use. It's built in 10 milestones, in Claude Code cloud sessions (ADR-023). Each session ends with a pull request that the user reviews and merges (ADR-025). This folder is the repository root.

## Where things are

| Need | Read |
|---|---|
| What to build, per milestone | `BUILD-PLAN.md` (only the section for the milestone you're working on) |
| What's done, what's next, resume notes | `PROGRESS.md` (always read first) |
| How the parts fit, repo map, conventions | `docs/ARCHITECTURE.md` |
| Decisions you must follow | `docs/DECISIONS.md` (ADRs) |
| Gate, regression rules, definition of done, CI | `docs/TESTING-AND-QUALITY.md` |
| Security and safety requirements (SEC, SAFE) | `docs/SECURITY.md` |
| The user's side: cloud setup, PR review, test builds | `docs/CLOUD-DEVELOPMENT.md` |
| Look, tokens, text size, components, UI wording | `docs/DESIGN-SYSTEM.md` + `design/prototype/` (open `index.html`) |
| Help articles and contextual help | `docs/KNOWLEDGE-BASE.md` + exemplars in `content/kb/en/` |
| Product reasoning (rarely needed) | `Church-Presentation-Platform-Plan.md` |

Read only what the milestone's **Read first** row lists. Don't re-plan or re-decide what these documents already settle.

## When the user says "Complete milestone N" (or "continue" or "resume")

1. **Orient.** Read `PROGRESS.md`, then the `## MN.` section of `BUILD-PLAN.md`, then its **Read first** documents.
2. **Check what's merged.**
   - Fetch `origin`. Every milestone in **Depends on** must be `Done` or `Done, H-checks pending` in PROGRESS.md **on `origin/main`**. If not, stop and tell the user which milestone must come first.
   - Check for open agent PRs: `gh api "repos/yomiade1/selah-app/pulls?state=open" --jq '.[] | "\(.number) \(.title)"'`. If one is open, stop and ask the user to review and merge it, unless they've told you to build on top of it.
3. **Check human inputs.** For any listed H-task that's still open, continue with fakes and feature flags (ADR-017), and note it in PROGRESS.md. Ask the user only for things that truly block progress.
4. **Start from green.**
   - Make sure git is clean, and start your branch from the latest `origin/main`. The session's own branch name is fine. Never work on `main`.
   - Run `tools/verify.sh --cloud`. Milestone 1 creates it, so skip this until it exists.
   - If the gate is red before you've changed anything, fix it first and log it under "Deviations".
5. **Resume, don't redo.** Skip WPs already ticked in PROGRESS.md. Read the resume notes.
6. **For each remaining WP, in order:**
   1. Implement it fully, including its tests.
   2. If it's user-facing, also do the strings, help anchors, hints, error codes, KB articles, contextual help, accessibility, and checks at 200% text and in both appearances.
   3. If it touches anything in `docs/SECURITY.md` (screens, sound, network, outside files, secrets, permissions), implement and test those SEC and SAFE items too.
   4. Run `tools/verify.sh --cloud` (full).
   5. Update `tests/ledger.json`.
   6. Tick the WP in PROGRESS.md with evidence.
   7. Commit `M<N>.<k>: <summary>` and push your branch. Pushing doesn't start CI.
7. **If you're running low on context or time:** finish or cleanly park the current WP, commit, write precise **Resume notes** in PROGRESS.md, push, and open the PR for the finished WPs (step 9). Tell the user to merge it and then say "Complete milestone N" again.
8. **Finish the milestone.** Check every acceptance criterion and record evidence. Then do the milestone-level items in TESTING-AND-QUALITY §4.2:
   - specs in `docs/specs/`;
   - CHANGELOG;
   - the `tests/fixtures/db/m<N>.db` fixture;
   - checklists for each (H) item, written for the user to run on the installed test build;
   - a review of the threat model in `docs/SECURITY.md` §3;
   - the full gate;
   - the PROGRESS.md status set to Done (or Done, H-checks pending). `release.yml` tags `m<N>` after the user merges; never create tags yourself.
9. **Open the PR** (ADR-025).
   - Write the description from `.github/pull_request_template.md` into a temporary file. Fill in every section, including "Test on your PC" and the SEC and SAFE items.
   - Run `tools/open-pr.sh --title "M<N>: <summary>" --body-file <file>`. Add `--label ci:macos` if this PR completes the milestone.
   - Run `tools/ci.sh status --wait`. If a job fails, read its annotations, fix the problem, push, and wait again. A new push cancels the older run.
   - For Windows feedback before the PR, you may run `tools/ci.sh run` once per session.
   - If the script can't reach the API, it prints a link. Give that link to the user.
10. **Report to the user:** the PR link; the CI result; the installer link (`tools/ci.sh link`, then **Artifacts**); what to test on their PC; which (H) checks and human tasks are open; any deviations; and the next step ("Merge PR #n, then say 'Complete milestone N'").

For "status" or "where are we", summarise PROGRESS.md and any open PRs; don't change code.

For "Address the review comments on PR #N": read the review and comments (`gh api repos/yomiade1/selah-app/pulls/N/reviews`, `.../pulls/N/comments` and `.../issues/N/comments`), check out the PR's branch, fix them with tests, run the gate, and push to that branch. If you can't push there, open a follow-up PR that says which PR it continues. Reply with what you changed.

## Rules that always apply

- **No regressions** (TESTING-AND-QUALITY §3):
  - never delete, skip or weaken tests or lints to pass;
  - the ledger only goes up;
  - golden images change only through `tools/bless-golden.sh --reason` or the `bless-golden.yml` workflow;
  - DB fixtures must keep migrating;
  - `buf breaking` must pass.
- **Secure and safe by default** (ADR-026, `docs/SECURITY.md`). Risky features start off. Outside input is untrusted and parsed with limits. Nothing reaches the screens or speakers without an authorised person choosing it. Every outgoing connection goes through `selah-net`; every secret goes through `SecretStore`. A failing SEC or SAFE test blocks the PR.
- **The repository may be public.** Never commit secrets, personal data (including personal email addresses), real church data, private notes or unlicensed material. `tools/check-secrets.py` runs in the gate.
- **Follow the ADRs.** To change a decision, add a new ADR and update the docs in the same commit.
- **Docs are part of done.** Help articles follow the plain-language rules (Grade ≤ 6, short sentences, "you", exact bold UI labels). Copy the exemplars' patterns.
- **Design:**
  - Use tokens only: no hard-coded colours or sizes.
  - Inter for UI, Manrope for headings.
  - All text sizes scale with the Text size setting.
  - Nothing covers the program preview or the Next and Clear controls while live.
  - No government branding.
- **Strings** live in `content/i18n/en/*.ftl`, never in code.
- **Clean room** (ADR-018): no code, assets or copy from ProPresenter, Presenter or EasyWorship, and never use unlicensed software or files.
- **No network in tests.** Downloads happen only in `tools/setup` and `tools/cloud-setup.sh`, with hashes pinned for anything that ships.
- **Git** (ADR-025): commit after each WP and push your branch. Never push to `main`, merge a PR, force-push a shared branch, delete branches you didn't create, or create tags. Open PRs only with `tools/open-pr.sh`.
- **CI** (ADR-024): no schedules, nothing long-running. Workflows must pass `tools/check-workflows.py`: GitHub-owned actions pinned by SHA, least permissions, no secrets.

## Environment

- **Cloud sessions** run Ubuntu 24.04 with 4 vCPUs, 16 GB of memory, and no GPU or display. `tools/cloud-setup.sh` provisions them; it runs as the environment's setup script and from the SessionStart hook in `.claude/settings.json`. If something's missing, run `tools/cloud-setup.sh --if-missing` with `CLAUDE_CODE_REMOTE=true` set.
- **Linux is for development and tests only** (ADR-023). Use lavapipe for rendering, Unix sockets for IPC, and fakes for Windows and macOS features. Type-check Windows code with `cargo check --target x86_64-pc-windows-gnu`. CI checks the real Windows and macOS builds.
- **GitHub:** `gh pr` and `gh issue` don't work in cloud sessions because they use GraphQL. Use `gh api` (REST), as `tools/open-pr.sh` and `tools/ci.sh` do. Job logs may be unreachable, so CI reports failures as annotations.
- **Time limits:** a foreground command stops after 10 minutes. Run long work (fuzzing, long tests) in the background, which allows up to 30 more minutes.
- **Hardware results** come from the user: they install the test build and attach **Help ▸ Diagnostics** reports to the PR.
- **Visual reference:** `design/prototype/` (static HTML). If you change it, regenerate its PNGs with headless Chromium when it's available; otherwise say in the PR that the PNGs are out of date.
- **Git:** `origin` is https://github.com/yomiade1/selah-app.git. Commits use the GitHub no-reply address that the setup script configures.
- **Local sessions** on the user's Windows PC are possible but not the norm. There, use `tools/*.ps1`, and H-01 applies.
