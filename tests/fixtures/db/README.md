# Historical library fixtures

`schema-v1.db` was saved by M2.1. It contains one invented song, an ordered section
and an arrangement that repeats the section. `m2.db` was saved by M2.9 at Milestone 2
completion with invented songs (including chords, translations, arrangements, Yoruba
and Korean lyrics), a template, media metadata, a presentation, service records, tags,
three-level policy settings, an unsaved editor buffer, help feedback, search indexes,
synonyms, op history, and active undo/redo stacks.

No real church or licensed content is present in either fixture. Both fixtures are
immutable; later builds must open and migrate every saved `*.db` fixture without losing
its content (verified in Stage 10 `migrations` and `crates/selah-core/tests/migrations.rs`).
The fixture generator in `crates/selah-core/examples/library_fixture.rs` refuses to
overwrite an existing fixture.
