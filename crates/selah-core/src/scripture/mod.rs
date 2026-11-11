//! Scripture subsystem (M2.5): 66-book table, 6-language reference parser,
//! OSIS/USFM/Zefania bounded importers, bundled public-domain KJV/WEB seeding (H-06),
//! passage lookup, and verse-to-slide splitting.
pub mod books;
pub mod import;
pub mod reference;
pub mod split;
pub mod store;

pub use books::{BOOKS, BookSpec, book_by_code, book_by_num, book_by_osis, resolve_book_alias};
pub use import::{
    BibleImportRequest, MAX_BIBLE_FILE_BYTES, MAX_VERSES_PER_BIBLE, MAX_XML_DEPTH, MAX_XML_EVENTS,
    OwnedBibleVerse, ParsedBible, detect_bible_format, import_bible, parse_bible_bytes, parse_osis,
    parse_usfm, parse_zefania,
};
pub use reference::{
    MAX_REFERENCE_BYTES, MAX_REFERENCE_SPANS, ParsedReference, format_span, format_spans_summary,
    parse_reference,
};
pub use split::{
    DEFAULT_SCRIPTURE_LINE_CHARS, DEFAULT_SCRIPTURE_MAX_LINES, MAX_SCRIPTURE_LINE_CHARS,
    MAX_SCRIPTURE_SLIDE_LINES, MAX_VERSES_PER_SLIDE, MIN_SCRIPTURE_LINE_CHARS,
    split_scripture_slides, split_scripture_slides_with_fit,
};
pub use store::{
    KJV_TSV_GZ_SHA256, MAX_PASSAGE_VERSES, WEB_TSV_GZ_SHA256, delete_bible, ensure_bundled_bibles,
    get_passages, list_bibles,
};

use selah_proto::v1::BibleFormat;

/// Fuzzing entry point for OSIS XML parsing (SEC-03). Never panics on arbitrary bytes.
pub fn fuzz_osis(bytes: &[u8]) -> usize {
    parse_bible_bytes(bytes, BibleFormat::Osis)
        .map(|b| b.verses.len())
        .unwrap_or(0)
}

/// Fuzzing entry point for USFM parsing (SEC-03). Never panics on arbitrary bytes.
pub fn fuzz_usfm(bytes: &[u8]) -> usize {
    parse_bible_bytes(bytes, BibleFormat::Usfm)
        .map(|b| b.verses.len())
        .unwrap_or(0)
}

/// Fuzzing entry point for Zefania XML parsing (SEC-03). Never panics on arbitrary bytes.
pub fn fuzz_zefania(bytes: &[u8]) -> usize {
    parse_bible_bytes(bytes, BibleFormat::Zefania)
        .map(|b| b.verses.len())
        .unwrap_or(0)
}

/// Fuzzing entry point for the 6-language scripture reference parser (SEC-03).
pub fn fuzz_reference(bytes: &[u8]) -> usize {
    let Ok(s) = std::str::from_utf8(bytes) else {
        return 0;
    };
    parse_reference(s, None).map(|r| r.spans.len()).unwrap_or(0)
}
