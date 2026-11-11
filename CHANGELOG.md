# Changes

## Unreleased

Fixes since Milestone 3:

- The command palette's drop shadow is now the soft elevation the design system calls for, and is gone in the Booth appearance, where shadows don't belong.

Milestone 3: the render engine, templates and the slide editor. Still a work-in-progress preview build with no live Sunday service runner yet (coming in M4).

- Multi-layer render engine & independent screens: composes Video Input, Media, Slide, Props and Messages layers at 60 Hz across independent screen resolutions with upcoming-slide pre-rasterisation.
- Templates & starter looks: edit fonts (across all 7 bundled variable fonts), weights, outlines, drop shadows, rounded card boxes with accent bars, line/letter spacing, safe margins and screen-shape variants (`16:9`, `32:9` LED, `4:3`, `9:16`), starting from 4 built-in presets (Sanctuary, Warm Serif, Lower Third, Stage Monitor) and assigning default templates per song, scripture or screen.
- Words that fit every screen: automatically shrinks long lyrics or Bible verses down to your template's minimum readable size, then cleanly splits onto extra slides before overflowing safe margins, showing a per-screen fit report (`SHRUNK_TO_FIT`, `EXTRA_SPLIT`, `AT_MIN_SIZE`) across every connected display shape.
- Backgrounds & looping media: image (`Fit`, `Fill`, `Stretch`, blur, opacity), solid/gradient backdrops, and gap-free 1080p/4K looping video backgrounds synchronised to the audio device clock with trim in/out points, speed control (`0.1×`–`4.0×`), smooth 25 ms fade-outs when cleared, and an always-on `-1 dBTP` master limiter so loud audio cannot clip through your PA.
- Transitions, built-in effects & flash protection: 8 transitions (`Cut`, `Fade`, `Dissolve`, `FadeThroughBlack`, `Push`, `Wipe`, `Zoom`, `Slide` with direction and easing) and 6 built-in visual effects (`Blur`, `ColourAdjust`, `Vignette`, `BackgroundBlur`, `ColourKey`, `Mask`), backed by an automatic WCAG 2.3.1 flashing-content analyser on imported videos, rapid transitions and live frames that requires confirmation before staging flagged items.
- Offscreen thumbnails & dual previews: real rendered thumbnails across song grids, Bible splits and editors, alongside isolated live **Program** and **Staged Next** previews.
- Custom slide editor: design custom slides with rich text, live `{clock}` and `{timer}` tokens, shapes (`Rect`, `RoundedRect`, `Ellipse`, `Line`), images and videos, using multi-select align, group/ungroup, layer order, locking and undo/redo.
- Expanded help & benchmark: 20 new M3 help articles on templates, safe areas, transitions, effects and flash warnings (57 guides total), plus the new 2×1080p + 4K video test in **Help ▸ Diagnostics**.

Milestone 2: library, songs, Scripture and the help system. Still a work-in-progress preview build, not for a live Sunday service yet.

- Songs library: create, edit, save, tag, arrange and delete songs with automatic background saving, per-song and global undo/redo, and multi-language search that tolerates typos and diacritics (tested with Yorùbá, Korean and Spanish).
- Four-format song pasting: paste plain lyrics, `[Verse]` / `[Chorus]` sections, chords above lyrics, or ChordPro `[C]` markup; transpose keys with sharp or flat spelling; and split slides cleanly by line limit or section breaks with preview across 16:9, LED 32:9, Stage and Stream views.
- Scripture offline and importable: includes the public-domain King James Version (KJV) and World English Bible (WEB), a 6-language verse reference parser (English, Spanish, Portuguese, French, Korean and Yorùbá), full-text Bible search, automatic verse-to-slide splitting, and offline import of OSIS, USFM and Zefania Bible files.
- Media bin and exchange: import images, videos and audio verified by content hash with automatic thumbnails decoded in an isolated worker process; export and import portable `.selah` bundles; and create or restore verified full-library backup archives.
- Library protection: database writes use atomic transactions, an append-only operation log, idempotent command replay, integrity checks on open, and preserved schema migrations.
- Help built into every screen: press **F1** for context-aware help, **Shift+F1** to point at any control ("What's this?"), **Ctrl+K** / **Cmd+K** to find songs, Scripture, media, commands or help articles, and read 37 built-in guides with helpfulness feedback — while **F12** (**Clear everything**) keeps working even inside help overlays.

Milestone 1: the foundations. This is a test build for checking the basics, not for a service yet.

- Selah has a Windows installer. It installs just for you, without administrator rights, and
  adds Selah to the Start menu. Uninstalling it keeps your settings and library.
- Selah can update itself, but only when you ask. Turn it on in Settings, under Updates. It
  checks each download before installing and never installs while something is on the screens.
- Selah doesn't connect to the internet unless you turn on a feature that needs it.
- Passwords and keys are kept in your computer's own secure store (Credential Manager or the
  Keychain).
- If you quit while something is on the screens, Selah asks first. Your computer doesn't go to
  sleep while something is showing.
- Clear everything (or F12) clears the screens even if part of Selah has stopped responding.
- In safe mode, Restart normally starts Selah again with everything on, without touching the
  screens.
- Help, then Report a problem, saves a support file with no passwords or names in it and opens
  the problem form for you.
- Help, then Diagnostics, shows details about your computer and runs a short speed test you
  can send to us.
- Settings has 7 text sizes and Light and Booth looks, and can follow your computer's text size.
- Help articles open inside Selah.

- Selah runs as separate programs for the screens, the data and the window you use. If one of
  them stops, Selah starts it again within 2 seconds and the screens keep showing the same thing.
- Selah can show a test pattern (colour bars, a grid or a grey ramp) on an output window.
- Words on the screens are drawn the same way on every computer, with the fonts Selah brings.
  English, Yorùbá (with tone marks), Korean and Arabic (right to left) are checked.
- If Selah crashes 3 times in a minute, it starts in safe mode.
- Selah's programs only talk to each other on your computer, and only as your user account.
- Logs leave out passwords, keys, email addresses and your user name.
- Every change is checked automatically on Windows, macOS and Linux before it's merged.
