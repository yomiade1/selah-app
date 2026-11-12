# Historical library fixtures

`schema-v1.db` was saved by M2.1. It contains one invented song, an ordered section
and an arrangement that repeats the section. `m2.db` was saved by M2.9 at Milestone 2
completion with invented songs (including chords, translations, arrangements, Yorùbá
and Korean lyrics), a template, media metadata, a presentation, service records, tags,
three-level policy settings, an unsaved editor buffer, help feedback, search indexes,
synonyms, op history, and active undo/redo stacks. `m3.db` was saved at Milestone 3
completion on top of the M2 seed with a rich role-slotted multi-variant `Template`
(outline, drop shadow, card box, safe area, `16:9` / `32:9` LED / `4:3`), default song and
scripture template policies, a `flash_warning`-flagged motion video item, and a custom
`Presentation` with strict `SlideElementsDocument` v1 slides (text with `{clock}`/`{timer}`,
rounded-rectangle shape, image, video, `fade` transition, and `vignette` effect) plus
live undo/redo history.
`m4.db` was saved at Milestone 4 completion on top of the M3 seed with a service holding
every planned item kind (countdown, header, song with key and look, scripture, media, note with
a macro, audio), a service type, a team with a Simple-mode volunteer, one look (per-screen
layers and a mask), prop, message (tokens and `{timer}`), timer (runs a macro at zero), two
macros, a clear group and a stage layout, plus the CCLI licence and default Bible settings.

No real church or licensed content is present in any fixture. All fixtures are
immutable; later builds must open and migrate every saved `*.db` fixture without losing
its content (verified in Stage 10 `migrations` and `crates/selah-core/tests/migrations.rs`).
The fixture generator in `crates/selah-core/examples/library_fixture.rs` refuses to
overwrite an existing fixture.
