# Selah knowledge base and contextual help

Selah ships with a complete help system that works offline. It does two jobs:

1. **A knowledge base**: articles people can search, browse, print and share.
2. **Contextual help**: the right few words at the moment someone needs them, inside the app, without leaving what they're doing.

Both come from one source, written in plain language at a **Grade 5–6 reading level**. This document is the spec and the style guide. Decisions: ADR-011 (docs-as-code), ADR-010 (localisation), ADR-008 (design).

---

## 1. Goals

- **Volunteers solve problems alone, fast.** A first-time volunteer finds the answer to a common question in under 60 seconds (tested in M10 usability studies, with at least 90% task success).
- **Help never gets in the way of a live service.** See the live-safety rules in §5.4.
- **Docs are never out of date.** Articles are versioned with the app. Screenshots are generated from the real UI. Lint fails the build when an article names a button that no longer exists.
- **Every feature is explained.** Coverage tests make sure every screen, setting and error has help (§7.2).

## 2. What "enterprise-grade" means here

| Need | How Selah meets it |
|---|---|
| Works without internet | The whole knowledge base is bundled in `kb.db` with the app; search runs locally |
| Correct for the installed version | Each article has `applies_to`; the app shows only matching articles; the website has a version switcher |
| Finds answers in people's own words | Full-text search with synonyms ("projector" finds "screen"), typo tolerance and question phrasing ("why is my screen black") |
| Right level for each person | Articles are tagged by audience (volunteer, producer, admin, IT) and mode; Simple mode shows volunteer articles first |
| The church's own procedures | Admins write **church articles** (for example "Our Sunday checklist"), pin them and share them with all volunteers. They're labelled "From your church" (M9). |
| Control | Admin policy can hide articles for features the church has turned off (for example streaming) |
| Training | Collections export to PDF, such as a Volunteer handbook with church articles included. Practice mode and guided tours let people learn safely. |
| Accessibility | WCAG 2.1 AA; follows the Text size setting; screen-reader friendly structure; alt text required |
| Languages | English, Spanish, Portuguese, French, Korean, Yorùbá at v1.0; falls back to English with a notice |
| Improvement loop | "Did this help?" votes and searches with no results are stored locally. They're shared only if the church opts in, and give admins and the content team insight. |
| Governance | Owners per area, required second-person review ("2i") before publishing, review dates, release gate on coverage |
| Deep links | `selah://help/<id>` opens an article from email or training material; "Copy link" on every article |

---

## 3. Writing style guide

Selah's help follows the **content design** approach that Sarah Winters (formerly Richards) set out at the UK Government Digital Service and in her book *Content Design*:
- Start from what the user needs to do.
- Write the way people read: they scan, so put the important words first.
- Use plain, common words.
- Keep everything short and in chunks.
- Have a second person check every piece (2i).

### 3.1 The rules

| # | Rule | Check |
|---|---|---|
| R1 | **Start with a user need.** Every article has `user_need: "As a <role>, I want to <task> so that <goal>."` If you can't write one, don't write the article. | lint: field required |
| R2 | **Reading level Grade 5–6.** Flesch-Kincaid grade ≤ 6.0 for English. Bold UI labels, keys, code and terms marked `readability_ignore` in `content/terms.yaml` are left out of the calculation. | lint: error |
| R3 | **Short sentences.** Average ≤ 14 words. No sentence over 25 words. | lint: error |
| R4 | **Short paragraphs.** At most 3 sentences or 60 words. | lint: error |
| R5 | **Front-load.** Put the most important words first in titles, headings and sentences. Titles are 8 words or fewer. | lint: title length; review |
| R6 | **Talk to the reader.** Use "you", active voice and present tense. "Select **Next slide**," not "The Next slide button should be clicked." | review; lint warns on "is/are/was … by" passives |
| R7 | **One action per step.** Number the steps. Say where, then what: "In **Today's service**, select the song." At most 10 steps; split longer tasks. | lint: step count, step ≤ 25 words |
| R8 | **Use the words on the screen.** UI labels are bold and match the interface exactly (`**Clear everything**`). | lint: label must exist in `content/i18n/en/*.ftl` |
| R9 | **Use device-neutral verbs.** "Select" (not "click" or "tap"), "press" for keys, "type", "choose". | lint: banned words |
| R10 | **No jargon without help.** Explain technical words the first time, or link to the glossary: "NDI (a way to send video over your network)". | lint: terms marked `explain` need a glossary link or an explanation in brackets |
| R11 | **No FAQs.** Write tasks and fixes instead; FAQs hide answers. | lint: banned headings ("FAQ", "Frequently asked") |
| R12 | **Don't make people feel bad.** No "simply", "just", "easy", "obviously" or "of course". | lint: banned words |
| R13 | **Plain words.** Use the word list below. No Latin abbreviations (e.g., i.e., etc., via). | lint: banned words |
| R14 | **Inclusive and neutral.** "They" for a person; roles like "pastor or speaker" and "worship leader"; no gendered defaults; works across church traditions. | lint: word list; review |
| R15 | **Numbers as digits.** "3 screens", "5 minutes". | review |
| R16 | **Descriptive links.** Link text says where it goes. Never "click here" or "read more" on its own. | lint |
| R17 | **Alt text on every image.** Describe what matters in the image, not "screenshot". | lint |
| R18 | **Sentence case** for titles and headings. | lint |

### 3.2 Word list (use the plain word)

| Instead of | Write |
|---|---|
| click, tap, hit | select / press |
| utilise, leverage | use |
| configure | set up / change |
| navigate to | go to |
| ensure | make sure |
| prior to | before |
| commence, initiate | start |
| terminate, abort | stop / end / cancel |
| execute | run / do |
| modify | change |
| populate | fill in |
| sufficient | enough |
| approximately | about |
| additional | more / extra |
| assistance | help |
| functionality | feature / what it does |
| in order to | to |
| therefore | so |
| please note | (delete it and say the thing) |
| master / slave | primary / backup |
| e.g. / i.e. / etc. / via | for example / that is / and so on / through |

The machine-readable list is `content/terms.yaml`. It also holds Selah's preferred product terms (screen, output, slide, song, service, item, template, look, prop, message, layer) and banned synonyms for them.

### 3.3 Before and after

> **Before (Grade 11):** "In order to configure edge blending functionality, navigate to the Screens configuration panel and ensure that the projector overlap values are populated appropriately."
>
> **After (Grade 4):** "To blend 2 projectors into one picture, go to **Screens**. Then set how much the pictures overlap."

> **Before:** "Error 0x8007: NDI source unavailable."
>
> **After:** "Selah can't find the camera on your network. Check that the camera is on, then select **Try again**. [Fix: a network camera doesn't show up]"

---

## 4. Article types and templates

Every article is one of these types. Templates live in `content/kb/_templates/`.

| Type | Answers | Title pattern | Structure |
|---|---|---|---|
| `task` | "How do I…?" | Verb first: "Show a song on the screens" | Short intro (1–2 sentences) → **Steps** (numbered) → optional tip → "If it doesn't work" → Related |
| `concept` | "What is…? Why…?" | "What a look is" / "How songs fit every screen" | What it is (1 paragraph) → Why it helps → an example → What to do next (links to tasks) |
| `troubleshooting` | "Something's wrong" | Describe the symptom: "The screens are black" | "Try these in order" (numbered checks, quickest first) → Still not working (escalate) → Related. Lists `error_codes`. |
| `reference` | "What are all the…?" | "Keyboard shortcuts", "Settings for screens" | Tables, generated where possible (shortcuts and settings come from code) |
| `tip` | One quick idea, for Simple mode and popovers | Short statement | 1–2 sentences, ≤ 40 words |
| `checklist` | "Am I ready?" | "Before the service starts" | Checkbox list, each item links to a task |
| `glossary` | One term | The term | 1–3 sentences, an example, related tasks |

Collections (Volunteer handbook, Producer guide, Admin and IT guide, Moving from another app) are defined in `content/kb/<lang>/collections.yaml`.

---

## 5. Contextual help in the app

### 5.1 Five levels of help

| Level | What | Where it appears | Source |
|---|---|---|---|
| **L0 Hint** | One sentence, ≤ 15 words, always visible | Under every setting and form field (USWDS hint) | `content/i18n/<lang>/*.ftl` keys ending `.hint` |
| **L1 Popover** | Title + up to 3 short sentences + "Read more: <article title>" | Info button (ⓘ) next to settings and panel titles, and in "What's this?" mode | Article `popover` field (or `summary`) |
| **L2 Article** | A full task, concept or fix | Help panel (F1), Help window, web docs | Article body |
| **L3 Collection** | A guide made of articles | Help home, PDF export | `collections.yaml` |
| **Guided** | Tours and practice | Coach marks, practice mode | `content/help/tours/*.yaml` |

### 5.2 Where help appears ("intuitively positioned")

| Moment | Help shown | Component |
|---|---|---|
| Looking at a setting | Hint under it; ⓘ if the hint isn't enough | `HintText`, `InfoButton` |
| Unsure about a panel | ⓘ in the panel header opens the help panel filtered to that panel | `InfoButton` → `HelpPanel` |
| Unsure about a screen | **Help** (F1) opens "For this screen": articles whose `context` matches the visible anchors, best first | `HelpPanel` |
| "What does this do?" | **What's this?** (Shift+F1), then select any control to see its popover | `WhatsThisOverlay` |
| Searching | Ctrl/Cmd+K shows help results under commands and content; questions are understood | Command palette |
| Nothing there yet | Empty states say what goes here, offer the main action, and link "Learn how" | `EmptyState` |
| Something went wrong | Alerts say what happened and what to do, with a **How to fix** link to the article for that error code | `ErrorAlert` |
| Before a service | Each "Ready for Sunday?" check that fails links to its fix | Summary box rows |
| First time using a feature | One coach mark, shown once; it can be dismissed and opened again from Help | `CoachMark` |
| Learning | **Start tour** and **Practice mode** (nothing goes to the real screens) | Tours, practice mode |
| Simple mode | **Quick tips** (1 line), help panel filtered to volunteer articles, **Message the booth** to get a person | Simple mode bar |
| Phones and tablets | "?" in the web remote opens a short web version of the article | Gateway `/help/<id>` |
| After an import | The import report links each warning to its fix | Import report |

### 5.3 Rules for every screen (part of the definition of done)

1. Every interactive region declares a **help anchor**, for example `help:Help.Anchor="live.layers"`. Anchors are listed in `content/help/anchors.yaml` with a description.
2. Every setting has a hint, and an article that lists it in `settings`.
3. Every error the screen can show has a catalogue code linked to a troubleshooting article.
4. Every empty state has a sentence, an action and a "Learn how" link.
5. The screen's help panel shows at least one article for that screen.

### 5.4 Live-safety rules

- Help never opens by itself while anything is live.
- The help panel and popovers never cover the program preview, the Next button or Clear controls. In Live, help opens as a narrow side panel or in its own window.
- Keyboard shortcuts for live control keep working while help is open; help search doesn't take Space, arrows or F9–F12.
- Tours and coach marks never run while outputs show live content, unless practice mode is on.
- External links open the browser only after confirmation while live.

---

## 6. Technical design

### 6.1 Files

```
content/
  kb/
    _templates/{task,concept,troubleshooting,reference,tip,checklist,glossary}.md
    en/
      collections.yaml
      getting-started/  live/  songs/  scripture/  services/  templates/  media/
      screens/  stage/  remote/  show-control/  broadcast/  import/  team/  admin/
      troubleshooting/  reference/  glossary/
    es/ pt/ fr/ ko/ yo/          same ids, translated (M10)
    media/                       hand-made images; generated/ for UI screenshots
  help/anchors.yaml              anchor registry
  help/tours/*.yaml              guided tours
  errors/errors.yaml             error catalogue
  terms.yaml                     preferred terms, banned words, readability ignores
```

### 6.2 Front matter

```yaml
---
id: live.show-a-song              # stable; matches the path (live/show-a-song.md)
title: Show a song on the screens
type: task                        # task | concept | troubleshooting | reference | tip | checklist | glossary
status: published                 # draft | review | published (only published ships in release builds)
audience: [volunteer, producer]   # volunteer | producer | admin | it
modes: [simple, producer]
context: [live.service-order, live.slides]   # help anchors where this article is offered
settings: []                      # setting keys this article explains
error_codes: []                   # e.g. [SEL-OUT-0001]
summary: Pick the song, then select a slide. It shows on the screens straight away.   # ≤ 25 words
popover: ""                       # optional ≤ 50 words; defaults to summary
user_need: As a volunteer, I want to put song words on the screens so that people can sing along.
keywords: [lyrics, words, project, display, put up]
related: [live.clear-the-screens, songs.add-a-song]
collections: [volunteer-handbook]
applies_to: ">=1.0"
platforms: [windows, macos]
last_reviewed: 2026-10-30
reviewed_by: ""                   # second person (2i); required for status: published in release builds
---
```

Translated files add `translation_of: <id>` and `source_hash: <sha256 of the English body>`, so stale translations are detected.

### 6.3 Markdown conventions

| Write | Meaning | Rendered as |
|---|---|---|
| `**Next slide**` | A UI label (bold is only for UI labels) | Bold, checked against the strings |
| `[[Space]]`, `[[Ctrl]]+[[K]]` | Keyboard keys | Key caps (platform-aware: Ctrl ↔ Cmd) |
| `[Clear the screens](kb:live.clear-the-screens)` | Link to an article | In-app link |
| `[Open Screens settings](app:settings.screens)` | Deep link into the app | Opens that screen |
| `[Try it in practice mode](tour:practice.show-a-song)` | Starts a tour or practice | Button |
| `> [!TIP]`, `> [!NOTE]`, `> [!WARNING]` | Callouts | USWDS alert (info, info, warning) |
| `![Alt text](media:live/next-slide.png)` | Image | Image (alt required) |
| `1.` numbered list | Steps | Numbered steps (USWDS process style) |

Raw HTML isn't allowed.

### 6.4 Build and runtime

- `selah-kb build` validates everything and writes `kb.db`. It contains:
  - FTS5 tables: Porter tokenizer for English, unicode61 for other languages. Weights: title 10, keywords 6, summary 4, body 1.
  - A trigram table on titles and keywords, for typos.
  - Synonyms taken from `terms.yaml` and keywords.
  - An anchor-to-article map.
  - The article AST as JSON.
  - Collections.
- Core serves it over `help.proto`: `SearchHelp(query, lang, role, mode, anchors[])`, `GetArticle(id, lang)`, `HelpForContext(anchors[])` and `RecordFeedback`.
  - Questions are normalised: "how do I", "why is my" and similar are removed before ranking.
  - Articles matching the visible anchors get a boost.
  - Target: ≤ 30 ms per query.
- The UI renders the AST with native Avalonia controls, never a web view, so the Text size setting, appearances and accessibility all work.
- Church articles (M9) are stored in `library.db`, edited in the app with the same Markdown subset and lint, synced, and merged into search results.
- The website comes from the same source, built as static HTML with the USWDS + Apple styles, a version switcher and the same search index (as WASM or JSON).
- PDF export renders a collection plus church articles to a printable handbook.

### 6.5 Tours

```yaml
id: tour.simple-mode-basics
title: Run slides in 5 minutes
mode: simple
requires_practice_mode: true
steps:
  - anchor: simple.now-showing
    text: This is what people see on the screens now.
  - anchor: simple.next-button
    text: Press Space or select Next slide to show the next slide.
    wait_for: live.next            # continues when the person does it
```

---

## 7. Quality gates

### 7.1 `selah-kb-lint` (runs in `tools/verify`)

- **Errors (block the build):**
  - Schema is invalid.
  - The id is duplicated or doesn't match the path.
  - Any of rules R1–R4, R7–R9, R11–R13, R16–R18 is broken.
  - A link is broken.
  - An image is missing or has no alt text.
  - An unknown anchor, error code or setting is referenced.
  - The summary is over 25 words; a popover is over 50 words; a tip is over 40 words; an article is over 600 words (split it).
- **Warnings:**
  - Passive voice (R6).
  - `last_reviewed` is older than 12 months.
  - An article is an orphan (no related links and no collection).
  - A translation is stale.
- **Planned items:** articles can link to articles, anchors and settings that later milestones will build. List them in `content/kb/planned.yaml` (`id`, `kind: article | anchor | setting | error`, `milestone: M5`).
  - A reference to a planned item is a **warning** while its milestone isn't finished.
  - It becomes an **error** once that milestone is marked Done in PROGRESS.md, or in release mode, so planned items can't linger.
  - Coverage tests skip planned anchors and settings.
- **Release mode (`--release`, used in M10):** drafts are errors; empty `reviewed_by` is an error; stale or missing translations for launch languages are errors; any planned item still referenced is an error.
- **Readability calculation:**

  ```
  grade = 0.39 × (words ÷ sentences) + 11.8 × (syllables ÷ words) − 15.59
  ```

  - Syllables are counted by a heuristic plus an exceptions dictionary (`crates/selah-kb-lint/data/syllables.txt`).
  - Bold UI labels, keys, code and `readability_ignore` terms are removed first.
  - For other languages, R2 is checked by a native reviewer using the same brief, because Flesch-Kincaid is English-only.

### 7.2 Coverage tests (block the build from M2)

1. Every anchor in `anchors.yaml` is used in the UI, and every anchor used in the UI is declared.
2. Every anchor has at least one published article that lists it in `context`.
3. Every error code in `errors.yaml` has a troubleshooting article.
4. Every setting has a `.hint` string and an article that lists it in `settings`.
5. Every tour step points to a real anchor.
6. Every bold UI label in an article exists in the English strings.

### 7.3 Human review (2i)

A second person reads every article before it's published. They check that it's correct (they try the steps in the app), that it meets the user need, and that it's plain. AI-drafted articles are marked `status: review` with empty `reviewed_by` until a person signs off (human task H-18). The release gate (M10) requires 100% of articles reviewed.

---

## 8. Workflow

1. **Building a feature?** Add its anchors to `anchors.yaml`, its strings and hints to `en/*.ftl`, its error codes to `errors.yaml`, and its articles in the same work package. Docs are part of done.
2. Run `selah-kb-lint` locally. The coverage tests will tell you what's missing.
3. Draft articles go in as `status: review`. A content designer does the 2i and sets `published`.
4. At release, screenshots are regenerated by UI automation, translations are refreshed, and the website is built.

---

## 9. Article inventory by milestone

These counts are approximate, and the coverage tests are the real measure of done. Each milestone must include at least the topics listed.

| Milestone | About | Must include |
|---|---|---|
| M1 | 3 exemplars + 2 + templates | `live.show-a-song` (task), `screens.what-edge-blending-is` (concept), `troubleshooting.screens-are-black` (troubleshooting); `troubleshooting.selah-started-in-safe-mode`; `getting-started.report-a-problem` |
| M2 | 35 | Add a song by pasting; edit sections; chords and chord charts; arrangements; find a song; add scripture; Bible translations; import a Bible file; add pictures and videos; tags; undo and redo; back up and restore your library; command palette; get help in Selah; what Selah saves automatically |
| M3 | 20 | What a template is; make a template; text that fits every screen; backgrounds; video backgrounds and looping; transitions; effects; make a custom slide; the slide editor; copyright lines; flashing warnings |
| M4 | 45 | Your first service; plan a service; service types; reuse last week's service; run a service in Simple mode; Producer mode tour; go to the next slide; clear the screens; looks; props; messages; timers and countdowns; macros; clear groups; stage display; chords on stage; Ready for Sunday checks; practice mode; print a service; keyboard shortcuts; clear the screens in an emergency; lock the booth |
| M5 | 30 | Set up your screens; screen profiles; edge blending; corner pin; masks; colour matching; LED walls; test patterns; NDI in and out; cameras; Spout and Syphon; web pages on screen; audio outputs and delay; audio playlists; output health; approve a new source or device |
| M6 | 25 | Move from ProPresenter; from EasyWorship; from Presenter; from MediaShow; from other apps; import PowerPoint, PDF, Keynote and Google Slides; launch PowerPoint from Selah; read the import report; fix import problems |
| M7 | 30 | Use your phone as a remote; pair a device; pastor view; stage display on any screen; API basics (IT); Companion and Stream Deck; MIDI; OSC; lighting (DMX/sACN); timelines; timecode; backup computer; campus feeds; keep remote control safe |
| M8 | 25 | SDI outputs; fill and key; record the service; stream the service; multiview; lower thirds for the stream; scripture from speech; translation lines; Content Packs; custom effects (shader SDK, IT); recording, streaming and listening lights |
| M9 | 35 | Sign in; your church account; campuses and rooms; roles and permissions; single sign-on (IT); user provisioning (IT); audit log; policies; CCLI SongSelect; CCLI reporting; Planning Center; MultiTracks and Loop; licences and plans; install for many computers (IT); updates and update rings; privacy and data; support bundle; write church articles |
| M10 | 15 + all translations | Glossary (complete); keyboard shortcuts (generated); settings reference (generated); collections (Volunteer handbook, Producer guide, Admin and IT guide, Moving from another app); "What's new in 1.0"; every article reviewed (2i), translated and with fresh screenshots |

## 10. Exemplars

The articles written with this plan set the bar for tone and structure. Copy their patterns:

- [`content/kb/en/live/show-a-song.md`](../content/kb/en/live/show-a-song.md): task
- [`content/kb/en/screens/what-edge-blending-is.md`](../content/kb/en/screens/what-edge-blending-is.md): concept
- [`content/kb/en/troubleshooting/screens-are-black.md`](../content/kb/en/troubleshooting/screens-are-black.md): troubleshooting
