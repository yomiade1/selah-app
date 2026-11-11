# Building Selah with cloud sessions

Selah is built entirely in Claude Code cloud sessions (ADR-023). You don't need any developer tools on your PC. Your part is to start sessions, review pull requests, merge them, and install test builds.

This guide covers the one-time setup, what happens in each milestone, how CI is kept small, and how to make the repository public for the CI period and private again afterwards.

## 1. How it works

```
You: "Complete milestone N"  →  cloud session (Ubuntu VM)
                                  builds work packages, runs tools/verify.sh --cloud,
                                  pushes its branch, opens a pull request
                                        │
                                        ▼
                               GitHub Actions CI (ADR-024)
                               guard · linux · windows (+ macos on request)
                               Windows job uploads Selah-win-x64-Setup.exe
                                        │
                                        ▼
You: review the PR, install Setup.exe on your PC, run the checklist, merge
                                        │
                                        ▼
                               release.yml tags the milestone (m<N>) and
                               publishes a prerelease with the installer
```

Cloud sessions run on Linux, which Selah doesn't ship on. Everything is built so that it also builds and tests on Linux: Linux uses software rendering (lavapipe) and fakes for Windows and macOS features. CI then checks the real Windows and macOS builds.

## 2. One-time setup

### 2.1 The Claude Code environment

In Claude Code on the web (claude.ai/code):

1. Connect GitHub and give the Claude GitHub app access to `yomiade1/selah-app`.
2. Create an environment called **Selah** for that repository.
3. **Network access:** choose **Custom**, keep the default trusted list, and add these domains:
   - `dot.net`, `dotnet.microsoft.com`, `builds.dotnet.microsoft.com`, `ci.dot.net`, `aka.ms` (the .NET SDK)
   - `globalcdn.nuget.org` (NuGet packages)
   - `objects.githubusercontent.com`, `release-assets.githubusercontent.com`, `codeload.github.com` (pinned downloads from GitHub releases: FFmpeg, fonts, Skia)
   - `static.rust-lang.org` (Rust components)
   - `archive.ubuntu.com`, `security.ubuntu.com` (system packages)
4. **Setup script:** paste the whole of [`tools/cloud-setup.sh`](../tools/cloud-setup.sh). It installs the .NET 10 SDK, pnpm, system libraries and Rust components, and normally takes 2 to 4 minutes. The environment is cached, so later sessions start quickly.
5. Leave auto-fix for pull requests **off**. Each automatic fix would start another CI run.

The repository also has a session-start hook (`.claude/settings.json`) that runs `tools/cloud-setup.sh --if-missing`. If the setup script wasn't saved, or a tool is missing, the hook installs it.

If a session reports a blocked download, add the domain it names to the list, then start a new session.

### 2.2 GitHub settings for the public period

Set these before you make the repository public (section 3).

**Settings ▸ General**
- **Features:** turn off Wikis and Projects. Keep Issues on (for problem reports from test builds).
- **Pull Requests:** allow **merge commits** only (turn off squash and rebase merging). Turn on **Automatically delete head branches**.

**Settings ▸ Moderation options ▸ Interaction limits**
- Choose **Limit to repository collaborators**. Strangers then can't open issues or pull requests, or comment. Limits last up to 6 months; renew them if the repository stays public longer.

**Settings ▸ Actions ▸ General**
- **Actions permissions:** choose **Allow `yomiade1`, and select non-`yomiade1`, actions and reusable workflows**, then tick **Allow actions created by GitHub** and **Require actions to be pinned to a full-length commit SHA**.
  - **The GitHub checkbox is essential.** CI uses `actions/checkout`, `actions/cache`, `actions/setup-dotnet`, `actions/setup-node` and `actions/upload-artifact`, all owned by `actions`, not by you (ADR-024 requires GitHub-owned actions pinned by SHA). Leaving it unticked allows only your own actions, and every CI run then fails instantly with `startup_failure`: no jobs, no logs, and the reason only in the run page's **Annotations** box. This happened on 2026-11-11 (runs 20 and 21) and blocked PR #7.
- **Fork pull request workflows:** **Require approval for all external contributors**.
- **Workflow permissions:** **Read repository contents and packages permissions**. Turn off **Allow GitHub Actions to create and approve pull requests**.
- **Artifact and log retention:** 7 days.

**Settings ▸ Rules ▸ Rulesets ▸ New branch ruleset**
- Name: `main`. Enforcement: **Active**. Target: the default branch.
- Turn on: **Restrict deletions**, **Require a pull request before merging** (0 approvals, because you're the reviewer), **Require status checks to pass** with `guard`, `linux` and `windows`, and **Block force pushes**.
- The status checks can only be picked after CI has run once. Add them after the first agent PR.

**Settings ▸ Advanced Security** (may be called Code security)
- Turn on **Private vulnerability reporting**, **Dependabot alerts**, **Secret scanning** and **Push protection**.
- Leave **Dependabot security updates** and **version updates** off. Sessions fix advisories through the gate, so pull requests come only from sessions.
- Leave **CodeQL default setup** off. It runs every week, and this project avoids scheduled CI.

## 3. Making the repository public

### 3.1 Clean history (done)

On 2026-11-01 the repository was recreated and given a fresh history: one commit, authored with your GitHub no-reply address. Earlier drafts, which included private research notes and your personal email address, were never published in this repository.

To keep it clean:
- Every commit uses your no-reply address (`173577567+yomiade1@users.noreply.github.com`). The session-start hook sets it for cloud sessions too, if the session's git address isn't already a no-reply one.
- Private notes stay in the private folder outside the repository. `tools/check-secrets.py` blocks personal email addresses and secrets in the gate and in CI.
- If something private is ever committed, don't just delete it in a new commit, because the history and the PR diff would still show it. Ask a local session to help you remove it from the history.

### 3.2 Then switch it to public

1. Check the settings in section 2.2.
2. **Settings ▸ General ▸ Danger Zone ▸ Change repository visibility ▸ Public**.
3. From now on, treat everything committed as permanently public. Anyone can fork a public repository, and forks stay public after you make it private again.

## 4. Each milestone

1. **Start a session** in the Selah environment and say **"Complete milestone N"**. The session reads PROGRESS.md and the build plan, and does the work. Long milestones take several sessions. Each session finishes whole work packages and opens a pull request.
2. **Wait for the pull request.** At the end, the session gives you the PR link, the CI result and the installer link.
3. **Review it.** The description follows the template: what changed, the evidence, which security and safety items it touches, what to test on your PC, and anything that needs you. The **Checks** tab shows CI.
4. **Test on your PC.** Open the CI run from the Checks tab, scroll to **Artifacts**, and download `Selah-win-x64-Setup`. Unzip it and run `Setup.exe`. It installs for your user only and doesn't need administrator rights.
   - Test builds aren't signed yet (H-15). If Windows SmartScreen warns you, select **More info ▸ Run anyway**.
   - Follow the "Test on your PC" checklist in the PR.
   - To report a problem, use **Help ▸ Report a problem** in Selah. It saves a support bundle and opens the problem-report form on GitHub. **While the repository is public, anything you attach is public too.** Check the bundle before you attach it.
5. **Merge** with **Create a merge commit**. When a milestone is complete, `release.yml` tags it and publishes a prerelease with the installer on the repository's **Releases** page.
6. **Want changes?** Leave review comments, then start a session and say **"Address the review comments on PR #N"**.

Start the next session only after you've merged the open pull request. A session that finds an unmerged agent PR stops and asks you to merge it first.

## 5. How CI is kept small (ADR-024)

| What | When it runs | Time limit |
|---|---|---|
| `guard`: workflow policy, secret scan, change detection | Every PR that's ready for review | 5 min |
| `linux`: the full gate | Every PR, unless only documents and design files changed | 30 min |
| `windows`: the gate on Windows, plus the installer | Every PR that changes code | 45 min |
| `macos` | Only with the `ci:macos` label, which sessions add to the PR that completes a milestone | 45 min |
| `release.yml` | When you merge to `main`. It builds the installer only when a milestone has just been completed. | 45 min |
| `bless-golden.yml` | Only when a session starts it to add a reviewed platform-specific reference image | 30 min |

- Nothing runs on a schedule. Pushing to a branch doesn't run CI. Only pull requests do, plus at most one manual run per session.
- A new push to a PR cancels the CI run that's still going for that PR.
- Long checks never run in CI. Extended fuzzing and 30-minute stream tests run inside cloud sessions, and soak and hardware benchmarks run on your PC through **Help ▸ Diagnostics**.
- While the repository is public, standard GitHub-hosted runners are free.

## 6. Going private again

When you make the repository private (**Settings ▸ General ▸ Change repository visibility**):

- **CI minutes count.** GitHub Free includes about 2,000 minutes a month for private repositories. Windows and macOS minutes use it up faster (at the time of writing, 2× and 10×). A typical PR costs about 60 counted minutes without macOS. Check GitHub's current pricing.
- **Rulesets** aren't enforced on private repositories on GitHub Free. Sessions still never push to `main` or merge (ADR-025), but GitHub no longer enforces it.
- **Push protection** for secrets isn't available on private repositories on GitHub Free. `tools/check-secrets.py` keeps running in the gate and in CI.
- **Interaction limits** no longer matter, because only collaborators can see the repository.
- **Downloads** from Actions and Releases need you to be signed in, which still works for your own testing.
- **Updates (H-26):** Selah must never contain a GitHub token, so in-app updates can't read a private repository. Choose one:
  - (default) install test builds by hand from PRs and Releases;
  - a separate public repository that holds only installers, filled by `release.yml` using a token stored as a repository secret (allowed once the repository is private);
  - your own download host (H-16).
- **Forks** made while the repository was public stay public.

## 7. Troubleshooting

| Problem | What to do |
|---|---|
| CI didn't start | Check that the PR isn't a draft. If it says "approval required", approve it on the Actions tab. |
| The session couldn't open the PR | It gives you a link instead. Open it and select **Create pull request**. |
| The setup script failed | Check the session log for a blocked domain, add it (section 2.1), and start a new session. |
| SmartScreen blocks Setup.exe | Select **More info ▸ Run anyway**. Signed builds come with H-15. |
| You can't merge: checks are required | Wait for CI to finish, or ask a session to fix the failing check. |
