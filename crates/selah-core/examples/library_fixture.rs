#![forbid(unsafe_code)]
//! Create a synthetic historical schema fixture. Never overwrite an existing fixture.
use selah_core::library::Library;
use selah_proto::v1::{Arrangement, Id, LyricLine, Section, SectionKind, Song};

fn id(last: u8) -> Id {
    Id {
        value: vec![1, 2, 3, 4, 5, 6, 0x70, 0, 0x80, 0, 0, 0, 0, 0, 0, last],
    }
}

fn main() -> anyhow::Result<()> {
    let output = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("usage: library_fixture <new-fixture.db>"))?,
    );
    anyhow::ensure!(
        !output.exists(),
        "historical fixtures cannot be overwritten"
    );
    let temporary = tempfile::tempdir()?;
    let mut library = Library::open(temporary.path())?;
    let song = Song {
        id: Some(id(1)),
        title: "Synthetic migration song".into(),
        public_domain: true,
        sections: vec![Section {
            id: Some(id(2)),
            label: "Verse 1".into(),
            kind: SectionKind::Verse as i32,
            lines: vec![LyricLine {
                text: "These words exist to test a saved library".into(),
                chords: vec![],
            }],
            ..Default::default()
        }],
        arrangements: vec![Arrangement {
            id: Some(id(3)),
            name: "Main".into(),
            section_ids: vec![id(2), id(2)],
        }],
        ..Default::default()
    };
    library.save(&song)?;
    let source = library.paths().database.clone();
    // Closing the last WAL connection checkpoints the committed pages into library.db.
    drop(library);
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut source = std::fs::File::open(source)?;
    let mut target = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    std::io::copy(&mut source, &mut target)?;
    target.sync_all()?;
    Ok(())
}
