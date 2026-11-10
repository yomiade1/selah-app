//! 6-language scripture reference parser (`en`, `es`, `pt`, `fr`, `ko`, `yo`) with SEC-03 bounds.
use anyhow::{Context, ensure};
use selah_proto::v1::ScriptureSpan;

use super::books::{BookSpec, book_by_num, resolve_book_alias};

pub const MAX_REFERENCE_BYTES: usize = 512;
pub const MAX_REFERENCE_SPANS: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedReference {
    pub spans: Vec<ScriptureSpan>,
    pub normalized: String,
}

pub fn format_span(
    book_name: &str,
    start_ch: u32,
    start_v: u32,
    end_ch: u32,
    end_v: u32,
) -> String {
    if start_v == 0 && end_v == 0 {
        if start_ch == end_ch {
            format!("{book_name} {start_ch}")
        } else {
            format!("{book_name} {start_ch}\u{2013}{end_ch}")
        }
    } else if start_ch == end_ch {
        if start_v == end_v {
            format!("{book_name} {start_ch}:{start_v}")
        } else {
            format!("{book_name} {start_ch}:{start_v}\u{2013}{end_v}")
        }
    } else {
        let start_part = if start_v == 0 {
            format!("{start_ch}")
        } else {
            format!("{start_ch}:{start_v}")
        };
        let end_part = if end_v == 0 {
            format!("{end_ch}")
        } else {
            format!("{end_ch}:{end_v}")
        };
        format!("{book_name} {start_part}\u{2013}{end_part}")
    }
}

fn normalize_input(raw: &str) -> anyhow::Result<String> {
    ensure!(
        !raw.contains('\0'),
        "scripture reference contains a NUL byte"
    );
    ensure!(
        raw.len() <= MAX_REFERENCE_BYTES,
        "scripture reference exceeds {MAX_REFERENCE_BYTES} bytes"
    );
    let mut out = String::with_capacity(raw.len());
    for ch in raw.trim().chars() {
        ensure!(
            !ch.is_control() || ch.is_whitespace(),
            "scripture reference contains control characters"
        );
        let mapped = match ch {
            '\u{2013}' | '\u{2014}' | '\u{2212}' | '\u{FF0D}' => '-',
            '\u{FF1A}' => ':',
            '\u{FF0C}' => ',',
            '\u{FF10}'..='\u{FF19}' => {
                char::from_u32('0' as u32 + (ch as u32 - 0xFF10)).unwrap_or(ch)
            }
            _ => ch,
        };
        out.push(mapped);
    }
    Ok(out)
}

fn is_spec_tail_char(ch: char) -> bool {
    ch.is_ascii_digit()
        || ch.is_whitespace()
        || matches!(ch, ':' | ',' | '-' | '.' | '장' | '편' | '절')
}

/// Split a single clause like `"1 Cor 13:4-7"` or `"Jn3:16"` into `(book_part, spec_part)`.
/// Returns `(None, clause)` if the clause has no alphabetic characters (book inherited from prior clause).
fn split_book_and_spec(clause: &str) -> anyhow::Result<(Option<&str>, &str)> {
    let trimmed = clause.trim().trim_end_matches('.');
    ensure!(!trimmed.is_empty(), "empty scripture reference clause");

    if !trimmed.chars().any(char::is_alphabetic) {
        return Ok((None, trimmed));
    }

    // Walk backward from the end over numeric/punctuation/Korean chapter-verse markers.
    let mut scan_start = trimmed.len();
    for (idx, ch) in trimmed.char_indices().rev() {
        if is_spec_tail_char(ch) {
            // Korean markers '장'/'편'/'절' only belong to the numeric spec if preceded by a digit.
            if matches!(ch, '장' | '편' | '절') {
                let before = &trimmed[..idx];
                if !before
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_digit())
                {
                    break;
                }
            }
            scan_start = idx;
        } else {
            break;
        }
    }

    let raw_spec = &trimmed[scan_start..];
    // Advance past leading separators (spaces or trailing period after book abbr like "Ps. 23").
    let first_digit = raw_spec
        .char_indices()
        .find(|&(_, ch)| ch.is_ascii_digit())
        .map(|(i, _)| scan_start + i)
        .context("missing chapter or verse numbers in scripture reference")?;

    let book_part = trimmed[..first_digit]
        .trim()
        .trim_end_matches(['.', ','])
        .trim();
    ensure!(
        !book_part.is_empty(),
        "missing Bible book name in scripture reference"
    );
    let spec_part = trimmed[first_digit..].trim();
    ensure!(
        !spec_part.is_empty(),
        "missing chapter or verse numbers in scripture reference"
    );
    Ok((Some(book_part), spec_part))
}

fn preprocess_spec(spec: &str) -> anyhow::Result<String> {
    let mut out = String::with_capacity(spec.len());
    let chars: Vec<char> = spec.chars().collect();
    for (idx, &ch) in chars.iter().enumerate() {
        match ch {
            '장' | '편' => {
                // If digits follow later (e.g. "3장 16절"), turn into ':'.
                if chars[idx + 1..].iter().any(|c| c.is_ascii_digit()) {
                    out.push(':');
                }
            }
            '절' => {}
            other => out.push(other),
        }
    }
    let normalized = out.trim().to_owned();
    ensure!(!normalized.is_empty(), "empty chapter or verse specifier");
    Ok(normalized)
}

fn parse_pos_u32(raw: &str, label: &str) -> anyhow::Result<u32> {
    let t = raw.trim();
    ensure!(
        !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()),
        "invalid {label} '{t}'"
    );
    let n: u32 = t
        .parse()
        .with_context(|| format!("invalid {label} '{t}'"))?;
    ensure!(n > 0, "{label} must be >= 1");
    ensure!(n <= 2000, "{label} out of range");
    Ok(n)
}

fn make_span(book: &BookSpec, sc: u32, sv: u32, ec: u32, ev: u32) -> anyhow::Result<ScriptureSpan> {
    ensure!(sc > 0 && ec > 0, "chapter number must be >= 1");
    ensure!(ec >= sc, "ending chapter cannot precede starting chapter");
    if sc == ec && (sv != 0 || ev != 0) {
        ensure!(sv > 0 && ev >= sv, "invalid verse range {sv}-{ev}");
    }
    Ok(ScriptureSpan {
        book_num: book.book_num,
        book_code: book.code.to_owned(),
        book_name: book.name.to_owned(),
        start_chapter: sc,
        start_verse: sv,
        end_chapter: ec,
        end_verse: ev,
        normalized: format_span(book.name, sc, sv, ec, ev),
    })
}

fn parse_verse_or_range(book: &BookSpec, ch: u32, token: &str) -> anyhow::Result<ScriptureSpan> {
    let compact: String = token.chars().filter(|c| !c.is_whitespace()).collect();
    if let Some((left, right)) = compact.split_once('-') {
        let sv = parse_pos_u32(left, "verse")?;
        if let Some((ec_str, ev_str)) = right.split_once(':').or_else(|| right.split_once('.')) {
            let ec = parse_pos_u32(ec_str, "chapter")?;
            let ev = parse_pos_u32(ev_str, "verse")?;
            make_span(book, ch, sv, ec, ev)
        } else {
            let ev = parse_pos_u32(right, "verse")?;
            make_span(book, ch, sv, ch, ev)
        }
    } else {
        let v = parse_pos_u32(&compact, "verse")?;
        make_span(book, ch, v, ch, v)
    }
}

fn parse_single_chapter_spec(book: &BookSpec, spec: &str) -> anyhow::Result<Vec<ScriptureSpan>> {
    // Allow "3", "24", "1-5", "1:24", "1:1-5", "1:2, 5-7", or space "1 24".
    if spec.contains(':') || spec.contains('.') {
        return parse_multi_chapter_spec(book, spec);
    }
    let parts: Vec<&str> = spec
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    ensure!(!parts.is_empty(), "empty verse specifier");

    // Check "1 24" (explicit chapter 1 followed by verse)
    if parts.len() == 1 {
        let tokens: Vec<&str> = parts[0].split_whitespace().collect();
        if tokens.len() == 2 && parse_pos_u32(tokens[0], "chapter").ok() == Some(1) {
            return Ok(vec![parse_verse_or_range(book, 1, tokens[1])?]);
        }
        let compact: String = parts[0].chars().filter(|c| !c.is_whitespace()).collect();
        if let Some((l, r)) = compact.split_once('-') {
            let sv = parse_pos_u32(l, "verse")?;
            let ev = parse_pos_u32(r, "verse")?;
            return Ok(vec![make_span(book, 1, sv, 1, ev)?]);
        }
        let n = parse_pos_u32(&compact, "chapter or verse")?;
        if n == 1 {
            return Ok(vec![make_span(book, 1, 0, 1, 0)?]);
        }
        return Ok(vec![make_span(book, 1, n, 1, n)?]);
    }

    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        out.push(parse_verse_or_range(book, 1, part)?);
    }
    Ok(out)
}

fn normalize_colon_and_dots(part: &str) -> String {
    part.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .replace('.', ":")
}

fn parse_multi_chapter_spec(book: &BookSpec, spec: &str) -> anyhow::Result<Vec<ScriptureSpan>> {
    // European "Jn 3,16" or "Jn 3, 16-18" or compact "Jn 3,16, 18-19" when there's no ':' and '.'
    if !spec.contains(':')
        && !spec.contains('.')
        && let Some((head, tail)) = spec.split_once(',')
    {
        let comma_parts: Vec<&str> = spec
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if comma_parts.len() == 2
            && !comma_parts[0].contains('-')
            && !comma_parts[0].contains(char::is_whitespace)
        {
            let ch = parse_pos_u32(comma_parts[0], "chapter")?;
            return Ok(vec![parse_verse_or_range(book, ch, comma_parts[1])?]);
        }
        if comma_parts.len() > 2
            && !head.trim().contains('-')
            && !head.trim().contains(char::is_whitespace)
            && !tail.starts_with(char::is_whitespace)
        {
            let ch = parse_pos_u32(comma_parts[0], "chapter")?;
            let mut out = Vec::with_capacity(comma_parts.len() - 1);
            for part in &comma_parts[1..] {
                out.push(parse_verse_or_range(book, ch, part)?);
            }
            return Ok(out);
        }
    }

    let comma_parts: Vec<&str> = spec.split(',').collect();
    ensure!(
        comma_parts.iter().all(|p| !p.trim().is_empty()),
        "empty segment around comma"
    );

    let mut spans = Vec::new();
    let mut active_chapter: Option<u32> = None;

    for (idx, raw_part) in comma_parts.iter().enumerate() {
        let part = raw_part.trim();
        let compact = normalize_colon_and_dots(part);

        if let Some((ch_str, rest)) = compact.split_once(':') {
            // Check for cross-chapter range like "1:26-2:3"
            let ch = parse_pos_u32(ch_str, "chapter")?;
            let span = parse_verse_or_range(book, ch, rest)?;
            active_chapter = Some(span.end_chapter);
            spans.push(span);
            continue;
        }

        // Handle "3 16" or "3 16-18" on first segment when no ':' or '.' is present
        let ws_tokens: Vec<&str> = part.split_whitespace().collect();
        if idx == 0 && ws_tokens.len() == 2 && active_chapter.is_none() {
            let ch = parse_pos_u32(ws_tokens[0], "chapter")?;
            let span = parse_verse_or_range(book, ch, ws_tokens[1])?;
            active_chapter = Some(span.end_chapter);
            spans.push(span);
            continue;
        }

        if let Some(ch) = active_chapter {
            // Continuation after "Rom 8:28, 31-32, 38-39"
            ensure!(
                ws_tokens.len() == 1 || compact.contains('-'),
                "ambiguous reference list item '{part}'"
            );
            spans.push(parse_verse_or_range(book, ch, &compact)?);
            continue;
        }

        // Pure chapter or chapter range ("13" or "1-2")
        if let Some((l, r)) = compact.split_once('-') {
            let sc = parse_pos_u32(l, "chapter")?;
            let ec = parse_pos_u32(r, "chapter")?;
            active_chapter = Some(ec);
            spans.push(make_span(book, sc, 0, ec, 0)?);
        } else {
            let ch = parse_pos_u32(&compact, "chapter")?;
            active_chapter = Some(ch);
            spans.push(make_span(book, ch, 0, ch, 0)?);
        }
    }

    Ok(spans)
}

/// Parse a scripture reference string in any of the 6 supported languages (`en`, `es`, `pt`, `fr`, `ko`, `yo`).
pub fn parse_reference(raw: &str, language_hint: Option<&str>) -> anyhow::Result<ParsedReference> {
    let cleaned = normalize_input(raw)?;
    ensure!(!cleaned.is_empty(), "scripture reference cannot be empty");

    let mut all_spans: Vec<ScriptureSpan> = Vec::new();
    let mut last_book: Option<&BookSpec> = None;

    for raw_clause in cleaned.split(';') {
        let clause = raw_clause.trim();
        ensure!(!clause.is_empty(), "empty reference around ';'");
        let (book_opt, raw_spec) = split_book_and_spec(clause)?;
        let book = match book_opt {
            Some(book_str) => {
                let resolved = resolve_book_alias(book_str, language_hint)
                    .with_context(|| format!("unknown Bible book '{book_str}'"))?;
                last_book = Some(resolved);
                resolved
            }
            None => last_book.context("missing Bible book name in scripture reference")?,
        };
        let spec = preprocess_spec(raw_spec)?;
        let mut clause_spans = if book.is_single_chapter {
            parse_single_chapter_spec(book, &spec)?
        } else {
            parse_multi_chapter_spec(book, &spec)?
        };
        ensure!(
            all_spans.len() + clause_spans.len() <= MAX_REFERENCE_SPANS,
            "too many scripture spans (max {MAX_REFERENCE_SPANS})"
        );
        all_spans.append(&mut clause_spans);
    }

    ensure!(!all_spans.is_empty(), "no valid scripture spans parsed");

    let normalized = format_spans_summary(&all_spans);
    Ok(ParsedReference {
        spans: all_spans,
        normalized,
    })
}

pub fn format_spans_summary(spans: &[ScriptureSpan]) -> String {
    if spans.is_empty() {
        return String::new();
    }
    let mut groups: Vec<String> = Vec::new();
    let mut idx = 0;
    while idx < spans.len() {
        let first = &spans[idx];
        let mut j = idx + 1;
        while j < spans.len()
            && spans[j].book_num == first.book_num
            && spans[j].start_chapter == first.start_chapter
            && spans[j].end_chapter == first.end_chapter
            && spans[j].start_verse > 0
            && first.start_verse > 0
        {
            j += 1;
        }
        if j - idx > 1 {
            let mut verse_parts = Vec::with_capacity(j - idx);
            for s in &spans[idx..j] {
                if s.start_verse == s.end_verse {
                    verse_parts.push(s.start_verse.to_string());
                } else {
                    verse_parts.push(format!("{}\u{2013}{}", s.start_verse, s.end_verse));
                }
            }
            let name = book_by_num(first.book_num)
                .map(|b| b.name)
                .unwrap_or(&first.book_name);
            groups.push(format!(
                "{name} {}:{}",
                first.start_chapter,
                verse_parts.join(", ")
            ));
            idx = j;
        } else {
            groups.push(first.normalized.clone());
            idx += 1;
        }
    }
    groups.join("; ")
}
