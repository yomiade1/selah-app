//! Song domain logic (M2.4): chord parsing & transposition, section colours/hotkeys,
//! 4-format paste detection & parsing, and rule-based slide splitting.
pub mod chords;
pub mod parse;
pub mod sections;
pub mod split;

pub use chords::{
    MAX_CHORD_LEN, ParsedChord, parse_chord, parse_key, transpose_chord, transpose_key,
    transpose_song,
};
pub use parse::{MAX_PASTE_BYTES, ParseOutput, detect_format, fuzz_chordpro, parse_song};
pub use sections::{
    assign_default_hotkeys_and_colors, color_to_hex, preferred_hotkey,
    resolve_arrangement_sections, section_color, section_color_hex, section_kind_key,
};
pub use split::{
    DEFAULT_MAX_LINES, MAX_SLIDE_LINES, SplitOptions, format_copyright_line, generate_slides,
};
