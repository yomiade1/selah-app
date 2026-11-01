# Selah design system: USWDS + Apple hybrid

This spec turns the chosen direction (ADR-008) into rules and tokens. The visual reference is [`design/prototype/`](../design/prototype/index.html): open `index.html` in a browser. Where the prototype and this document differ, **this document wins**, and the prototype should be updated.

## 1. Principles

1. **USWDS decides meaning.** Colour semantics, alerts, tags, step indicators, summary boxes, labels, hints, focus and tables follow USWDS patterns, so states are unmistakable and accessible.
2. **Apple decides structure and feel.** Toolbars, sidebar source lists, inset grouped lists, segmented controls, switches, popovers, sheets, large titles and materials follow Apple's Human Interface Guidelines, so the app feels calm and native.
3. **Live safety first.** Nothing may cover the program preview or the Next and Clear controls during a live service, including help, tours and toasts.
4. **Plain words everywhere.** UI text follows the same plain-language rules as the knowledge base ([KNOWLEDGE-BASE.md §3](KNOWLEDGE-BASE.md#3-writing-style-guide)).
5. **No government look.** No .gov banner, no flag, no "official" wording, no federal seal-like marks.

## 2. Appearances

| Appearance | Used for | Notes |
|---|---|---|
| **Light** | Plan, Library, Edit, Settings, Help, web planning | Default for those areas |
| **Booth** (dark) | Live (Producer and Simple), stage editor, outputs while live, pastor view | Default for Live. Low glare in dark rooms. |
| **Automatic** | User choice | Follows the OS, but Live always uses Booth unless the user turns that off |
| **High contrast** | Accessibility setting | Thicker borders (2 px), no translucency, AAA text contrast where possible |

## 3. Colour tokens

Defined once in `design/tokens/tokens.json` (DTCG format) and generated into Avalonia resources, CSS variables (web apps) and Rust constants (stage display). Values come from the USWDS palette.

| Token | Light | Booth | Use |
|---|---|---|---|
| `color.bg` | `#F2F2F7` | `#000000` | Window background |
| `color.surface` | `#FFFFFF` | `#1C1C1E` | Cards, panels |
| `color.surface-2` | `#F7F8F9` | `#2C2C2E` | Sidebars, grouped rows |
| `color.line` | `#DFE1E2` | `#3A3A3C` | Hairlines |
| `color.text` | `#1B1B1B` | `#F5F5F7` | Primary text |
| `color.text-2` | `#3D4551` | `#D1D1D6` | Secondary text |
| `color.muted` | `#565C65` | `#A1A1A6` | Hints, meta (still ≥ 4.5:1) |
| `color.primary` | `#005EA2` | `#2491FF` | Primary actions, selection |
| `color.primary-dark` | `#1A4480` | `#0050D8` | Hover, pressed |
| `color.primary-darker` | `#162E51` | `#162E51` | Step numbers, process lists |
| `color.focus` | `#2491FF` | `#2491FF` | Focus ring (4 px, 2 px offset) |
| `color.live` | `#D83933` | `#FB5A47` | LIVE state, program outline, destructive |
| `color.success` | `#00A91C` (text on light: `#008817`) | `#21C834` | Next, ready, healthy |
| `color.warning` | `#FFBE2E` | `#FFBE2E` | Warnings (text on it: `#1B1B1B`) |
| `color.info` | `#00BDE3` | `#00BDE3` | Info alerts, summary box |
| `color.emergency` | `#9C3D10` | `#9C3D10` | Severe system alerts |
| Section colours | verse `#2378C3`, chorus `#C84281`, bridge `#E66F0E`, tag `#009EC1`, intro/other `#71767A` | same | Song sections (always paired with a text label) |

Rules:
- Text contrast is at least 4.5:1, and UI component contrast at least 3:1, in both appearances. A unit test checks every token pair listed in `tokens.json` under `contrastPairs`.
- Colour is never the only signal. LIVE and NEXT always have a text tag as well as the outline.
- Program is outlined in `live`, Next in `success`. This matches broadcast switchers.

## 4. Typography (ADR-009)

| Family | Use | Weights bundled |
|---|---|---|
| **Inter** | All interface text: body, controls, lists, tables | 400, 500, 600, 700, 800 (variable font preferred) |
| **Manrope** | Headings, large titles, big numbers (timers), the brand mark | 500, 600, 700, 800 |
| **JetBrains Mono** | Timecodes, durations, keyboard keys, technical values | 400, 600, 700 |
| Fallbacks | Korean: Noto Sans KR. Arabic: Noto Naskh Arabic. Then the system font. | — |

**Type scale at 100% text size** (base 15 px; Simple mode multiplies by 1.2):

| Token | Size / line height | Family, weight | Use |
|---|---|---|---|
| `type.caption` | 12.5 / 1.4 | Inter 500 | Meta under thumbnails |
| `type.tag` | 11.5 / 1 | Inter 700, uppercase, +0.4 letter spacing | USWDS tags |
| `type.footnote` | 13 / 1.45 | Inter 400 | Hints (`usa-hint`), secondary rows |
| `type.body-sm` | 14 / 1.5 | Inter 400 | Dense tables (Producer) |
| `type.body` | **15 / 1.5** | Inter 400 | Default text, inputs, rows |
| `type.label` | 14.5 / 1.4 | Inter 700 | Form labels (`usa-label`) |
| `type.button` | 14 / 1 | Inter 700 | Buttons |
| `type.body-lg` | 17 / 1.5 | Inter 500 | Emphasised text, article body in the help window |
| `type.title-3` | 20 / 1.25 | Manrope 700 | Card titles, article H2 |
| `type.title-2` | 24 / 1.2 | Manrope 700 | Section titles |
| `type.title-1` | 32 / 1.15 | Manrope 800, −0.6 letter spacing | Large titles |
| `type.display` | 44 / 1.1 | Manrope 800 | Empty states, onboarding, big timers |

No interface text is smaller than 11.5 px at 90%.

### 4.1 Text size setting

- **Steps:** Smaller 90% · **Default 100%** · Large 115% · Larger 130% · Extra large 150% · Huge 175% · Largest 200%. There's also a switch, **Use my computer's text size**, which maps the OS scale (Windows text scaling, macOS accessibility text size) onto the nearest step.
- **Scope:**
  - It changes every `type.*` token, control heights, paddings tied to text, and icon sizes next to text.
  - It does **not** change slides, outputs, stage displays (they have their own size controls) or window chrome owned by the OS.
- **Storage and policy:** It's stored in the user profile and syncs between computers from M9. Admin policy can set the default and a minimum. If a policy locks it, the control shows a lock icon with an explanation.
- **Layout behaviour:**
  - Layout breakpoints use **effective width** (window width ÷ scale).
  - Producer mode collapses the inspector into tabs below 1,100 effective px and moves the sidebar into a drawer below 900.
  - Previews shrink before any text truncates.
  - Critical labels (Next, Clear, LIVE, output health) never truncate.
- **Where:** Settings ▸ Appearance ▸ Text size in Selah, which has a live preview (prototype screen 8). Keyboard: Ctrl/Cmd + `+` / `−` / `0`.
- **Tests:** UI snapshot tests run at 100%, 150% and 200%, in both appearances and in the pseudo-locale. A test fails if any element marked `critical` is clipped or trimmed.

## 5. Space, shape, elevation, motion

- **Spacing scale (px at 100%):** 2, 4, 8, 12, 16, 20, 24, 32, 40, 48. This follows the USWDS 8-px unit with half-steps.
- **Radius:**
  - 2 tags (USWDS)
  - 6 inputs
  - 8 buttons and segmented items
  - 10 controls and selection rows
  - 14 cards and grouped lists (Apple)
  - 18 big Simple-mode buttons
  - 999 pills and status chips
- **Elevation:** light `0 1px 2px rgba(0,0,0,.06), 0 4px 16px rgba(0,0,0,.06)`. Booth uses no shadows; separation comes from surface colour.
- **Materials:** toolbars and bottom bars use translucent `surface` at 72% opacity with background blur and 180% saturation. High contrast turns translucency off.
- **Focus:** 4 px `color.focus` outline, 2 px offset, on every focusable element. It's never removed.
- **Motion:**
  - 150–250 ms ease-out for panels and popovers.
  - No motion on live-critical controls apart from the state change itself.
  - **Reduce motion** (setting, or the OS preference) turns off non-essential animation.
- **Touch targets:** at least 44 × 44 px. Simple mode's primary buttons are at least 64 px high.

## 6. Components

The "Origin" column shows which system each pattern comes from. "Prototype" points to the reference screen.

| Component | Origin | Rules | Prototype |
|---|---|---|---|
| Toolbar | Apple | Translucent; mode segmented control on the left; status chips on the right; search in the middle (Ctrl/Cmd+K) | all |
| Sidebar source list | Apple | Rounded selection (primary fill); section headers are short; status uses tags | 01, 04, 05, 08 |
| Large title + eyebrow | Apple + USWDS | Eyebrow is uppercase, primary colour, 13 px; title uses `type.title-1` | 01, 03, 04, 05, 08 |
| Segmented control | Apple | 2–7 options; used for modes, arrangement, preview target | 01, 03, 08 |
| Inset grouped list | Apple | Rows ≥ 46 px; label left, value or control right; hint as second line | 01, 03, 05, 08 |
| Switch | Apple | Green when on; always paired with a text label | 03, 05, 08 |
| Button | USWDS colours, Apple shape | Primary (primary fill); Outline; Base (neutral); Secondary (live red, destructive or Clear all); Success (Next); Unstyled (link). Height 36 (Producer), 64–80 (Simple). | all |
| Tag | USWDS | Uppercase, 2 px radius; LIVE (live), NEXT and READY (success), WARN (warning, dark text), INFO (primary), LIGHT (neutral) | all |
| Alert | USWDS | 8 px left border; icon; heading plus 1–2 sentences; actions on the right; types info, success, warning, error, emergency | 01, 02, 03, 05 |
| Summary box | USWDS | For checks and overviews ("Ready for Sunday?") | 04 |
| Step indicator | USWDS | For flows (adding a song) and service progress (Simple mode) | 02, 03 |
| Process list | USWDS | Numbered steps with a connecting line (pairing, setup) | 07 |
| Form field | USWDS | Bold label, hint under it, 40 px input with a 1 px dark border, success and error states, error text with an icon | 03, 05 |
| Table | USWDS | 2 px header rule, row hairlines, group header rows, inside an Apple card | 04 |
| Slide card | Selah | 16:9 canvas, 12 px radius, meta row (section dot, label, number); LIVE and NEXT tags and outlines | 01, 02, 03 |
| Output preview | Selah | Header with health dot, name and look tag; program uses the live outline | 01, 05 |
| Layer row | Selah | Indicator bar, name, detail, Clear button with key hint | 01 |
| Health dot / status chip | Selah | Green, amber, red, and always with text | all |
| Info button + popover | Selah (Apple popover) | Circle "i" next to a setting or panel title; the popover has a title, up to 3 short sentences and "Read more: <article title>" | 08 |
| Help panel | Apple inspector | Right side; search; "For this screen"; article view; feedback; "From your church" articles | 08 |
| Coach mark | Selah | Points at an anchor; 1–2 sentences; Next / Skip tour; never during live output | — |
| Empty state | USWDS + Apple | Illustration or icon, one sentence, primary action, "Learn how" link | — |

**Icons:** use [Lucide](https://lucide.dev) (ISC licence), bundled as SVG, 1.75 px stroke, sized 16/20/24 at 100% and scaled with Text size. Don't use SF Symbols (Apple-only licence).

## 7. Writing for the interface (microcopy)

- Use the same plain-language rules as the knowledge base (Grade 5–6, short sentences, "you", active voice).
- **Buttons** are verbs that say what happens: "Show now", "Clear everything", "Add to service". Avoid "OK", "Submit" and "Yes/No"; repeat the verb instead ("Delete song" / "Keep song").
- **Errors** say what happened, then what to do, then link to a fix: "The left projector isn't responding. Check its cable, then select Try again. [How to fix]". Never blame the user, and never show a raw exception.
- **Hints** are one sentence, under 15 words, and describe the result rather than the mechanism.
- **Consistent terms** come from `content/terms.yaml`. Use "screen" for what the audience sees, "output" in Screens settings and for IT audiences, and "slide", "song", "service", "item", "template", "look", "prop", "message" and "layer". `tools/lint-strings` flags banned synonyms.
- Use sentence case everywhere except tags. Numbers are digits. Time and date formats follow the locale.

## 8. Contextual help placement

See [KNOWLEDGE-BASE.md §5](KNOWLEDGE-BASE.md#5-contextual-help-in-the-app) for the full rules. In short: hint text under every setting, an info button where a hint isn't enough, a help panel per screen (F1), "What's this?" mode (Shift+F1), help results in the command palette, empty states, errors and checks that link to fixes, and tours only when nothing is live.

## 9. Accessibility checklist (every screen)

- [ ] Keyboard-only operation works, with a visible focus ring and a logical tab order.
- [ ] Every control has an accessible name; icons have labels; images have alt text.
- [ ] Token contrast passes, and so do custom colours (section colours are paired with text).
- [ ] Layout works at 200% text size and in the pseudo-locale, with no clipped critical labels.
- [ ] No information relies on colour alone, motion or sound.
- [ ] Screen readers announce live state changes ("Slide 4 is now on screen") through polite live regions, rate-limited.

## 10. Safety and privacy patterns

These patterns carry the safety requirements in [SECURITY.md §5](SECURITY.md#5-safety-requirements). Like the live controls, none of them may be covered by help, tours or toasts.

| Pattern | Look and behaviour | Requirement |
|---|---|---|
| **Clear everything** | Always visible in Live, next to Next. A secondary button with an error-colour outline and a clear icon. F12, or hold Esc for 1 second. No confirmation, because speed matters. | SAFE-03 |
| **Safe mode banner** | A USWDS warning alert across the top of the window: "Selah started in safe mode. Some features are off." with **Learn why** and **Restart normally**. It can't be dismissed while safe mode is on. | SAFE-07 |
| **Capture indicators** | A small pill in the toolbar for each active capture: a red dot plus the word (**Recording**, **Streaming**, **Listening**, **Camera on**). Never colour alone. It's also shown on the stage display when the speaker can be heard or seen. | SAFE-09 |
| **Approve a source or device** | An Apple-style sheet: the device's name and type, what it will be able to do, and the buttons **Allow** and **Don't allow**. Approved devices are listed in Settings, where they can be removed. | SAFE-08 |
| **Preview first** | Outside content opens in Preview with an info tag ("From the web", "From NDI"). Moving it to the screens is always the operator's action. | SAFE-02 |
| **Flashing warning** | A USWDS warning tag, **Flashing**, on items and transitions that fail the flash check, with an info button that explains the risk. Strobe-like effects ask for confirmation once per service. | SAFE-04 |
| **Booth lock** | A lock icon in the Live toolbar and a slim banner, "Booth locked". Only Next, Previous and Clear work. To unlock, hold the lock button for 2 seconds or press Ctrl/Cmd+L. | SAFE-06 |
| **Something is on the screens** | Shown when quitting, updating or changing screen settings while live: "Something is on the screens. Do you want to stop showing it?" with **Keep showing** (default) and **Clear and quit** (or **Clear and update**). | SAFE-06 |
