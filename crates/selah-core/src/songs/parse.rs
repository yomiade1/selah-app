use std::collections::HashMap;

use anyhow::{bail, ensure};
use selah_proto::v1::{
    Arrangement, Chord, Id, LyricLine, Section, SectionKind, Song, SongPasteFormat,
};

use super::chords::parse_chord;
use super::sections::assign_default_hotkeys_and_colors;
use crate::library::{MAX_CHILDREN, new_id};

pub const MAX_PASTE_BYTES: usize = 512 * 1024;
pub const MAX_PASTE_LINES: usize = 4096;
pub const MAX_LINE_BYTES: usize = 4096;
pub const MAX_SECTIONS: usize = 256;
pub const MAX_LINES_PER_SECTION: usize = 512;
pub const MAX_CHORDS_PER_LINE: usize = 64;
pub const MAX_DIRECTIVE_LEN: usize = 1024;
pub const MAX_BLOCK_DEPTH: usize = 4;

#[derive(Clone, Debug)]
pub struct ParseOutput {
    pub song: Song,
    pub detected_format: SongPasteFormat,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug)]
struct RawSection {
    kind: SectionKind,
    label: String,
    explicitly_numbered: bool,
    explicit_marker: bool,
    lines: Vec<LyricLine>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChordLineConfidence {
    None,
    AmbiguousSingleA,
    Strong,
}

fn push_warning(warnings: &mut Vec<String>, msg: impl Into<String>) {
    if warnings.len() < 32 {
        warnings.push(msg.into());
    }
}

fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_owned();
    }
    let mut out = String::with_capacity(line.len());
    for ch in line.chars() {
        if ch == '\t' {
            out.push_str("    ");
        } else {
            out.push(ch);
        }
    }
    out
}

fn sanitize_lines(raw: &str) -> anyhow::Result<Vec<String>> {
    ensure!(
        raw.len() <= MAX_PASTE_BYTES,
        "pasted song exceeds {MAX_PASTE_BYTES} bytes"
    );
    ensure!(!raw.contains('\0'), "pasted song contains a NUL byte");
    let stripped = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);
    let normalized = stripped.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    for line in normalized.split('\n') {
        ensure!(
            lines.len() < MAX_PASTE_LINES,
            "too many lines in pasted song"
        );
        ensure!(
            line.len() <= MAX_LINE_BYTES,
            "song line exceeds {MAX_LINE_BYTES} bytes"
        );
        lines.push(expand_tabs(line));
    }
    Ok(lines)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SectionSpec {
    Parsed {
        kind: SectionKind,
        label: String,
        explicitly_numbered: bool,
    },
}

fn parse_label_parts(raw_label: &str, allow_bare: bool) -> Option<SectionSpec> {
    let trimmed = raw_label.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();

    // Bare single-word or "<word> <number>" section keywords.
    let split_num = |prefixes: &[&str]| -> Option<(bool, Option<u32>)> {
        for &p in prefixes {
            if lower == p {
                return Some((false, None));
            }
            if let Some(rest) = lower.strip_prefix(p) {
                let tail = rest.trim();
                if let Ok(n) = tail.parse::<u32>()
                    && (1..=99).contains(&n)
                {
                    return Some((true, Some(n)));
                }
                // Handle compact forms like V1, V2, C1, B2 when bracketed/colon-suffixed
                if !allow_bare
                    && let Ok(n) = rest.parse::<u32>()
                    && (1..=99).contains(&n)
                {
                    return Some((true, Some(n)));
                }
            }
        }
        None
    };

    if let Some((explicit, num)) = split_num(&["verse", "v"]) {
        if allow_bare && lower.starts_with('v') && !lower.starts_with("verse") {
            // Bare "v" / "v1" without brackets or colon is too ambiguous; require [V1] or V1:
        } else {
            let label = match num {
                Some(n) => format!("Verse {n}"),
                None => "Verse".to_owned(),
            };
            return Some(SectionSpec::Parsed {
                kind: SectionKind::Verse,
                label,
                explicitly_numbered: explicit,
            });
        }
    }

    if let Some((explicit, num)) = split_num(&["pre-chorus", "prechorus", "pre chorus", "pc"])
        && (!allow_bare || !lower.starts_with("pc"))
    {
        let label = match num {
            Some(n) => format!("Pre-Chorus {n}"),
            None => "Pre-Chorus".to_owned(),
        };
        return Some(SectionSpec::Parsed {
            kind: SectionKind::Other,
            label,
            explicitly_numbered: explicit,
        });
    }

    if let Some((explicit, num)) = split_num(&["chorus", "refrain", "c", "ch"])
        && (!allow_bare || lower.starts_with("chorus") || lower.starts_with("refrain"))
    {
        let label = match num {
            Some(n) => format!("Chorus {n}"),
            None => "Chorus".to_owned(),
        };
        return Some(SectionSpec::Parsed {
            kind: SectionKind::Chorus,
            label,
            explicitly_numbered: explicit,
        });
    }

    if let Some((explicit, num)) = split_num(&["bridge", "br", "b"])
        && (!allow_bare || lower.starts_with("bridge"))
    {
        let label = match num {
            Some(n) => format!("Bridge {n}"),
            None => "Bridge".to_owned(),
        };
        return Some(SectionSpec::Parsed {
            kind: SectionKind::Bridge,
            label,
            explicitly_numbered: explicit,
        });
    }

    if let Some((explicit, num)) = split_num(&["tag", "coda", "vamp", "hook", "t"])
        && (!allow_bare || lower.starts_with("tag") || lower.starts_with("coda"))
    {
        let base = if lower.starts_with("coda") {
            "Coda"
        } else if lower.starts_with("vamp") {
            "Vamp"
        } else if lower.starts_with("hook") {
            "Hook"
        } else {
            "Tag"
        };
        let label = match num {
            Some(n) => format!("{base} {n}"),
            None => base.to_owned(),
        };
        return Some(SectionSpec::Parsed {
            kind: SectionKind::Tag,
            label,
            explicitly_numbered: explicit,
        });
    }

    if let Some((explicit, num)) = split_num(&["intro", "introduction", "in", "i"])
        && (!allow_bare || lower.starts_with("intro"))
    {
        let label = match num {
            Some(n) => format!("Intro {n}"),
            None => "Intro".to_owned(),
        };
        return Some(SectionSpec::Parsed {
            kind: SectionKind::Intro,
            label,
            explicitly_numbered: explicit,
        });
    }

    if let Some((explicit, num)) = split_num(&["outro", "ending", "end", "out", "o"])
        && (!allow_bare || lower.starts_with("outro") || lower.starts_with("ending"))
    {
        let base = if lower.starts_with("ending") {
            "Ending"
        } else {
            "Outro"
        };
        let label = match num {
            Some(n) => format!("{base} {n}"),
            None => base.to_owned(),
        };
        return Some(SectionSpec::Parsed {
            kind: SectionKind::Outro,
            label,
            explicitly_numbered: explicit,
        });
    }

    if let Some((explicit, num)) = split_num(&["interlude", "instrumental", "inst", "turnaround"])
        && (!allow_bare || lower.starts_with("interlude") || lower.starts_with("instrumental"))
    {
        let base = if lower.starts_with("instrumental") || lower.starts_with("inst") {
            "Instrumental"
        } else if lower.starts_with("turnaround") {
            "Turnaround"
        } else {
            "Interlude"
        };
        let label = match num {
            Some(n) => format!("{base} {n}"),
            None => base.to_owned(),
        };
        return Some(SectionSpec::Parsed {
            kind: SectionKind::Other,
            label,
            explicitly_numbered: explicit,
        });
    }

    None
}

/// Parse a standalone section marker line (`[Verse 1]`, `Chorus:`, `{c: Bridge}`, `Verse 2`, etc.).
fn parse_section_marker(line: &str) -> Option<SectionSpec> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.len() > 80 {
        return None;
    }

    // [Section Label] (ensure not a chord token)
    if let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let inner = inner.trim();
        if inner.is_empty() || parse_chord(inner).is_some() {
            return None;
        }
        if let Some(spec) = parse_label_parts(inner, false) {
            return Some(spec);
        }
        if inner
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        {
            return Some(SectionSpec::Parsed {
                kind: SectionKind::Other,
                label: inner.to_owned(),
                explicitly_numbered: false,
            });
        }
        return None;
    }

    // Section Label: (only when ending with `:`)
    if let Some(prefix) = trimmed.strip_suffix(':') {
        let prefix = prefix.trim();
        if let Some(spec) = parse_label_parts(prefix, false) {
            return Some(spec);
        }
        return None;
    }

    // Bare standalone label (`Verse 1`, `Chorus`, `Bridge`, `Tag`, `Intro`, `Outro`)
    parse_label_parts(trimmed, true)
}

fn is_separator_token(tok: &str) -> bool {
    if matches!(
        tok,
        "|" | "||" | "/" | "//" | "-" | "(" | ")" | "N.C." | "NC"
    ) {
        return true;
    }
    let t = tok.trim_matches(['(', ')']);
    if let Some(rest) = t.strip_prefix('x').or_else(|| t.strip_prefix('X')) {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    }
    false
}

fn classify_chord_line(line: &str) -> (ChordLineConfidence, Vec<(u32, String)>) {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('{') || parse_section_marker(line).is_some() {
        return (ChordLineConfidence::None, Vec::new());
    }

    let mut chords: Vec<(u32, String)> = Vec::new();
    let mut non_sep_count = 0usize;
    let mut chars = line.char_indices().peekable();
    let mut col: u32 = 0;

    while let Some(&(_, ch)) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            col = col.saturating_add(1);
            continue;
        }
        let token_col = col;
        let mut token = String::new();
        while let Some(&(_, c)) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            token.push(c);
            chars.next();
            col = col.saturating_add(1);
        }

        let cleaned = token.trim_matches(['|', '(', ')', ',']);
        if is_separator_token(&token) || cleaned.is_empty() {
            continue;
        }
        non_sep_count += 1;
        if parse_chord(cleaned).is_some() {
            if chords.len() >= MAX_CHORDS_PER_LINE {
                return (ChordLineConfidence::None, Vec::new());
            }
            chords.push((token_col, cleaned.to_owned()));
        } else {
            return (ChordLineConfidence::None, Vec::new());
        }
    }

    if chords.is_empty() {
        return (ChordLineConfidence::None, Vec::new());
    }
    let has_spacing = line.starts_with(' ') || line.contains("  ") || trimmed.contains('|');
    if non_sep_count == 1 && !has_spacing && matches!(chords[0].1.as_str(), "A" | "Am") {
        return (ChordLineConfidence::AmbiguousSingleA, chords);
    }
    (ChordLineConfidence::Strong, chords)
}

#[derive(Clone, Debug)]
enum Directive {
    Title(String),
    Artist(String),
    Key(String),
    Tempo(f64),
    Ccli(String),
    Copyright(String),
    Tags(Vec<String>),
    StartBlock {
        kind: SectionKind,
        label: String,
        explicitly_numbered: bool,
    },
    EndBlock,
    StartTab,
    EndTab,
    Comment(String),
    Ignored,
}

fn parse_chordpro_directive(line: &str) -> Option<Directive> {
    let trimmed = line.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') || trimmed.len() > MAX_DIRECTIVE_LEN {
        return None;
    }
    let inner = trimmed[1..trimmed.len() - 1].trim();
    if inner.is_empty() {
        return Some(Directive::Ignored);
    }

    let (raw_name, raw_arg) = match inner.split_once([':', '=']) {
        Some((k, v)) => (k.trim().to_ascii_lowercase(), Some(v.trim())),
        None => {
            // Also support `{sov Verse 1}` space-separated form cleanly
            match inner.split_once(char::is_whitespace) {
                Some((k, v)) => (k.trim().to_ascii_lowercase(), Some(v.trim())),
                None => (inner.to_ascii_lowercase(), None),
            }
        }
    };
    let arg = raw_arg.unwrap_or("");

    match raw_name.as_str() {
        "title" | "t" => Some(Directive::Title(arg.to_owned())),
        "subtitle" | "st" => Some(Directive::Ignored),
        "artist" | "author" | "composer" | "lyricist" => Some(Directive::Artist(arg.to_owned())),
        "key" => Some(Directive::Key(arg.to_owned())),
        "tempo" => {
            if let Ok(bpm) = arg.trim().parse::<f64>()
                && bpm.is_finite()
                && (0.0..=1000.0).contains(&bpm)
            {
                Some(Directive::Tempo(bpm))
            } else {
                Some(Directive::Ignored)
            }
        }
        "ccli" | "ccli_number" => Some(Directive::Ccli(arg.to_owned())),
        "copyright" => Some(Directive::Copyright(arg.to_owned())),
        "tags" | "keywords" => {
            let tags = arg
                .split([',', ';'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            Some(Directive::Tags(tags))
        }
        "start_of_verse" | "sov" => {
            let (label, explicit) = if arg.is_empty() {
                ("Verse".to_owned(), false)
            } else if let Some(SectionSpec::Parsed {
                label,
                explicitly_numbered,
                ..
            }) = parse_label_parts(arg, false)
            {
                (label, explicitly_numbered)
            } else {
                (arg.to_owned(), true)
            };
            Some(Directive::StartBlock {
                kind: SectionKind::Verse,
                label,
                explicitly_numbered: explicit,
            })
        }
        "end_of_verse" | "eov" | "end_of_chorus" | "eoc" | "end_of_bridge" | "eob"
        | "end_of_part" | "eop" => Some(Directive::EndBlock),
        "start_of_chorus" | "soc" => {
            let (label, explicit) = if arg.is_empty() {
                ("Chorus".to_owned(), false)
            } else if let Some(SectionSpec::Parsed {
                label,
                explicitly_numbered,
                ..
            }) = parse_label_parts(arg, false)
            {
                (label, explicitly_numbered)
            } else {
                (arg.to_owned(), true)
            };
            Some(Directive::StartBlock {
                kind: SectionKind::Chorus,
                label,
                explicitly_numbered: explicit,
            })
        }
        "start_of_bridge" | "sob" => {
            let (label, explicit) = if arg.is_empty() {
                ("Bridge".to_owned(), false)
            } else if let Some(SectionSpec::Parsed {
                label,
                explicitly_numbered,
                ..
            }) = parse_label_parts(arg, false)
            {
                (label, explicitly_numbered)
            } else {
                (arg.to_owned(), true)
            };
            Some(Directive::StartBlock {
                kind: SectionKind::Bridge,
                label,
                explicitly_numbered: explicit,
            })
        }
        "start_of_part" | "sop" => {
            if let Some(SectionSpec::Parsed {
                kind,
                label,
                explicitly_numbered,
            }) = parse_label_parts(arg, false)
            {
                Some(Directive::StartBlock {
                    kind,
                    label,
                    explicitly_numbered,
                })
            } else {
                Some(Directive::StartBlock {
                    kind: SectionKind::Other,
                    label: if arg.is_empty() {
                        "Part".to_owned()
                    } else {
                        arg.to_owned()
                    },
                    explicitly_numbered: !arg.is_empty(),
                })
            }
        }
        "start_of_tab" | "sot" => Some(Directive::StartTab),
        "end_of_tab" | "eot" => Some(Directive::EndTab),
        "comment" | "c" | "comment_italic" | "ci" | "comment_box" | "cb" => {
            Some(Directive::Comment(arg.to_owned()))
        }
        _ => Some(Directive::Ignored),
    }
}

fn line_has_inline_chords(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || parse_section_marker(line).is_some() {
        return false;
    }
    for (start_idx, ch) in line.char_indices() {
        if ch == '[' {
            let after_open = start_idx + 1;
            if let Some(rel_close) = line[after_open..].find(']') {
                let close_idx = after_open + rel_close;
                let candidate = line[after_open..close_idx].trim();
                if !candidate.contains('[') && parse_chord(candidate).is_some() {
                    return true;
                }
            }
        }
    }
    false
}

fn parse_chordpro_lyric_line(line: &str, warnings: &mut Vec<String>) -> LyricLine {
    let mut text = String::with_capacity(line.len());
    let mut chords = Vec::new();
    let mut char_pos: u32 = 0;

    let mut rest = line;
    while !rest.is_empty() {
        if let Some(open_rel) = rest.find('[') {
            let before = &rest[..open_rel];
            text.push_str(before);
            char_pos = char_pos.saturating_add(before.chars().count() as u32);
            let after_open = &rest[open_rel + 1..];
            if let Some(close_rel) = after_open.find(']') {
                let candidate = after_open[..close_rel].trim();
                if !candidate.contains('[') && parse_chord(candidate).is_some() {
                    if chords.len() < MAX_CHORDS_PER_LINE {
                        chords.push(Chord {
                            position: char_pos,
                            text: candidate.to_owned(),
                        });
                    } else {
                        push_warning(
                            warnings,
                            "line exceeded maximum chord count; extra chords skipped",
                        );
                    }
                    rest = &after_open[close_rel + 1..];
                    continue;
                }
            }
            // Literal `[` without a valid `[Chord]` tag
            text.push('[');
            char_pos = char_pos.saturating_add(1);
            rest = after_open;
        } else {
            text.push_str(rest);
            break;
        }
    }

    LyricLine {
        text: text.trim_end().to_owned(),
        chords: chords
            .into_iter()
            .filter(|c| (c.position as usize) <= text.trim_end().chars().count())
            .collect(),
    }
}

pub fn detect_format(raw: &str) -> anyhow::Result<SongPasteFormat> {
    let lines = sanitize_lines(raw)?;
    Ok(detect_format_lines(&lines))
}

fn detect_format_lines(lines: &[String]) -> SongPasteFormat {
    let mut has_chordpro = false;
    let mut has_strong_chord_line = false;
    let mut has_markers = false;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if parse_chordpro_directive(line).is_some() || line_has_inline_chords(line) {
            has_chordpro = true;
            break;
        }
        if parse_section_marker(line).is_some() {
            has_markers = true;
            continue;
        }
        if matches!(classify_chord_line(line).0, ChordLineConfidence::Strong) {
            has_strong_chord_line = true;
        }
    }

    if has_chordpro {
        SongPasteFormat::ChordPro
    } else if has_strong_chord_line {
        SongPasteFormat::ChordsOverLyrics
    } else if has_markers {
        SongPasteFormat::Markers
    } else {
        SongPasteFormat::Plain
    }
}

fn normalize_block_key(lines: &[LyricLine]) -> String {
    lines
        .iter()
        .map(|l| {
            l.text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn finalize_song(
    raw_sections: Vec<RawSection>,
    mut song: Song,
    default_title: &str,
    from_plain: bool,
    warnings: Vec<String>,
    detected_format: SongPasteFormat,
) -> anyhow::Result<ParseOutput> {
    let filtered: Vec<RawSection> = raw_sections
        .into_iter()
        .filter(|s| !s.lines.is_empty() || s.explicit_marker)
        .collect();

    if from_plain && filtered.len() > 1 {
        return Ok(ParseOutput {
            song: finalize_plain_stanzas(filtered, song, default_title)?,
            detected_format,
            warnings,
        });
    }

    // Consolidate repeated Chorus (or identically repeated explicit recall markers like `[Chorus]` empty
    // or identical chorus text), and number unnumbered sections (`Verse` -> `Verse 1`, `Verse 2`, ...).
    let mut sections: Vec<Section> = Vec::new();
    let mut arrangement_ids: Vec<Id> = Vec::new();
    let mut chorus_by_label: HashMap<String, (Id, String)> = HashMap::new();
    let mut unnumbered_verse_count = 0u32;
    let mut unnumbered_bridge_count = 0u32;
    let mut used_labels: HashMap<String, usize> = HashMap::new();

    for raw_sec in filtered {
        ensure!(
            sections.len() < MAX_SECTIONS,
            "song exceeds {MAX_SECTIONS} sections"
        );
        let label_lower = raw_sec.label.trim().to_ascii_lowercase();

        // Handle Chorus recall: empty `[Chorus]` recall or verbatim repeat of an earlier Chorus
        if raw_sec.kind == SectionKind::Chorus
            && let Some((existing_id, existing_norm)) = chorus_by_label.get(&label_lower)
        {
            let norm = normalize_block_key(&raw_sec.lines);
            if raw_sec.lines.is_empty() || norm == *existing_norm {
                arrangement_ids.push(existing_id.clone());
                continue;
            }
        }

        let mut label = raw_sec.label.clone();
        if raw_sec.kind == SectionKind::Verse
            && !raw_sec.explicitly_numbered
            && label_lower == "verse"
        {
            unnumbered_verse_count += 1;
            label = format!("Verse {unnumbered_verse_count}");
        } else if raw_sec.kind == SectionKind::Bridge
            && !raw_sec.explicitly_numbered
            && label_lower == "bridge"
        {
            unnumbered_bridge_count += 1;
            if unnumbered_bridge_count > 1 {
                label = format!("Bridge {unnumbered_bridge_count}");
            }
        }

        // Disambiguate if another distinct section already used the same non-chorus label or differing chorus
        let count = used_labels.entry(label.to_ascii_lowercase()).or_insert(0);
        *count += 1;
        if *count > 1 {
            label = format!("{label} {}", *count);
        }

        let id = new_id();
        if raw_sec.kind == SectionKind::Chorus {
            chorus_by_label
                .entry(label_lower)
                .or_insert_with(|| (id.clone(), normalize_block_key(&raw_sec.lines)));
        }
        arrangement_ids.push(id.clone());
        sections.push(Section {
            id: Some(id),
            label,
            kind: raw_sec.kind as i32,
            color: None,
            hotkey: String::new(),
            lines: raw_sec.lines,
            translations: HashMap::new(),
        });
    }

    if sections.is_empty() {
        let id = new_id();
        arrangement_ids.push(id.clone());
        sections.push(Section {
            id: Some(id),
            label: "Verse 1".into(),
            kind: SectionKind::Verse as i32,
            ..Default::default()
        });
    }

    assign_default_hotkeys_and_colors(&mut sections);
    song.sections = sections;
    song.arrangements = vec![Arrangement {
        id: Some(new_id()),
        name: "Default".into(),
        section_ids: arrangement_ids,
    }];
    if song.title.trim().is_empty() {
        song.title = infer_title(&song.sections, default_title);
    }
    if song.id.is_none() {
        song.id = Some(new_id());
    }

    Ok(ParseOutput {
        song,
        detected_format,
        warnings,
    })
}

fn finalize_plain_stanzas(
    filtered: Vec<RawSection>,
    mut song: Song,
    default_title: &str,
) -> anyhow::Result<Song> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let keys: Vec<String> = filtered
        .iter()
        .map(|s| normalize_block_key(&s.lines))
        .collect();
    for k in &keys {
        if !k.is_empty() {
            *counts.entry(k.clone()).or_insert(0) += 1;
        }
    }

    // Distinct repeated stanzas become Chorus / Chorus 2...
    let mut repeated_order: Vec<String> = Vec::new();
    for k in &keys {
        if !k.is_empty() && counts.get(k).copied().unwrap_or(0) >= 2 && !repeated_order.contains(k)
        {
            repeated_order.push(k.clone());
        }
    }
    let multi_chorus = repeated_order.len() > 1;
    let mut chorus_ids: HashMap<String, Id> = HashMap::new();

    let mut sections: Vec<Section> = Vec::new();
    let mut arrangement_ids: Vec<Id> = Vec::new();
    let mut verse_num = 0u32;

    for (raw_sec, key) in filtered.into_iter().zip(keys.into_iter()) {
        ensure!(
            sections.len() < MAX_SECTIONS,
            "song exceeds {MAX_SECTIONS} sections"
        );
        if let Some(pos) = repeated_order.iter().position(|r| r == &key) {
            if let Some(existing_id) = chorus_ids.get(&key) {
                arrangement_ids.push(existing_id.clone());
                continue;
            }
            let id = new_id();
            let label = if multi_chorus {
                format!("Chorus {}", pos + 1)
            } else {
                "Chorus".to_owned()
            };
            chorus_ids.insert(key, id.clone());
            arrangement_ids.push(id.clone());
            sections.push(Section {
                id: Some(id),
                label,
                kind: SectionKind::Chorus as i32,
                color: None,
                hotkey: String::new(),
                lines: raw_sec.lines,
                translations: HashMap::new(),
            });
        } else {
            verse_num += 1;
            let id = new_id();
            arrangement_ids.push(id.clone());
            sections.push(Section {
                id: Some(id),
                label: format!("Verse {verse_num}"),
                kind: SectionKind::Verse as i32,
                color: None,
                hotkey: String::new(),
                lines: raw_sec.lines,
                translations: HashMap::new(),
            });
        }
    }

    assign_default_hotkeys_and_colors(&mut sections);
    song.sections = sections;
    song.arrangements = vec![Arrangement {
        id: Some(new_id()),
        name: "Default".into(),
        section_ids: arrangement_ids,
    }];
    if song.title.trim().is_empty() {
        song.title = infer_title(&song.sections, default_title);
    }
    if song.id.is_none() {
        song.id = Some(new_id());
    }
    Ok(song)
}

fn infer_title(sections: &[Section], default_title: &str) -> String {
    let trimmed_default = default_title.trim();
    if !trimmed_default.is_empty() {
        return trimmed_default.chars().take(512).collect();
    }
    for sec in sections {
        for line in &sec.lines {
            let clean = line
                .text
                .trim()
                .trim_end_matches(['.', ',', ';', ':', '!', '?'])
                .trim();
            if !clean.is_empty() {
                return clean.chars().take(80).collect();
            }
        }
    }
    "Untitled Song".into()
}

fn parse_plain_or_markers(
    lines: &[String],
    expect_markers: bool,
) -> anyhow::Result<Vec<RawSection>> {
    let mut raw_sections: Vec<RawSection> = Vec::new();
    let mut current: Option<RawSection> = None;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Blank line closes an implicit plain stanza, OR separates stanzas before any marker.
            if let Some(sec) = current.take() {
                if sec.explicit_marker && sec.lines.is_empty() {
                    // Blank line immediately after a header `[Chorus]` shouldn't discard header before its body
                    current = Some(sec);
                } else {
                    raw_sections.push(sec);
                }
            }
            continue;
        }

        if expect_markers
            && let Some(SectionSpec::Parsed {
                kind,
                label,
                explicitly_numbered,
            }) = parse_section_marker(line)
        {
            if let Some(prev) = current.take() {
                raw_sections.push(prev);
            }
            ensure!(raw_sections.len() < MAX_SECTIONS, "too many sections");
            current = Some(RawSection {
                kind,
                label,
                explicitly_numbered,
                explicit_marker: true,
                lines: Vec::new(),
            });
            continue;
        }

        let sec = current.get_or_insert_with(|| RawSection {
            kind: SectionKind::Verse,
            label: "Verse".into(),
            explicitly_numbered: false,
            explicit_marker: false,
            lines: Vec::new(),
        });
        ensure!(
            sec.lines.len() < MAX_LINES_PER_SECTION,
            "section exceeds {MAX_LINES_PER_SECTION} lines"
        );
        sec.lines.push(LyricLine {
            text: line.trim_end().to_owned(),
            chords: Vec::new(),
        });
    }

    if let Some(prev) = current.take() {
        raw_sections.push(prev);
    }
    Ok(raw_sections)
}

fn parse_chords_over_lyrics(lines: &[String]) -> anyhow::Result<Vec<RawSection>> {
    let line_classes: Vec<(ChordLineConfidence, Vec<(u32, String)>)> =
        lines.iter().map(|l| classify_chord_line(l)).collect();
    let has_strong_anywhere = line_classes
        .iter()
        .any(|(conf, _)| *conf == ChordLineConfidence::Strong);

    let mut raw_sections: Vec<RawSection> = Vec::new();
    let mut current: Option<RawSection> = None;
    let mut pending_chords: Option<Vec<(u32, String)>> = None;

    for (line, (conf, extracted_chords)) in lines.iter().zip(line_classes.into_iter()) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let Some(orphan) = pending_chords.take() {
                let sec = current.get_or_insert_with(|| RawSection {
                    kind: SectionKind::Verse,
                    label: "Verse".into(),
                    explicitly_numbered: false,
                    explicit_marker: false,
                    lines: Vec::new(),
                });
                ensure!(
                    sec.lines.len() < MAX_LINES_PER_SECTION,
                    "section exceeds {MAX_LINES_PER_SECTION} lines"
                );
                sec.lines.push(LyricLine {
                    text: String::new(),
                    chords: orphan
                        .into_iter()
                        .map(|(_, text)| Chord { position: 0, text })
                        .collect(),
                });
            }
            if let Some(sec) = current.take() {
                if sec.explicit_marker && sec.lines.is_empty() {
                    current = Some(sec);
                } else {
                    raw_sections.push(sec);
                }
            }
            continue;
        }

        if let Some(SectionSpec::Parsed {
            kind,
            label,
            explicitly_numbered,
        }) = parse_section_marker(line)
        {
            if let Some(orphan) = pending_chords.take() {
                let sec = current.get_or_insert_with(|| RawSection {
                    kind: SectionKind::Verse,
                    label: "Verse".into(),
                    explicitly_numbered: false,
                    explicit_marker: false,
                    lines: Vec::new(),
                });
                ensure!(
                    sec.lines.len() < MAX_LINES_PER_SECTION,
                    "section exceeds {MAX_LINES_PER_SECTION} lines"
                );
                sec.lines.push(LyricLine {
                    text: String::new(),
                    chords: orphan
                        .into_iter()
                        .map(|(_, text)| Chord { position: 0, text })
                        .collect(),
                });
            }
            if let Some(prev) = current.take() {
                raw_sections.push(prev);
            }
            ensure!(raw_sections.len() < MAX_SECTIONS, "too many sections");
            current = Some(RawSection {
                kind,
                label,
                explicitly_numbered,
                explicit_marker: true,
                lines: Vec::new(),
            });
            continue;
        }

        let is_chord_line = match conf {
            ChordLineConfidence::Strong => true,
            ChordLineConfidence::AmbiguousSingleA => has_strong_anywhere,
            ChordLineConfidence::None => false,
        };

        if is_chord_line {
            if let Some(orphan) = pending_chords.take() {
                let sec = current.get_or_insert_with(|| RawSection {
                    kind: SectionKind::Verse,
                    label: "Verse".into(),
                    explicitly_numbered: false,
                    explicit_marker: false,
                    lines: Vec::new(),
                });
                ensure!(
                    sec.lines.len() < MAX_LINES_PER_SECTION,
                    "section exceeds {MAX_LINES_PER_SECTION} lines"
                );
                sec.lines.push(LyricLine {
                    text: String::new(),
                    chords: orphan
                        .into_iter()
                        .map(|(_, text)| Chord { position: 0, text })
                        .collect(),
                });
            }
            pending_chords = Some(extracted_chords);
            continue;
        }

        // Lyric line! Attach any pending chords cleanly bounded to `text.chars().count()`.
        let lyric_text = line.trim_end().to_owned();
        let char_len = lyric_text.chars().count() as u32;
        let chords = match pending_chords.take() {
            Some(c_list) => c_list
                .into_iter()
                .map(|(col, text)| Chord {
                    position: col.min(char_len),
                    text,
                })
                .collect(),
            None => Vec::new(),
        };

        let sec = current.get_or_insert_with(|| RawSection {
            kind: SectionKind::Verse,
            label: "Verse".into(),
            explicitly_numbered: false,
            explicit_marker: false,
            lines: Vec::new(),
        });
        ensure!(
            sec.lines.len() < MAX_LINES_PER_SECTION,
            "section exceeds {MAX_LINES_PER_SECTION} lines"
        );
        sec.lines.push(LyricLine {
            text: lyric_text,
            chords,
        });
    }

    if let Some(orphan) = pending_chords.take() {
        let sec = current.get_or_insert_with(|| RawSection {
            kind: SectionKind::Verse,
            label: "Verse".into(),
            explicitly_numbered: false,
            explicit_marker: false,
            lines: Vec::new(),
        });
        ensure!(
            sec.lines.len() < MAX_LINES_PER_SECTION,
            "section exceeds {MAX_LINES_PER_SECTION} lines"
        );
        sec.lines.push(LyricLine {
            text: String::new(),
            chords: orphan
                .into_iter()
                .map(|(_, text)| Chord { position: 0, text })
                .collect(),
        });
    }
    if let Some(prev) = current.take() {
        raw_sections.push(prev);
    }
    Ok(raw_sections)
}

fn parse_chordpro(
    lines: &[String],
    warnings: &mut Vec<String>,
) -> anyhow::Result<(Song, Vec<RawSection>)> {
    let mut song = Song {
        public_domain: false,
        ..Default::default()
    };
    let mut raw_sections: Vec<RawSection> = Vec::new();
    let mut current: Option<RawSection> = None;
    let mut block_depth: usize = 0;
    let mut in_tab = false;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Blank line inside an explicit `{sov}..{eov}` block separates nothing,
            // outside an explicit block splits implicit verses cleanly.
            if let Some(sec) = current.take() {
                if block_depth > 0 || (sec.explicit_marker && sec.lines.is_empty()) {
                    current = Some(sec);
                } else {
                    raw_sections.push(sec);
                }
            }
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }

        if let Some(directive) = parse_chordpro_directive(line) {
            match directive {
                Directive::StartTab => {
                    in_tab = true;
                }
                Directive::EndTab => {
                    in_tab = false;
                }
                _ if in_tab => {}
                Directive::Title(t) => {
                    if !t.is_empty() {
                        song.title = t.chars().take(512).collect();
                    }
                }
                Directive::Artist(a) => {
                    if !a.is_empty() && song.authors.len() < MAX_CHILDREN {
                        song.authors.push(a.chars().take(512).collect());
                    }
                }
                Directive::Key(k) => song.key = k.chars().take(64).collect(),
                Directive::Tempo(bpm) => song.tempo = bpm,
                Directive::Ccli(c) => song.ccli_number = c.chars().take(64).collect(),
                Directive::Copyright(c) => {
                    if c.to_ascii_lowercase().contains("public domain") {
                        song.public_domain = true;
                    }
                    song.copyright = c.chars().take(512).collect();
                }
                Directive::Tags(tags) => {
                    for tag in tags {
                        if song.tags.len() < MAX_CHILDREN {
                            song.tags.push(tag.chars().take(128).collect());
                        }
                    }
                }
                Directive::StartBlock {
                    kind,
                    label,
                    explicitly_numbered,
                } => {
                    ensure!(
                        block_depth < MAX_BLOCK_DEPTH,
                        "ChordPro block nesting exceeds {MAX_BLOCK_DEPTH}"
                    );
                    if let Some(prev) = current.take() {
                        raw_sections.push(prev);
                    }
                    ensure!(raw_sections.len() < MAX_SECTIONS, "too many sections");
                    block_depth += 1;
                    current = Some(RawSection {
                        kind,
                        label,
                        explicitly_numbered,
                        explicit_marker: true,
                        lines: Vec::new(),
                    });
                }
                Directive::EndBlock => {
                    block_depth = block_depth.saturating_sub(1);
                    if let Some(prev) = current.take() {
                        raw_sections.push(prev);
                    }
                }
                Directive::Comment(text) => {
                    if let Some(SectionSpec::Parsed {
                        kind,
                        label,
                        explicitly_numbered,
                    }) = parse_label_parts(&text, false)
                    {
                        if let Some(prev) = current.take() {
                            raw_sections.push(prev);
                        }
                        ensure!(raw_sections.len() < MAX_SECTIONS, "too many sections");
                        current = Some(RawSection {
                            kind,
                            label,
                            explicitly_numbered,
                            explicit_marker: true,
                            lines: Vec::new(),
                        });
                    }
                }
                Directive::Ignored => {}
            }
            continue;
        }

        if in_tab {
            continue;
        }

        if let Some(SectionSpec::Parsed {
            kind,
            label,
            explicitly_numbered,
        }) = parse_section_marker(line)
        {
            if let Some(prev) = current.take() {
                raw_sections.push(prev);
            }
            ensure!(raw_sections.len() < MAX_SECTIONS, "too many sections");
            current = Some(RawSection {
                kind,
                label,
                explicitly_numbered,
                explicit_marker: true,
                lines: Vec::new(),
            });
            continue;
        }

        let sec = current.get_or_insert_with(|| RawSection {
            kind: SectionKind::Verse,
            label: "Verse".into(),
            explicitly_numbered: false,
            explicit_marker: false,
            lines: Vec::new(),
        });
        ensure!(
            sec.lines.len() < MAX_LINES_PER_SECTION,
            "section exceeds {MAX_LINES_PER_SECTION} lines"
        );
        sec.lines.push(parse_chordpro_lyric_line(line, warnings));
    }

    if let Some(prev) = current.take() {
        raw_sections.push(prev);
    }
    Ok((song, raw_sections))
}

pub fn parse_song(
    raw: &str,
    format: SongPasteFormat,
    default_title: &str,
) -> anyhow::Result<ParseOutput> {
    let lines = sanitize_lines(raw)?;
    let has_content = lines.iter().any(|l| !l.trim().is_empty());
    if !has_content {
        bail!("pasted song text is empty");
    }

    let resolved = match format {
        SongPasteFormat::Unspecified => detect_format_lines(&lines),
        other => other,
    };

    let mut warnings = Vec::new();
    let (song, raw_sections, from_plain) = match resolved {
        SongPasteFormat::Plain => (
            Song::default(),
            parse_plain_or_markers(&lines, false)?,
            true,
        ),
        SongPasteFormat::Markers => (
            Song::default(),
            parse_plain_or_markers(&lines, true)?,
            false,
        ),
        SongPasteFormat::ChordsOverLyrics => {
            (Song::default(), parse_chords_over_lyrics(&lines)?, false)
        }
        SongPasteFormat::ChordPro => {
            let (s, secs) = parse_chordpro(&lines, &mut warnings)?;
            (s, secs, false)
        }
        SongPasteFormat::Unspecified => unreachable!(),
    };

    finalize_song(
        raw_sections,
        song,
        default_title,
        from_plain,
        warnings,
        resolved,
    )
}

/// SEC-03 fuzz entry point for ChordPro / arbitrary pasted song bytes.
/// Must never panic, hang, or allocate unbounded memory on arbitrary bytes.
pub fn fuzz_chordpro(data: &[u8]) -> usize {
    if data.len() > MAX_PASTE_BYTES {
        return 0;
    }
    let text = String::from_utf8_lossy(data);
    match parse_song(&text, SongPasteFormat::ChordPro, "") {
        Ok(parsed) => parsed.song.sections.len(),
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Document;

    #[test]
    fn plain_repeated_stanza_becomes_chorus_and_preserves_arrangement() {
        let input = "\
Line one of verse one
Line two of verse one

Chorus line A
Chorus line B

Line one of verse two
Line two of verse two

Chorus line A
Chorus line B";

        let out = parse_song(input, SongPasteFormat::Unspecified, "Sample Hymn").unwrap();
        assert_eq!(out.detected_format, SongPasteFormat::Plain);
        assert_eq!(out.song.title, "Sample Hymn");
        assert!(out.song.validate().is_ok());

        let labels: Vec<&str> = out.song.sections.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, vec!["Verse 1", "Chorus", "Verse 2"]);
        assert_eq!(out.song.sections[1].kind, SectionKind::Chorus as i32);

        let arr: Vec<&str> = out.song.arrangements[0]
            .section_ids
            .iter()
            .map(|id| {
                out.song
                    .sections
                    .iter()
                    .find(|s| s.id.as_ref() == Some(id))
                    .unwrap()
                    .label
                    .as_str()
            })
            .collect();
        assert_eq!(arr, vec!["Verse 1", "Chorus", "Verse 2", "Chorus"]);
    }

    #[test]
    fn chords_over_lyrics_computes_unicode_scalar_positions_and_guards_english_a() {
        let plain_english = "\
A mighty fortress is our God

A
bulwark never failing";
        // Notice `"A"` alone with zero other chord lines stays Plain, NOT ChordsOverLyrics!
        assert_eq!(
            detect_format(plain_english).unwrap(),
            SongPasteFormat::Plain
        );

        let chorded = "\
[Verse 1]
G              C/G   G
’Twas grace that taught my heart
       D/F#            G
And grace my fears relieved";
        let out = parse_song(chorded, SongPasteFormat::Unspecified, "Test").unwrap();
        assert_eq!(out.detected_format, SongPasteFormat::ChordsOverLyrics);
        assert!(out.song.validate().is_ok());
        let line0 = &out.song.sections[0].lines[0];
        assert_eq!(line0.text, "’Twas grace that taught my heart");
        // Verify `'’'` counts as 1 Unicode char (offset 0 = '’', offset 15 = 'a' in taught or column 15, offset 21)
        assert_eq!(line0.chords[0].position, 0);
        assert_eq!(line0.chords[0].text, "G");
        assert_eq!(line0.chords[1].position, 15);
        assert_eq!(line0.chords[1].text, "C/G");
        assert_eq!(line0.chords[2].position, 21);
        assert_eq!(line0.chords[2].text, "G");
    }

    #[test]
    fn chordpro_extracts_metadata_directives_sections_and_unicode_chord_positions() {
        let cho = "\
{title: Amazing Grace}
{artist: John Newton}
{key: G}
{ccli: 22025}
{copyright: Public Domain}
{sov}
[G]’Twas grace that [C]taught my heart to [G]fear
{eov}
{sot}
e|--3--0--|
{eot}
{sob}
[Em]When we’ve [D]been there
{eob}";
        let out = parse_song(cho, SongPasteFormat::Unspecified, "").unwrap();
        assert_eq!(out.detected_format, SongPasteFormat::ChordPro);
        assert_eq!(out.song.title, "Amazing Grace");
        assert_eq!(out.song.authors, vec!["John Newton"]);
        assert_eq!(out.song.key, "G");
        assert_eq!(out.song.ccli_number, "22025");
        assert!(out.song.public_domain);
        assert!(out.song.validate().is_ok());
        assert_eq!(out.song.sections.len(), 2);
        assert_eq!(out.song.sections[0].label, "Verse 1");
        assert_eq!(out.song.sections[1].label, "Bridge");
        // Unicode char index before `[C]` in `"’Twas grace that "`: 17 chars (NOT 19 UTF-8 bytes)
        assert_eq!(out.song.sections[0].lines[0].chords[1].position, 17);
        assert_eq!(out.song.sections[0].lines[0].chords[1].text, "C");
    }

    #[test]
    fn sec03_paste_limits_reject_oversized_nul_and_deep_nesting() {
        assert!(parse_song("hello\0world", SongPasteFormat::Plain, "x").is_err());
        let big = "a".repeat(MAX_PASTE_BYTES + 1);
        assert!(parse_song(&big, SongPasteFormat::Plain, "x").is_err());
        let deep = "{sov}\n".repeat(MAX_BLOCK_DEPTH + 1) + "line";
        assert!(parse_song(&deep, SongPasteFormat::ChordPro, "x").is_err());
    }
}
