# Historical library fixtures

`schema-v1.db` was saved by M2.1. It contains one invented song, an ordered section
and an arrangement that repeats the section. No real church or licensed content
is present. The fixture is immutable; later builds must migrate it without losing
its content. `selah-core` integration tests copy it into a temporary library.

The milestone completion fixture `m2.db` will be saved after all M2 work packages
pass. The fixture generator in `crates/selah-core/examples/library_fixture.rs`
refuses to overwrite an existing fixture.
