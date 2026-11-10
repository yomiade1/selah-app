//! Bounded, hardened Bible importers for OSIS, USFM, and Zefania XML (SEC-03, M2-A5).
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use selah_proto::v1::{BibleFormat, BibleTranslation};

use super::books::{BookSpec, book_by_code, book_by_num, book_by_osis, resolve_book_alias};
use crate::library::{
    BibleVerseRecord, CommandBusError, Library, is_valid_translation_code, open_bible_db,
    write_bible_database_with_meta,
};

pub const MAX_BIBLE_FILE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_XML_DEPTH: usize = 32;
pub const MAX_XML_EVENTS: usize = 2_000_000;
pub const MAX_VERSES_PER_BIBLE: usize = 50_000;
pub const MAX_VERSE_TEXT_BYTES: usize = 16 * 1024;
const MAX_CODE_BYTES: usize = 32;
const MAX_NAME_BYTES: usize = 128;
const MAX_LANGUAGE_BYTES: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedBibleVerse {
    pub book_num: u32,
    pub book_code: String,
    pub book_name: String,
    pub chapter: u32,
    pub verse: u32,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct ParsedBible {
    pub code: String,
    pub name: String,
    pub language: String,
    pub format: BibleFormat,
    pub verses: Vec<OwnedBibleVerse>,
}

#[derive(Clone, Debug)]
pub struct BibleImportRequest<'a> {
    pub source_path: &'a str,
    pub raw_bytes: &'a [u8],
    pub format: BibleFormat,
    pub code: &'a str,
    pub name: &'a str,
    pub language: &'a str,
    pub replace_existing: bool,
}

pub fn detect_bible_format(text: &str) -> anyhow::Result<BibleFormat> {
    let trimmed = text.trim_start_matches('\u{FEFF}').trim_start();
    let probe_end = trimmed
        .char_indices()
        .take_while(|&(idx, _)| idx < 16 * 1024)
        .last()
        .map(|(idx, ch)| idx + ch.len_utf8())
        .unwrap_or(0);
    let prefix = &trimmed[..probe_end];
    let lower = prefix.to_ascii_lowercase();

    if lower.starts_with('<') || lower.contains("<?xml") {
        if lower.contains("<xmlbible") || lower.contains("<biblebook") {
            return Ok(BibleFormat::Zefania);
        }
        if lower.contains("<osis")
            || lower.contains("<osistext")
            || (lower.contains("<verse") && lower.contains("osisid"))
        {
            return Ok(BibleFormat::Osis);
        }
    }

    for line in prefix.lines() {
        let l = line.trim_start();
        if l.starts_with("\\id") || l.starts_with("\\c ") || l.starts_with("\\v ") {
            return Ok(BibleFormat::Usfm);
        }
    }

    if lower.contains("<xmlbible") || lower.contains("<biblebook") {
        return Ok(BibleFormat::Zefania);
    }
    if lower.contains("<osis") || lower.contains("<osistext") {
        return Ok(BibleFormat::Osis);
    }

    anyhow::bail!("could not detect Bible format; specify OSIS, USFM, or Zefania")
}

fn normalize_verse_text(raw: &str) -> anyhow::Result<String> {
    ensure!(!raw.contains('\0'), "verse text contains a NUL byte");
    let mut out = String::with_capacity(raw.len());
    let mut prev_ws = true;
    for ch in raw.chars() {
        ensure!(
            !ch.is_control() || ch.is_whitespace(),
            "verse text contains control characters"
        );
        if ch.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    let trimmed = out.trim();
    ensure!(
        trimmed.len() <= MAX_VERSE_TEXT_BYTES,
        "verse text exceeds {MAX_VERSE_TEXT_BYTES} bytes"
    );
    Ok(trimmed.to_owned())
}

fn push_space_if_needed(buf: &mut String) {
    if !buf.is_empty() && !buf.ends_with(char::is_whitespace) {
        buf.push(' ');
    }
}

fn append_text_bounded(buf: &mut String, piece: &str) -> anyhow::Result<()> {
    ensure!(!piece.contains('\0'), "Bible text contains a NUL byte");
    ensure!(
        buf.len().saturating_add(piece.len()) <= MAX_VERSE_TEXT_BYTES * 2,
        "verse buffer exceeds size limit"
    );
    buf.push_str(piece);
    Ok(())
}

fn resolve_xml_general_ref(name: &str) -> anyhow::Result<&'static str> {
    if let Some(resolved) = resolve_predefined_entity(name) {
        return Ok(resolved);
    }
    match name {
        "nbsp" => Ok(" "),
        "ndash" => Ok("\u{2013}"),
        "mdash" => Ok("\u{2014}"),
        "lsquo" => Ok("\u{2018}"),
        "rsquo" => Ok("\u{2019}"),
        "ldquo" => Ok("\u{201C}"),
        "rdquo" => Ok("\u{201D}"),
        _ => anyhow::bail!("unsupported XML entity reference '&{name};'"),
    }
}

fn append_general_ref(buf: &mut String, r: &quick_xml::events::BytesRef<'_>) -> anyhow::Result<()> {
    if r.is_char_ref() {
        let ch = r
            .resolve_char_ref()
            .context("invalid XML character reference")?
            .context("empty XML character reference")?;
        ensure!(ch != '\0', "XML character reference cannot be NUL");
        ensure!(
            buf.len().saturating_add(ch.len_utf8()) <= MAX_VERSE_TEXT_BYTES * 2,
            "verse buffer exceeds size limit"
        );
        buf.push(ch);
        return Ok(());
    }
    let decoded = r.decode().context("invalid UTF-8 in XML entity")?;
    let resolved = resolve_xml_general_ref(decoded.as_ref())?;
    append_text_bounded(buf, resolved)
}

fn attr_value(elem: &BytesStart<'_>, target_local: &[u8]) -> anyhow::Result<Option<String>> {
    for attr in elem.attributes() {
        let attr = attr.context("malformed XML attribute")?;
        if attr
            .key
            .local_name()
            .as_ref()
            .eq_ignore_ascii_case(target_local)
        {
            let val = attr
                .normalized_value(XmlVersion::Implicit1_0)
                .context("invalid XML attribute value")?;
            ensure!(!val.contains('\0'), "XML attribute contains a NUL byte");
            return Ok(Some(val.into_owned()));
        }
    }
    Ok(None)
}

fn normalize_language_code(raw: &str) -> String {
    let t = raw.trim().to_ascii_lowercase();
    match t.as_str() {
        "eng" | "english" => "en".into(),
        "spa" | "esp" | "spanish" | "español" | "espanol" => "es".into(),
        "por" | "portuguese" | "português" | "portugues" => "pt".into(),
        "fra" | "fre" | "french" | "français" | "francais" => "fr".into(),
        "kor" | "korean" | "한국어" => "ko".into(),
        "yor" | "yoruba" | "yorùbá" => "yo".into(),
        other if !other.is_empty() && other.len() <= MAX_LANGUAGE_BYTES => other.to_owned(),
        _ => "en".into(),
    }
}

fn sanitize_fallback_code(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(MAX_CODE_BYTES)
        .collect();
    if cleaned.is_empty() {
        "IMPORTED".into()
    } else {
        cleaned.to_ascii_uppercase()
    }
}

fn parse_pos_verse_int(raw: &str) -> Option<u32> {
    let digits: String = raw
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let val: u32 = digits.parse().ok()?;
    (val > 0 && val <= 2000).then_some(val)
}

fn lookup_import_book(token: &str) -> Option<&'static BookSpec> {
    let t = token.trim();
    if t.is_empty() {
        return None;
    }
    // Common USFM/Paratext variants before fallback
    let mapped = match t.to_ascii_uppercase().as_str() {
        "EZE" => "EZK",
        "JOE" => "JOL",
        "NAH" => "NAM",
        "JAM" => "JAS",
        "PHL" | "PHI" => "PHP",
        "SOL" | "SOS" => "SNG",
        "PSM" => "PSA",
        "PRV" => "PRO",
        _ => t,
    };
    book_by_code(mapped)
        .or_else(|| book_by_osis(t))
        .or_else(|| resolve_book_alias(t, None))
}

fn parse_osis_verse_id(raw: &str) -> Option<(&'static BookSpec, u32, u32)> {
    // Handle multi-ID attributes like "John.3.16 John.3.17" or work-prefixed "KJV:John.3.16"
    let first = raw.split_whitespace().next()?;
    let without_work = first.rsplit_once(':').map(|(_, r)| r).unwrap_or(first);
    let mut parts = without_work.split('.');
    let book_tok = parts.next()?;
    let ch_tok = parts.next()?;
    let v_tok = parts.next()?;
    let book = lookup_import_book(book_tok)?;
    let ch = parse_pos_verse_int(ch_tok)?;
    let v = parse_pos_verse_int(v_tok)?;
    Some((book, ch, v))
}

fn insert_verse(
    map: &mut BTreeMap<(u32, u32, u32), OwnedBibleVerse>,
    book: &BookSpec,
    chapter: u32,
    verse: u32,
    raw_text: &str,
) -> anyhow::Result<()> {
    let text = normalize_verse_text(raw_text)?;
    if text.is_empty() {
        return Ok(());
    }
    let key = (book.book_num, chapter, verse);
    if !map.contains_key(&key) {
        ensure!(
            map.len() < MAX_VERSES_PER_BIBLE,
            "Bible exceeds maximum {MAX_VERSES_PER_BIBLE} verses"
        );
        map.insert(
            key,
            OwnedBibleVerse {
                book_num: book.book_num,
                book_code: book.code.to_owned(),
                book_name: book.name.to_owned(),
                chapter,
                verse,
                text,
            },
        );
    } else if let Some(existing) = map.get_mut(&key) {
        push_space_if_needed(&mut existing.text);
        existing.text.push_str(&text);
        ensure!(
            existing.text.len() <= MAX_VERSE_TEXT_BYTES,
            "merged verse text exceeds {MAX_VERSE_TEXT_BYTES} bytes"
        );
    }
    Ok(())
}

/// Parse OSIS XML (supports both container `<verse osisID=...>` and milestone `<verse osisID=... sID=.../>...<verse eID=.../>`).
pub fn parse_osis(text: &str) -> anyhow::Result<ParsedBible> {
    ensure!(!text.contains('\0'), "OSIS input contains a NUL byte");
    let clean_text = text.trim_start_matches('\u{FEFF}');
    let mut reader = Reader::from_str(clean_text);
    reader.config_mut().trim_text(false);

    let mut depth = 0usize;
    let mut events = 0usize;
    let mut verses: BTreeMap<(u32, u32, u32), OwnedBibleVerse> = BTreeMap::new();

    #[derive(Clone, Debug)]
    struct ActiveOsisVerse {
        book: &'static BookSpec,
        chapter: u32,
        verse: u32,
        sid: Option<String>,
        text: String,
    }

    let mut active_verse: Option<ActiveOsisVerse> = None;
    let mut note_depth = 0usize;
    let mut skip_title_depth = 0usize;
    let mut header_depth = 0usize;
    let mut meta_title_capture = false;
    let mut meta_id_capture = false;
    let mut meta_lang_capture = false;
    let mut meta_buf = String::new();

    let mut detected_code = String::new();
    let mut detected_name = String::new();
    let mut detected_lang = String::new();

    loop {
        events += 1;
        ensure!(
            events <= MAX_XML_EVENTS,
            "OSIS XML exceeds maximum {MAX_XML_EVENTS} events"
        );
        match reader.read_event().context("malformed OSIS XML")? {
            Event::Eof => break,
            Event::DocType(_) => anyhow::bail!("XML DOCTYPE/DTD declarations are not allowed"),
            Event::Start(e) => {
                depth += 1;
                ensure!(
                    depth <= MAX_XML_DEPTH,
                    "OSIS XML nesting exceeds maximum depth {MAX_XML_DEPTH}"
                );
                let local = e.local_name();
                let name = local.as_ref();
                if name.eq_ignore_ascii_case(b"osisText")
                    && let Some(work) = attr_value(&e, b"osisIDWork")?
                    && detected_code.is_empty()
                {
                    detected_code = work;
                } else if name.eq_ignore_ascii_case(b"header") {
                    header_depth += 1;
                } else if header_depth > 0 {
                    if name.eq_ignore_ascii_case(b"title") && detected_name.is_empty() {
                        meta_title_capture = true;
                        meta_buf.clear();
                    } else if name.eq_ignore_ascii_case(b"identifier") && detected_code.is_empty() {
                        meta_id_capture = true;
                        meta_buf.clear();
                    } else if name.eq_ignore_ascii_case(b"language") && detected_lang.is_empty() {
                        meta_lang_capture = true;
                        meta_buf.clear();
                    }
                } else if name.eq_ignore_ascii_case(b"note") {
                    note_depth += 1;
                } else if name.eq_ignore_ascii_case(b"title") && note_depth == 0 {
                    skip_title_depth += 1;
                } else if name.eq_ignore_ascii_case(b"verse") {
                    ensure!(
                        active_verse.is_none(),
                        "nested OSIS <verse> tags are not supported"
                    );
                    if let Some(osis_id) = attr_value(&e, b"osisID")?
                        && let Some((book, ch, v)) = parse_osis_verse_id(&osis_id)
                    {
                        let sid = attr_value(&e, b"sID")?;
                        active_verse = Some(ActiveOsisVerse {
                            book,
                            chapter: ch,
                            verse: v,
                            sid,
                            text: String::new(),
                        });
                    }
                } else if let Some(av) = active_verse.as_mut()
                    && note_depth == 0
                    && skip_title_depth == 0
                    && (name.eq_ignore_ascii_case(b"l")
                        || name.eq_ignore_ascii_case(b"p")
                        || name.eq_ignore_ascii_case(b"lg"))
                {
                    push_space_if_needed(&mut av.text);
                }
            }
            Event::Empty(e) => {
                let local = e.local_name();
                let name = local.as_ref();
                if name.eq_ignore_ascii_case(b"verse") {
                    if let Some(eid) = attr_value(&e, b"eID")? {
                        if let Some(av) = active_verse.take() {
                            if let Some(ref expected_sid) = av.sid {
                                ensure!(
                                    expected_sid == &eid,
                                    "mismatched OSIS verse eID '{eid}' for sID '{expected_sid}'"
                                );
                            }
                            insert_verse(&mut verses, av.book, av.chapter, av.verse, &av.text)?;
                        }
                    } else if let Some(sid) = attr_value(&e, b"sID")?
                        && let Some(osis_id) = attr_value(&e, b"osisID")?
                        && let Some((book, ch, v)) = parse_osis_verse_id(&osis_id)
                    {
                        ensure!(
                            active_verse.is_none(),
                            "unclosed OSIS milestone verse before sID '{sid}'"
                        );
                        active_verse = Some(ActiveOsisVerse {
                            book,
                            chapter: ch,
                            verse: v,
                            sid: Some(sid),
                            text: String::new(),
                        });
                    }
                } else if let Some(av) = active_verse.as_mut()
                    && note_depth == 0
                    && skip_title_depth == 0
                    && (name.eq_ignore_ascii_case(b"lb") || name.eq_ignore_ascii_case(b"milestone"))
                {
                    push_space_if_needed(&mut av.text);
                }
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                let local = e.local_name();
                let name = local.as_ref();
                if header_depth > 0 {
                    if name.eq_ignore_ascii_case(b"header") {
                        header_depth = header_depth.saturating_sub(1);
                    } else if name.eq_ignore_ascii_case(b"title") && meta_title_capture {
                        detected_name = meta_buf.trim().to_owned();
                        meta_title_capture = false;
                        meta_buf.clear();
                    } else if name.eq_ignore_ascii_case(b"identifier") && meta_id_capture {
                        detected_code = meta_buf.trim().to_owned();
                        meta_id_capture = false;
                        meta_buf.clear();
                    } else if name.eq_ignore_ascii_case(b"language") && meta_lang_capture {
                        detected_lang = meta_buf.trim().to_owned();
                        meta_lang_capture = false;
                        meta_buf.clear();
                    }
                } else if name.eq_ignore_ascii_case(b"note") {
                    note_depth = note_depth.saturating_sub(1);
                } else if name.eq_ignore_ascii_case(b"title") && skip_title_depth > 0 {
                    skip_title_depth = skip_title_depth.saturating_sub(1);
                } else if name.eq_ignore_ascii_case(b"verse") {
                    if let Some(av) = active_verse.take() {
                        ensure!(
                            av.sid.is_none(),
                            "OSIS milestone verse had matching container end tag"
                        );
                        insert_verse(&mut verses, av.book, av.chapter, av.verse, &av.text)?;
                    }
                } else if let Some(av) = active_verse.as_mut()
                    && note_depth == 0
                    && skip_title_depth == 0
                    && (name.eq_ignore_ascii_case(b"l") || name.eq_ignore_ascii_case(b"p"))
                {
                    push_space_if_needed(&mut av.text);
                }
            }
            Event::Text(e) => {
                let s = e.decode().context("invalid UTF-8 in OSIS text")?;
                if meta_title_capture || meta_id_capture || meta_lang_capture {
                    if meta_buf.len().saturating_add(s.len()) <= 512 {
                        meta_buf.push_str(s.as_ref());
                    }
                } else if let Some(av) = active_verse.as_mut()
                    && note_depth == 0
                    && skip_title_depth == 0
                {
                    append_text_bounded(&mut av.text, s.as_ref())?;
                }
            }
            Event::CData(e) => {
                let s = e.decode().context("invalid UTF-8 in OSIS CDATA")?;
                if let Some(av) = active_verse.as_mut()
                    && note_depth == 0
                    && skip_title_depth == 0
                {
                    append_text_bounded(&mut av.text, s.as_ref())?;
                }
            }
            Event::GeneralRef(r) => {
                if meta_title_capture || meta_id_capture || meta_lang_capture {
                    append_general_ref(&mut meta_buf, &r)?;
                } else if let Some(av) = active_verse.as_mut()
                    && note_depth == 0
                    && skip_title_depth == 0
                {
                    append_general_ref(&mut av.text, &r)?;
                }
            }
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
        }
    }

    ensure!(
        active_verse.is_none(),
        "unclosed OSIS <verse> at end of document"
    );
    ensure!(
        !verses.is_empty(),
        "OSIS document contains no valid Bible verses"
    );

    let code = sanitize_fallback_code(&detected_code);
    let name = if detected_name.is_empty() {
        code.clone()
    } else {
        detected_name
    };
    let language = normalize_language_code(&detected_lang);

    Ok(ParsedBible {
        code,
        name,
        language,
        format: BibleFormat::Osis,
        verses: verses.into_values().collect(),
    })
}

/// Parse Zefania XML (`<XMLBIBLE>` / `<BIBLEBOOK>` / `<CHAPTER>` / `<VERS>`).
pub fn parse_zefania(text: &str) -> anyhow::Result<ParsedBible> {
    ensure!(!text.contains('\0'), "Zefania input contains a NUL byte");
    let clean_text = text.trim_start_matches('\u{FEFF}');
    let mut reader = Reader::from_str(clean_text);
    reader.config_mut().trim_text(false);

    let mut depth = 0usize;
    let mut events = 0usize;
    let mut verses: BTreeMap<(u32, u32, u32), OwnedBibleVerse> = BTreeMap::new();

    let mut current_book: Option<&'static BookSpec> = None;
    let mut current_chapter: Option<u32> = None;
    let mut active_verse: Option<(u32, String)> = None;
    let mut note_depth = 0usize;
    let mut in_information = false;
    let mut info_field: Option<String> = None;
    let mut info_buf = String::new();

    let mut detected_code = String::new();
    let mut detected_name = String::new();
    let mut detected_lang = String::new();

    loop {
        events += 1;
        ensure!(
            events <= MAX_XML_EVENTS,
            "Zefania XML exceeds maximum {MAX_XML_EVENTS} events"
        );
        match reader.read_event().context("malformed Zefania XML")? {
            Event::Eof => break,
            Event::DocType(_) => anyhow::bail!("XML DOCTYPE/DTD declarations are not allowed"),
            Event::Start(e) => {
                depth += 1;
                ensure!(
                    depth <= MAX_XML_DEPTH,
                    "Zefania XML nesting exceeds maximum depth {MAX_XML_DEPTH}"
                );
                let local = e.local_name();
                let name = local.as_ref();
                if name.eq_ignore_ascii_case(b"XMLBIBLE") {
                    if let Some(biblename) = attr_value(&e, b"biblename")?
                        && detected_name.is_empty()
                    {
                        detected_name = biblename;
                    }
                } else if name.eq_ignore_ascii_case(b"INFORMATION") {
                    in_information = true;
                } else if in_information {
                    info_field = Some(String::from_utf8_lossy(name).to_ascii_lowercase());
                    info_buf.clear();
                } else if name.eq_ignore_ascii_case(b"NOTE")
                    || name.eq_ignore_ascii_case(b"CAPTION")
                    || name.eq_ignore_ascii_case(b"PROLOG")
                {
                    note_depth += 1;
                } else if name.eq_ignore_ascii_case(b"BIBLEBOOK") {
                    let mut resolved = None;
                    if let Some(num_str) = attr_value(&e, b"bnumber")?
                        && let Some(num) = parse_pos_verse_int(&num_str)
                    {
                        resolved = book_by_num(num);
                    }
                    if resolved.is_none()
                        && let Some(bname) = attr_value(&e, b"bname")?
                    {
                        resolved = lookup_import_book(&bname);
                    }
                    if resolved.is_none()
                        && let Some(bsname) = attr_value(&e, b"bsname")?
                    {
                        resolved = lookup_import_book(&bsname);
                    }
                    current_book = resolved;
                    current_chapter = None;
                } else if name.eq_ignore_ascii_case(b"CHAPTER") {
                    current_chapter = attr_value(&e, b"cnumber")?
                        .as_deref()
                        .and_then(parse_pos_verse_int);
                } else if name.eq_ignore_ascii_case(b"VERS") {
                    ensure!(
                        active_verse.is_none(),
                        "nested Zefania <VERS> tags are not supported"
                    );
                    if let Some(v_str) = attr_value(&e, b"vnumber")?
                        && let Some(v_num) = parse_pos_verse_int(&v_str)
                    {
                        active_verse = Some((v_num, String::new()));
                    }
                }
            }
            Event::Empty(e) => {
                let local = e.local_name();
                let name = local.as_ref();
                if let Some((_, ref mut buf)) = active_verse
                    && note_depth == 0
                    && name.eq_ignore_ascii_case(b"BR")
                {
                    push_space_if_needed(buf);
                }
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                let local = e.local_name();
                let name = local.as_ref();
                if name.eq_ignore_ascii_case(b"INFORMATION") {
                    in_information = false;
                    info_field = None;
                    info_buf.clear();
                } else if in_information {
                    if let Some(field) = info_field.take() {
                        let val = info_buf.trim().to_owned();
                        if !val.is_empty() {
                            match field.as_str() {
                                "title" if detected_name.is_empty() => detected_name = val,
                                "identifier" if detected_code.is_empty() => detected_code = val,
                                "language" if detected_lang.is_empty() => detected_lang = val,
                                _ => {}
                            }
                        }
                    }
                    info_buf.clear();
                } else if name.eq_ignore_ascii_case(b"NOTE")
                    || name.eq_ignore_ascii_case(b"CAPTION")
                    || name.eq_ignore_ascii_case(b"PROLOG")
                {
                    note_depth = note_depth.saturating_sub(1);
                } else if name.eq_ignore_ascii_case(b"VERS") {
                    if let Some((v_num, buf)) = active_verse.take()
                        && let (Some(book), Some(ch)) = (current_book, current_chapter)
                    {
                        insert_verse(&mut verses, book, ch, v_num, &buf)?;
                    }
                } else if name.eq_ignore_ascii_case(b"CHAPTER") {
                    current_chapter = None;
                } else if name.eq_ignore_ascii_case(b"BIBLEBOOK") {
                    current_book = None;
                    current_chapter = None;
                }
            }
            Event::Text(e) => {
                let s = e.decode().context("invalid UTF-8 in Zefania text")?;
                if info_field.is_some() {
                    if info_buf.len().saturating_add(s.len()) <= 512 {
                        info_buf.push_str(s.as_ref());
                    }
                } else if let Some((_, ref mut buf)) = active_verse
                    && note_depth == 0
                {
                    append_text_bounded(buf, s.as_ref())?;
                }
            }
            Event::CData(e) => {
                let s = e.decode().context("invalid UTF-8 in Zefania CDATA")?;
                if let Some((_, ref mut buf)) = active_verse
                    && note_depth == 0
                {
                    append_text_bounded(buf, s.as_ref())?;
                }
            }
            Event::GeneralRef(r) => {
                if info_field.is_some() {
                    append_general_ref(&mut info_buf, &r)?;
                } else if let Some((_, ref mut buf)) = active_verse
                    && note_depth == 0
                {
                    append_general_ref(buf, &r)?;
                }
            }
            Event::Comment(_) | Event::Decl(_) | Event::PI(_) => {}
        }
    }

    ensure!(
        active_verse.is_none(),
        "unclosed Zefania <VERS> at end of document"
    );
    ensure!(
        !verses.is_empty(),
        "Zefania document contains no valid Bible verses"
    );

    let code = sanitize_fallback_code(if detected_code.is_empty() {
        &detected_name
    } else {
        &detected_code
    });
    let name = if detected_name.is_empty() {
        code.clone()
    } else {
        detected_name
    };
    let language = normalize_language_code(&detected_lang);

    Ok(ParsedBible {
        code,
        name,
        language,
        format: BibleFormat::Zefania,
        verses: verses.into_values().collect(),
    })
}

fn strip_usfm_inline(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut i = 0usize;
    let bytes = raw.as_bytes();
    let mut in_word_attrs = false;
    let mut in_word_span = false;

    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let rest = &raw[i + 1..];
            // Closing note markers handled inside note branch; standalone * or marker*
            if let Some(note_end_marker) = if rest.starts_with("fe ") || rest.starts_with("fe+") {
                Some("\\fe*")
            } else if rest.starts_with("f ") || rest.starts_with("f+") {
                Some("\\f*")
            } else if rest.starts_with("x ") || rest.starts_with("x+") {
                Some("\\x*")
            } else {
                None
            } {
                if let Some(close_rel) = raw[i..].find(note_end_marker) {
                    i += close_rel + note_end_marker.len();
                } else {
                    break;
                }
                continue;
            }

            // General inline marker: \+?marker\*?
            let mut m_end = 0usize;
            for (rel, ch) in rest.char_indices() {
                if ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '_' {
                    m_end = rel + ch.len_utf8();
                } else {
                    break;
                }
            }
            if m_end > 0 {
                let marker = &rest[..m_end];
                let after_marker = i + 1 + m_end;
                let has_star = bytes.get(after_marker) == Some(&b'*');
                let clean_marker = marker.trim_start_matches('+');
                if has_star {
                    if clean_marker == "w" {
                        in_word_span = false;
                        in_word_attrs = false;
                    }
                    i = after_marker + 1;
                    continue;
                }
                if clean_marker == "w" {
                    in_word_span = true;
                    in_word_attrs = false;
                }
                i = after_marker;
                if bytes.get(i) == Some(&b' ') {
                    i += 1;
                }
                continue;
            }
        }

        let ch = raw[i..].chars().next().unwrap_or(' ');
        let len = ch.len_utf8();
        if in_word_span && ch == '|' {
            in_word_attrs = true;
            i += len;
            continue;
        }
        if in_word_attrs {
            i += len;
            continue;
        }
        if ch == '~' {
            out.push(' ');
        } else if ch == '/' && raw[i..].starts_with("//") {
            out.push(' ');
            i += 2;
            continue;
        } else {
            out.push(ch);
        }
        i += len;
    }

    out
}

fn is_usfm_verse_continuation_marker(marker: &str) -> bool {
    matches!(
        marker,
        "p" | "m"
            | "pmo"
            | "pm"
            | "pmc"
            | "pmr"
            | "pi"
            | "pi1"
            | "pi2"
            | "pi3"
            | "mi"
            | "nb"
            | "pc"
            | "q"
            | "q1"
            | "q2"
            | "q3"
            | "q4"
            | "qc"
            | "qr"
            | "qm"
            | "qm1"
            | "qm2"
            | "li"
            | "li1"
            | "li2"
            | "li3"
            | "b"
            | "cls"
    )
}

/// Parse USFM (`\id`, `\c`, `\v`, continuation lines `\p`/`\q*`, inline footnotes/words).
pub fn parse_usfm(text: &str) -> anyhow::Result<ParsedBible> {
    ensure!(!text.contains('\0'), "USFM input contains a NUL byte");
    let clean_text = text.trim_start_matches('\u{FEFF}');
    let mut verses: BTreeMap<(u32, u32, u32), OwnedBibleVerse> = BTreeMap::new();

    let mut current_book: Option<&'static BookSpec> = None;
    let mut current_chapter: Option<u32> = None;
    let mut active_verse: Option<(u32, String)> = None;
    let mut in_verse_text = false;
    let mut detected_name = String::new();
    let mut detected_code = String::new();

    let flush_verse = |verses: &mut BTreeMap<(u32, u32, u32), OwnedBibleVerse>,
                       book: Option<&'static BookSpec>,
                       chapter: Option<u32>,
                       active: &mut Option<(u32, String)>|
     -> anyhow::Result<()> {
        if let Some((v_num, raw)) = active.take()
            && let (Some(b), Some(c)) = (book, chapter)
        {
            let cleaned = strip_usfm_inline(&raw);
            insert_verse(verses, b, c, v_num, &cleaned)?;
        }
        Ok(())
    };

    for raw_line in clean_text.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(after_slash) = trimmed.strip_prefix('\\') {
            let mut split_at = after_slash.len();
            for (idx, ch) in after_slash.char_indices() {
                if ch.is_whitespace() {
                    split_at = idx;
                    break;
                }
            }
            let marker = &after_slash[..split_at];
            let rest = after_slash[split_at..].trim_start();

            if marker == "id" {
                flush_verse(
                    &mut verses,
                    current_book,
                    current_chapter,
                    &mut active_verse,
                )?;
                in_verse_text = false;
                current_chapter = None;
                let mut parts = rest.splitn(2, char::is_whitespace);
                let book_tok = parts.next().unwrap_or("");
                let desc = parts.next().unwrap_or("").trim();
                current_book = lookup_import_book(book_tok);
                if detected_name.is_empty() && !desc.is_empty() {
                    detected_name = desc.chars().take(64).collect();
                }
                if detected_code.is_empty() && current_book.is_some() && !desc.is_empty() {
                    detected_code =
                        sanitize_fallback_code(desc.split_whitespace().next().unwrap_or(""));
                }
            } else if marker == "c" {
                flush_verse(
                    &mut verses,
                    current_book,
                    current_chapter,
                    &mut active_verse,
                )?;
                in_verse_text = false;
                current_chapter = rest.split_whitespace().next().and_then(parse_pos_verse_int);
            } else if marker == "v" {
                flush_verse(
                    &mut verses,
                    current_book,
                    current_chapter,
                    &mut active_verse,
                )?;
                if current_book.is_some() && current_chapter.is_some() {
                    let mut parts = rest.splitn(2, char::is_whitespace);
                    let v_tok = parts.next().unwrap_or("");
                    let v_body = parts.next().unwrap_or("");
                    if let Some(v_num) = parse_pos_verse_int(v_tok) {
                        let mut buf = String::new();
                        append_text_bounded(&mut buf, v_body)?;
                        active_verse = Some((v_num, buf));
                        in_verse_text = true;
                    } else {
                        in_verse_text = false;
                    }
                }
            } else if is_usfm_verse_continuation_marker(marker) {
                if let Some((_, ref mut buf)) = active_verse {
                    in_verse_text = true;
                    if !rest.is_empty() {
                        push_space_if_needed(buf);
                        append_text_bounded(buf, rest)?;
                    }
                }
            } else {
                // Non-verse structural marker (\s1, \r, \mt, \rem, \h, \toc1, etc.)
                in_verse_text = false;
            }
        } else if in_verse_text && let Some((_, ref mut buf)) = active_verse {
            push_space_if_needed(buf);
            append_text_bounded(buf, trimmed)?;
        }
    }

    flush_verse(
        &mut verses,
        current_book,
        current_chapter,
        &mut active_verse,
    )?;
    ensure!(
        !verses.is_empty(),
        "USFM document contains no valid Bible verses"
    );

    let code = if detected_code.is_empty() {
        "USFM".into()
    } else {
        detected_code
    };
    let name = if detected_name.is_empty() {
        "Imported USFM Bible".into()
    } else {
        detected_name
    };

    Ok(ParsedBible {
        code,
        name,
        language: "en".into(),
        format: BibleFormat::Usfm,
        verses: verses.into_values().collect(),
    })
}

pub fn parse_bible_bytes(bytes: &[u8], format: BibleFormat) -> anyhow::Result<ParsedBible> {
    ensure!(
        bytes.len() <= MAX_BIBLE_FILE_BYTES,
        "Bible file exceeds maximum {MAX_BIBLE_FILE_BYTES} bytes"
    );
    ensure!(!bytes.is_empty(), "Bible input cannot be empty");
    let text = std::str::from_utf8(bytes).context("Bible file is not valid UTF-8")?;
    ensure!(!text.contains('\0'), "Bible input contains a NUL byte");

    let resolved = match format {
        BibleFormat::Unspecified => detect_bible_format(text)?,
        other => other,
    };

    match resolved {
        BibleFormat::Osis => parse_osis(text),
        BibleFormat::Usfm => parse_usfm(text),
        BibleFormat::Zefania => parse_zefania(text),
        BibleFormat::Unspecified => unreachable!(),
    }
}

fn read_bounded_bible_file(path_str: &str) -> Result<Vec<u8>, CommandBusError> {
    if path_str.contains('\0') {
        return Err(CommandBusError::Invalid(
            "Bible file path contains a NUL byte".into(),
        ));
    }
    let path = Path::new(path_str);
    let meta = std::fs::symlink_metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            CommandBusError::NotFound("Bible source file not found".into())
        } else {
            CommandBusError::Invalid(format!("cannot inspect Bible source file: {e}"))
        }
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(CommandBusError::Invalid(
            "Bible source must be a regular non-symlink file".into(),
        ));
    }
    if meta.len() > MAX_BIBLE_FILE_BYTES as u64 {
        return Err(CommandBusError::Invalid(format!(
            "Bible source exceeds {MAX_BIBLE_FILE_BYTES} bytes"
        )));
    }
    let file = std::fs::File::open(path)
        .map_err(|e| CommandBusError::Invalid(format!("cannot open Bible source file: {e}")))?;
    let mut limited = file.take((MAX_BIBLE_FILE_BYTES + 1) as u64);
    let mut bytes = Vec::with_capacity(meta.len().min(4 * 1024 * 1024) as usize);
    limited
        .read_to_end(&mut bytes)
        .map_err(|e| CommandBusError::Invalid(format!("failed reading Bible source file: {e}")))?;
    if bytes.len() > MAX_BIBLE_FILE_BYTES {
        return Err(CommandBusError::Invalid(format!(
            "Bible source exceeds {MAX_BIBLE_FILE_BYTES} bytes"
        )));
    }
    Ok(bytes)
}

pub fn import_bible(
    library: &Library,
    request: &BibleImportRequest<'_>,
) -> Result<BibleTranslation, CommandBusError> {
    let format = BibleFormat::try_from(request.format as i32)
        .map_err(|_| CommandBusError::Invalid("unknown Bible format".into()))?;
    let owned_bytes;
    let payload: &[u8] = if !request.raw_bytes.is_empty() {
        if request.raw_bytes.len() > MAX_BIBLE_FILE_BYTES {
            return Err(CommandBusError::Invalid(format!(
                "Bible payload exceeds {MAX_BIBLE_FILE_BYTES} bytes"
            )));
        }
        request.raw_bytes
    } else if !request.source_path.trim().is_empty() {
        owned_bytes = read_bounded_bible_file(request.source_path.trim())?;
        &owned_bytes
    } else {
        return Err(CommandBusError::Invalid(
            "either source_path or raw_bytes must be supplied".into(),
        ));
    };

    let parsed =
        parse_bible_bytes(payload, format).map_err(|e| CommandBusError::Invalid(e.to_string()))?;

    let code = if request.code.trim().is_empty() {
        parsed.code
    } else {
        request.code.trim().to_owned()
    };
    if !is_valid_translation_code(&code) || code.contains('\0') {
        return Err(CommandBusError::Invalid(
            "Bible translation code must be 1..=32 ASCII letters, digits, '-' or '_'".into(),
        ));
    }

    let name = if request.name.trim().is_empty() {
        parsed.name
    } else {
        request.name.trim().to_owned()
    };
    if name.is_empty() || name.len() > MAX_NAME_BYTES || name.contains('\0') {
        return Err(CommandBusError::Invalid(
            "Bible name must be 1..=128 bytes without NUL bytes".into(),
        ));
    }

    let language = if request.language.trim().is_empty() {
        parsed.language
    } else {
        normalize_language_code(request.language)
    };
    if language.is_empty() || language.len() > MAX_LANGUAGE_BYTES || language.contains('\0') {
        return Err(CommandBusError::Invalid(
            "Bible language code is invalid".into(),
        ));
    }

    let existing = library
        .list_validated_bible_files()
        .map_err(CommandBusError::Internal)?;
    let prior_match: Option<(String, PathBuf)> = existing
        .into_iter()
        .find(|(existing_code, _)| existing_code.eq_ignore_ascii_case(&code));
    if let Some((ref existing_code, _)) = prior_match
        && !request.replace_existing
    {
        return Err(CommandBusError::Invalid(format!(
            "Bible translation '{existing_code}' already exists"
        )));
    }

    let bibles_root = library
        .paths()
        .bibles
        .canonicalize()
        .map_err(|e| CommandBusError::Internal(e.into()))?;
    let target_path = bibles_root.join(format!("{code}.db"));
    match std::fs::symlink_metadata(&target_path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(CommandBusError::Invalid(
                    "target Bible path is not a regular file".into(),
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(CommandBusError::Internal(e.into())),
    }

    let records: Vec<BibleVerseRecord<'_>> = parsed
        .verses
        .iter()
        .map(|v| BibleVerseRecord {
            book_num: v.book_num,
            book_code: &v.book_code,
            book_name: &v.book_name,
            chapter: v.chapter,
            verse: v.verse,
            text: &v.text,
        })
        .collect();

    write_bible_database_with_meta(&target_path, &code, &name, &language, false, &records)
        .map_err(CommandBusError::Internal)?;

    if let Some((_, prior_path)) = prior_match
        && prior_path != target_path
        && prior_path.exists()
    {
        let _ = std::fs::remove_file(&prior_path);
    }

    let db = open_bible_db(&target_path).map_err(CommandBusError::Internal)?;
    let (book_count, verse_count): (u32, u32) = db.query_row(
        "SELECT (SELECT COUNT(*) FROM books), (SELECT COUNT(*) FROM verses)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;

    Ok(BibleTranslation {
        code,
        name,
        language,
        book_count,
        verse_count,
        is_bundled: false,
        books: Vec::new(),
    })
}
