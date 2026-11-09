use std::collections::HashMap;

use selah_proto::v1::Song;

pub const MAX_CHORD_LEN: usize = 32;

const SHARP_NOTES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const FLAT_NOTES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedChord {
    pub root: u8,
    pub root_is_flat: bool,
    pub suffix: String,
    pub bass: Option<(u8, bool)>,
}

fn note_to_semitone(letter: char, accidental: Option<char>) -> Option<(u8, bool)> {
    let base: i8 = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let (delta, is_flat) = match accidental {
        None => (0, false),
        Some('#') | Some('♯') => (1, false),
        Some('b') | Some('♭') => (-1, true),
        _ => return None,
    };
    Some(((base + delta).rem_euclid(12) as u8, is_flat))
}

fn normalize_note_str(s: &str) -> String {
    s.replace('♯', "#").replace('♭', "b")
}

fn parse_note_prefix(s: &str) -> Option<(u8, bool, usize)> {
    let mut chars = s.char_indices();
    let (_, letter) = chars.next()?;
    if !matches!(letter, 'A'..='G') {
        return None;
    }
    if let Some((idx, next)) = chars.next()
        && matches!(next, '#' | 'b' | '♯' | '♭')
    {
        let (semi, is_flat) = note_to_semitone(letter, Some(next))?;
        return Some((semi, is_flat, idx + next.len_utf8()));
    }
    let (semi, is_flat) = note_to_semitone(letter, None)?;
    Some((semi, is_flat, letter.len_utf8()))
}

fn parse_exact_note(s: &str) -> Option<(u8, bool)> {
    let (semi, is_flat, len) = parse_note_prefix(s)?;
    if len == s.len() {
        Some((semi, is_flat))
    } else {
        None
    }
}

fn is_valid_alteration(part: &str) -> bool {
    let clean = normalize_note_str(part);
    matches!(
        clean.as_str(),
        "2" | "4"
            | "5"
            | "6"
            | "7"
            | "9"
            | "11"
            | "13"
            | "6/9"
            | "add2"
            | "add4"
            | "add9"
            | "add11"
            | "add13"
            | "sus"
            | "sus2"
            | "sus4"
            | "maj7"
            | "maj9"
            | "maj11"
            | "maj13"
            | "M7"
            | "M9"
            | "M11"
            | "M13"
            | "no3"
            | "no5"
            | "omit3"
            | "omit5"
            | "#5"
            | "b5"
            | "+5"
            | "-5"
            | "#9"
            | "b9"
            | "+9"
            | "-9"
            | "#11"
            | "b11"
            | "+11"
            | "#13"
            | "b13"
            | "alt"
    )
}

fn is_valid_chord_suffix(raw: &str) -> bool {
    if raw.is_empty() {
        return true;
    }
    if raw.len() > MAX_CHORD_LEN {
        return false;
    }
    let s = normalize_note_str(raw);
    let mut rest = s.as_str();

    // Peel balanced parenthesized alteration groups at the end (or inline),
    // validating their contents so random parenthesized English never passes.
    if let Some(open) = rest.find('(') {
        let Some(close_rel) = rest[open + 1..].find(')') else {
            return false;
        };
        let close = open + 1 + close_rel;
        let inner = &rest[open + 1..close];
        if inner.is_empty() || inner.contains('(') {
            return false;
        }
        let valid_group = inner
            .split([',', '/'])
            .all(|tok| !tok.is_empty() && is_valid_alteration(tok));
        if !valid_group {
            return false;
        }
        let without = format!("{}{}", &rest[..open], &rest[close + 1..]);
        return is_valid_chord_suffix(&without);
    }
    if rest.contains(')') {
        return false;
    }

    // Consume leading quality keyword if present.
    let quality_prefixes = [
        "major", "minor", "maj", "min", "dim", "aug", "sus", "add", "m", "M", "Δ", "ø", "°", "+",
        "-",
    ];
    let mut consumed_quality = false;
    for prefix in quality_prefixes {
        if let Some(after) = rest.strip_prefix(prefix) {
            rest = after;
            consumed_quality = true;
            break;
        }
    }

    // Special case: `"M"` alone (rare uppercase M without digit) looks too ambiguous vs English
    // unless followed by a chord digit/extension (`CM7`, `CMaj7`).
    if consumed_quality && s == "M" {
        return false;
    }

    // Consume optional extension number (e.g., 7, 9, 6/9, 11, 13, 5, 6, 4, 2).
    for ext in ["6/9", "13", "11", "7", "9", "6", "5", "4", "2"] {
        if let Some(after) = rest.strip_prefix(ext) {
            rest = after;
            break;
        }
    }

    // Consume up to 4 trailing valid alterations (sus4, add9, b5, #9, maj7, etc.).
    let alt_tokens = [
        "maj13", "maj11", "maj9", "maj7", "add13", "add11", "add9", "add4", "add2", "sus4", "sus2",
        "sus", "omit3", "omit5", "no3", "no5", "alt", "#13", "b13", "#11", "b11", "+11", "#9",
        "b9", "+9", "-9", "#5", "b5", "+5", "-5", "M13", "M11", "M9", "M7", "+", "-",
    ];
    for _ in 0..4 {
        if rest.is_empty() {
            return true;
        }
        if let Some(matched) = alt_tokens.iter().find(|tok| rest.starts_with(*tok)) {
            rest = &rest[matched.len()..];
        } else {
            return false;
        }
    }
    rest.is_empty()
}

/// Parse a musical chord symbol (e.g. `G`, `C#m7`, `Bbadd9`, `D/F#`, `Em7(b5)`).
pub fn parse_chord(raw: &str) -> Option<ParsedChord> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_CHORD_LEN {
        return None;
    }
    let (root, root_is_flat, root_len) = parse_note_prefix(trimmed)?;
    let remainder = &trimmed[root_len..];

    // Check for optional slash bass `/X` or `/X#` / `/Xb` at the end.
    // Be careful not to split inside `6/9` unless there's another `/` after `6/9`.
    let (suffix_raw, bass) = if let Some(slash_idx) = remainder.rfind('/') {
        let candidate_bass = &remainder[slash_idx + 1..];
        if let Some(bass_note) = parse_exact_note(candidate_bass) {
            (&remainder[..slash_idx], Some(bass_note))
        } else if remainder == "6/9" || remainder.ends_with("6/9") {
            (remainder, None)
        } else {
            return None;
        }
    } else {
        (remainder, None)
    };

    if !is_valid_chord_suffix(suffix_raw) {
        return None;
    }

    Some(ParsedChord {
        root,
        root_is_flat,
        suffix: suffix_raw.to_owned(),
        bass,
    })
}

pub fn transpose_pitch(semitone: u8, semitones: i32, prefer_flats: bool) -> &'static str {
    let idx = (i32::from(semitone) + semitones).rem_euclid(12) as usize;
    if prefer_flats {
        FLAT_NOTES[idx]
    } else {
        SHARP_NOTES[idx]
    }
}

pub fn format_parsed_chord(chord: &ParsedChord, semitones: i32, prefer_flats: bool) -> String {
    let mut out = String::new();
    out.push_str(transpose_pitch(chord.root, semitones, prefer_flats));
    out.push_str(&chord.suffix);
    if let Some((bass_semi, _)) = chord.bass {
        out.push('/');
        out.push_str(transpose_pitch(bass_semi, semitones, prefer_flats));
    }
    out
}

pub fn transpose_chord(raw: &str, semitones: i32, prefer_flats: bool) -> anyhow::Result<String> {
    let parsed = parse_chord(raw).ok_or_else(|| anyhow::anyhow!("invalid chord: {raw}"))?;
    Ok(format_parsed_chord(&parsed, semitones, prefer_flats))
}

/// Parse a song key like `"G"`, `"Eb"`, `"F#m"`, `"Bb major"`, returning `(semitone, is_minor, prefer_flats)`.
pub fn parse_key(raw: &str) -> Option<(u8, bool, bool)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (root, is_flat_spelling, root_len) = parse_note_prefix(trimmed)?;
    let tail = trimmed[root_len..].trim().to_ascii_lowercase();
    let is_minor = tail == "m" || tail == "min" || tail == "minor";
    if !tail.is_empty() && !is_minor && tail != "maj" && tail != "major" {
        return None;
    }
    let natural_flat = if is_minor {
        // Minor flat keys: Dm(2), Gm(7), Cm(0), Fm(5), Bbm(10), Ebm(3), Abm(8)
        matches!(root, 0 | 2 | 3 | 5 | 7 | 8 | 10)
    } else {
        // Major flat keys: F(5), Bb(10), Eb(3), Ab(8), Db(1), Gb(6)
        matches!(root, 1 | 3 | 5 | 6 | 8 | 10)
    };
    Some((root, is_minor, is_flat_spelling || natural_flat))
}

pub fn transpose_key(raw: &str, semitones: i32, prefer_flats: Option<bool>) -> Option<String> {
    let (root, is_minor, _) = parse_key(raw)?;
    let target = (i32::from(root) + semitones).rem_euclid(12) as u8;
    let use_flats = prefer_flats.unwrap_or(if is_minor {
        matches!(target, 0 | 2 | 3 | 5 | 7 | 10)
    } else {
        matches!(target, 1 | 3 | 5 | 8 | 10)
    });
    let mut out = transpose_pitch(target, 0, use_flats).to_owned();
    if is_minor {
        out.push('m');
    }
    Some(out)
}

pub fn transpose_song(
    song: &mut Song,
    semitones: i32,
    prefer_flats: Option<bool>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        (-120..=120).contains(&semitones),
        "semitone shift must be between -120 and 120"
    );
    let step = semitones.rem_euclid(12);
    let use_flats = match prefer_flats {
        Some(v) => v,
        None => {
            if let Some(transposed) = transpose_key(&song.key, semitones, None) {
                parse_key(&transposed).is_some_and(|(_, _, flats)| flats)
            } else {
                // Infer from chords if key is unset, else flats for downward shifts.
                let mut flat_count = 0usize;
                let mut sharp_count = 0usize;
                for sec in &song.sections {
                    for line in &sec.lines {
                        for c in &line.chords {
                            if let Some(parsed) = parse_chord(&c.text) {
                                if parsed.root_is_flat {
                                    flat_count += 1;
                                } else if c.text.contains('#') {
                                    sharp_count += 1;
                                }
                            }
                        }
                    }
                }
                if flat_count > sharp_count {
                    true
                } else if sharp_count > flat_count {
                    false
                } else {
                    semitones < 0
                }
            }
        }
    };

    if step == 0 && prefer_flats.is_none() {
        return Ok(());
    }

    if !song.key.trim().is_empty()
        && let Some(new_key) = transpose_key(&song.key, semitones, Some(use_flats))
    {
        song.key = new_key;
    }

    for section in &mut song.sections {
        for line in &mut section.lines {
            for chord in &mut line.chords {
                if let Some(parsed) = parse_chord(&chord.text) {
                    chord.text = format_parsed_chord(&parsed, semitones, use_flats);
                }
            }
        }
    }
    Ok(())
}

/// Build an aligned map of Nashville / normalized chords? No—utility kept focused;
/// helper for stripping trailing/duplicate chord spacing when validating lyrics.
#[allow(dead_code)]
pub(crate) fn count_distinct_chords(song: &Song) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for sec in &song.sections {
        for line in &sec.lines {
            for chord in &line.chords {
                *counts.entry(chord.text.clone()).or_insert(0) += 1;
            }
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::new_id;
    use selah_proto::v1::{Chord, LyricLine, Section, SectionKind};

    #[test]
    fn parse_chord_accepts_worship_and_jazz_voicings_and_rejects_english_words() {
        for valid in [
            "G", "C", "D", "Em", "Am7", "F#m", "Bb", "Eb/G", "D/F#", "Gsus4", "Cadd9", "Fmaj7",
            "Bm7b5", "C#dim7", "Eaug", "Bb6/9", "G7alt", "A7(#9)", "Cmaj9/E", "D♯m7", "A♭",
        ] {
            assert!(parse_chord(valid).is_some(), "expected chord: {valid}");
        }
        for word in [
            "", "Amazing", "Grace", "And", "Blessed", "Chorus", "Verse", "a", "am", "M", "Do",
            "Far", "God", "Holy", "Christ", "I", "The",
        ] {
            assert!(parse_chord(word).is_none(), "expected non-chord: {word}");
        }
    }

    #[test]
    fn transpose_chord_handles_roots_slash_bass_and_enharmonics() {
        assert_eq!(transpose_chord("G", 2, false).unwrap(), "A");
        assert_eq!(transpose_chord("D/F#", 2, false).unwrap(), "E/G#");
        assert_eq!(transpose_chord("G/B", -2, true).unwrap(), "F/A");
        assert_eq!(transpose_chord("Cadd9", 3, true).unwrap(), "Ebadd9");
        assert_eq!(transpose_chord("F#m7", 1, false).unwrap(), "Gm7");
        assert_eq!(transpose_chord("C", -1, false).unwrap(), "B");
        assert!(transpose_chord("NotAChord", 2, false).is_err());
    }

    #[test]
    fn transpose_song_updates_key_and_all_chords_preserving_unicode_positions() {
        let mut song = Song {
            id: Some(new_id()),
            title: "Amazing Grace".into(),
            key: "G".into(),
            sections: vec![Section {
                id: Some(new_id()),
                label: "Verse 1".into(),
                kind: SectionKind::Verse as i32,
                lines: vec![LyricLine {
                    text: "’Twas grace that taught".into(),
                    chords: vec![
                        Chord {
                            position: 0,
                            text: "G".into(),
                        },
                        Chord {
                            position: 6,
                            text: "C/G".into(),
                        },
                        Chord {
                            position: 17,
                            text: "D/F#".into(),
                        },
                    ],
                }],
                ..Default::default()
            }],
            ..Default::default()
        };

        transpose_song(&mut song, 2, None).unwrap();
        assert_eq!(song.key, "A");
        let chords = &song.sections[0].lines[0].chords;
        assert_eq!(chords[0].text, "A");
        assert_eq!(chords[0].position, 0);
        assert_eq!(chords[1].text, "D/A");
        assert_eq!(chords[1].position, 6);
        assert_eq!(chords[2].text, "E/G#");
        assert_eq!(chords[2].position, 17);

        // Down 3 semitones from A -> F#? Wait: 9 - 3 = 6 (Gb/F#). With explicit flats -> Gb:
        transpose_song(&mut song, -3, Some(true)).unwrap();
        assert_eq!(song.key, "Gb");
        assert_eq!(song.sections[0].lines[0].chords[0].text, "Gb");
        assert_eq!(song.sections[0].lines[0].chords[2].text, "Db/F");
    }
}
