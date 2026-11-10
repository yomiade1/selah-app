use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use anyhow::ensure;
use prost::Message;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use selah_proto::v1::*;

use super::Library;
use super::ops::CommandBusError;

pub const MAX_SEARCH_QUERY_BYTES: usize = 512;
pub const MAX_SEARCH_LIMIT: u32 = 200;
const MAX_SYNONYM_BYTES: usize = 128;
const MAX_QUERY_TOKENS: usize = 16;
const SEARCH_INDEX_VERSION: &str = "1";
const BIBLE_SCHEMA_VERSION: &str = "1";

const DEFAULT_SYNONYMS: &[(&str, &str)] = &[
    // Songs and worship
    ("xmas", "christmas"),
    ("noel", "christmas"),
    ("resurrection", "easter"),
    ("calvary", "cross"),
    ("hallelujah", "alleluia"),
    ("saviour", "savior"),
    ("honour", "honor"),
    ("favour", "favor"),
    ("neighbour", "neighbor"),
    ("eucharist", "communion"),
    ("baptism", "christening"),
    ("adoration", "worship"),
    ("chorale", "hymn"),
    // Scripture
    ("charity", "love"),
    ("ghost", "spirit"),
    ("brethren", "brothers"),
    ("comforter", "advocate"),
    // Media
    ("backdrop", "background"),
    ("wallpaper", "background"),
    ("photo", "image"),
    ("photograph", "image"),
    ("picture", "image"),
    ("clip", "video"),
    ("movie", "video"),
    ("track", "audio"),
];

#[derive(Clone, Copy, Debug)]
pub struct BibleVerseRecord<'a> {
    pub book_num: u32,
    pub book_code: &'a str,
    pub book_name: &'a str,
    pub chapter: u32,
    pub verse: u32,
    pub text: &'a str,
}

pub(crate) fn fold_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            // Drop common Unicode combining diacritical marks so decomposed accents
            // (for example Yoruba tone + underdot) fold reliably for trigrams.
            '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{FE20}'..='\u{FE2F}' => continue,
            '’' | '‘' | 'ʼ' => out.push('\''),
            '–' | '—' | '−' => out.push('-'),
            'æ' | 'Æ' => out.push_str("ae"),
            'œ' | 'Œ' => out.push_str("oe"),
            'ß' => out.push_str("ss"),
            _ => {
                for lower in ch.to_lowercase() {
                    let mapped = match lower {
                        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' | 'ạ' | 'ả' | 'ấ'
                        | 'ầ' | 'ẩ' | 'ẫ' | 'ậ' | 'ắ' | 'ằ' | 'ẳ' | 'ẵ' | 'ặ' => {
                            'a'
                        }
                        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
                        'ď' | 'đ' => 'd',
                        'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' | 'ẹ' | 'ẻ' | 'ẽ'
                        | 'ế' | 'ề' | 'ể' | 'ễ' | 'ệ' => 'e',
                        'ğ' | 'ĝ' | 'ġ' | 'ģ' => 'g',
                        'ĥ' | 'ħ' => 'h',
                        'í' | 'ì' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' | 'ị' | 'ỉ' => {
                            'i'
                        }
                        'ĵ' => 'j',
                        'ķ' => 'k',
                        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
                        'ḿ' => 'm',
                        'ñ' | 'ń' | 'ņ' | 'ň' | 'ǹ' => 'n',
                        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'ọ' | 'ỏ' | 'ố'
                        | 'ồ' | 'ổ' | 'ỗ' | 'ộ' | 'ớ' | 'ờ' | 'ở' | 'ỡ' | 'ợ' => {
                            'o'
                        }
                        'ŕ' | 'ŗ' | 'ř' => 'r',
                        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' | 'ṣ' => 's',
                        'ţ' | 'ť' | 'ŧ' | 'ț' => 't',
                        'ú' | 'ù' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' | 'ụ' | 'ủ'
                        | 'ứ' | 'ừ' | 'ử' | 'ữ' | 'ự' => 'u',
                        'ŵ' => 'w',
                        'ý' | 'ÿ' | 'ŷ' | 'ỳ' | 'ỵ' | 'ỷ' | 'ỹ' => 'y',
                        'ź' | 'ż' | 'ž' => 'z',
                        other => other,
                    };
                    out.push(mapped);
                }
            }
        }
    }
    out
}

fn tokenize_folded(folded: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for raw in folded.split(|c: char| !c.is_alphanumeric() && c != '\'') {
        let stripped = raw
            .strip_suffix("'s")
            .or_else(|| raw.strip_suffix("'"))
            .unwrap_or(raw);
        for part in stripped.split('\'') {
            if !part.is_empty() {
                tokens.push(part.to_owned());
            }
        }
    }
    tokens
}

fn tokenize_query(folded: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut tokens = Vec::new();
    for token in tokenize_folded(folded) {
        if seen.insert(token.clone()) {
            tokens.push(token);
            if tokens.len() >= MAX_QUERY_TOKENS {
                break;
            }
        }
    }
    tokens
}

fn validate_query(query: &str, limit: u32) -> Result<String, CommandBusError> {
    if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
        return Err(CommandBusError::Invalid(format!(
            "page size must be 1 to {MAX_SEARCH_LIMIT}"
        )));
    }
    if query.contains('\0') {
        return Err(CommandBusError::Invalid(
            "search query contains a NUL character".into(),
        ));
    }
    if query.len() > MAX_SEARCH_QUERY_BYTES {
        return Err(CommandBusError::Invalid(format!(
            "search query exceeds {MAX_SEARCH_QUERY_BYTES} bytes"
        )));
    }
    Ok(query.trim().to_owned())
}

fn validate_synonym_term(term: &str) -> anyhow::Result<String> {
    ensure!(!term.contains('\0'), "synonym contains a NUL character");
    ensure!(
        term.len() <= MAX_SYNONYM_BYTES,
        "synonym exceeds {MAX_SYNONYM_BYTES} bytes"
    );
    let folded = fold_text(term.trim());
    let tokens = tokenize_folded(&folded);
    ensure!(
        tokens.len() == 1,
        "synonym must normalise to a single word token"
    );
    Ok(tokens.into_iter().next().unwrap_or_default())
}

pub(crate) fn is_valid_translation_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 32
        && code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn song_lyrics_text(song: &Song) -> String {
    let mut out = String::new();
    for section in &song.sections {
        for line in &section.lines {
            if !line.text.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&line.text);
            }
        }
        for lines in section.translations.values() {
            for line in &lines.lines {
                if !line.is_empty() {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(line);
                }
            }
        }
    }
    out
}

pub(crate) fn index_song_in_tx(tx: &Transaction<'_>, song: &Song) -> anyhow::Result<()> {
    let id = super::required_id(&song.id)?;
    let rowid: i64 = tx
        .prepare_cached("SELECT rowid FROM songs WHERE id=?1")?
        .query_row([id], |r| r.get(0))?;
    remove_song_rowid_in_tx(tx, rowid)?;

    let authors = song.authors.join("\n");
    let tags = song.tags.join("\n");
    let lyrics = song_lyrics_text(song);

    tx.prepare_cached(
        "INSERT INTO songs_fts(rowid, id, title, lyrics, authors, ccli_number, tags) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?
    .execute(params![
        rowid,
        id,
        song.title,
        lyrics,
        authors,
        song.ccli_number,
        tags
    ])?;

    tx.prepare_cached(
        "INSERT INTO songs_trigram(rowid, title, authors, ccli_number, tags, lyrics) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![
        rowid,
        fold_text(&song.title),
        fold_text(&authors),
        fold_text(&song.ccli_number),
        fold_text(&tags),
        fold_text(&lyrics),
    ])?;
    Ok(())
}

fn remove_song_rowid_in_tx(tx: &Transaction<'_>, rowid: i64) -> anyhow::Result<()> {
    tx.prepare_cached("DELETE FROM songs_fts WHERE rowid=?1")?
        .execute([rowid])?;
    tx.prepare_cached("DELETE FROM songs_trigram WHERE rowid=?1")?
        .execute([rowid])?;
    Ok(())
}

pub(crate) fn remove_song_from_index_in_tx(tx: &Transaction<'_>, id: &[u8]) -> anyhow::Result<()> {
    if let Some(rowid) = tx
        .prepare_cached("SELECT rowid FROM songs WHERE id=?1")?
        .query_row([id], |r| r.get::<_, i64>(0))
        .optional()?
    {
        remove_song_rowid_in_tx(tx, rowid)?;
    }
    Ok(())
}

pub(crate) fn index_media_in_tx(tx: &Transaction<'_>, item: &MediaItem) -> anyhow::Result<()> {
    let id = super::required_id(&item.id)?;
    let rowid: i64 = tx
        .prepare_cached("SELECT rowid FROM media_items WHERE id=?1")?
        .query_row([id], |r| r.get(0))?;
    remove_media_rowid_in_tx(tx, rowid)?;

    let tags = item.tags.join("\n");
    tx.prepare_cached("INSERT INTO media_fts(rowid, id, name, tags) VALUES (?1, ?2, ?3, ?4)")?
        .execute(params![rowid, id, item.name, tags])?;
    tx.prepare_cached("INSERT INTO media_trigram(rowid, name, tags) VALUES (?1, ?2, ?3)")?
        .execute(params![rowid, fold_text(&item.name), fold_text(&tags)])?;
    Ok(())
}

fn remove_media_rowid_in_tx(tx: &Transaction<'_>, rowid: i64) -> anyhow::Result<()> {
    tx.prepare_cached("DELETE FROM media_fts WHERE rowid=?1")?
        .execute([rowid])?;
    tx.prepare_cached("DELETE FROM media_trigram WHERE rowid=?1")?
        .execute([rowid])?;
    Ok(())
}

pub(crate) fn remove_media_from_index_in_tx(tx: &Transaction<'_>, id: &[u8]) -> anyhow::Result<()> {
    if let Some(rowid) = tx
        .prepare_cached("SELECT rowid FROM media_items WHERE id=?1")?
        .query_row([id], |r| r.get::<_, i64>(0))
        .optional()?
    {
        remove_media_rowid_in_tx(tx, rowid)?;
    }
    Ok(())
}

/// Ensures FTS5, trigram indexes and default synonyms are ready after migrations.
/// Historical fixtures (e.g. `schema-v1.db`) already hold encoded `songs` and `media_items`
/// blobs that SQL migrations alone cannot decode, so we backfill once on upgrade.
pub(crate) fn ensure_search_ready(db: &mut Connection) -> anyhow::Result<()> {
    let current: Option<String> = db
        .query_row(
            "SELECT value FROM search_meta WHERE key='index_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if current.as_deref() == Some(SEARCH_INDEX_VERSION) {
        return Ok(());
    }

    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let recheck: Option<String> = tx
        .query_row(
            "SELECT value FROM search_meta WHERE key='index_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if recheck.as_deref() == Some(SEARCH_INDEX_VERSION) {
        tx.commit()?;
        return Ok(());
    }

    tx.execute_batch(
        "DELETE FROM songs_fts;
         DELETE FROM songs_trigram;
         DELETE FROM media_fts;
         DELETE FROM media_trigram;",
    )?;

    let songs: Vec<(i64, Vec<u8>)> = {
        let mut stmt = tx.prepare("SELECT rowid, data FROM songs ORDER BY rowid")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (rowid, data) in songs {
        ensure!(
            data.len() <= super::MAX_DOCUMENT_BYTES,
            "stored song exceeds maximum size"
        );
        let song = Song::decode(data.as_slice())?;
        let authors = song.authors.join("\n");
        let tags = song.tags.join("\n");
        let lyrics = song_lyrics_text(&song);
        let id = super::required_id(&song.id)?;
        tx.prepare_cached(
            "INSERT INTO songs_fts(rowid, id, title, lyrics, authors, ccli_number, tags) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?
        .execute(params![
            rowid,
            id,
            song.title,
            lyrics,
            authors,
            song.ccli_number,
            tags
        ])?;
        tx.prepare_cached(
            "INSERT INTO songs_trigram(rowid, title, authors, ccli_number, tags, lyrics) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?
        .execute(params![
            rowid,
            fold_text(&song.title),
            fold_text(&authors),
            fold_text(&song.ccli_number),
            fold_text(&tags),
            fold_text(&lyrics),
        ])?;
    }

    let media_rows: Vec<(i64, Vec<u8>)> = {
        let mut stmt = tx.prepare("SELECT rowid, data FROM media_items ORDER BY rowid")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (rowid, data) in media_rows {
        ensure!(
            data.len() <= super::MAX_DOCUMENT_BYTES,
            "stored media item exceeds maximum size"
        );
        let item = MediaItem::decode(data.as_slice())?;
        let tags = item.tags.join("\n");
        let id = super::required_id(&item.id)?;
        tx.prepare_cached("INSERT INTO media_fts(rowid, id, name, tags) VALUES (?1, ?2, ?3, ?4)")?
            .execute(params![rowid, id, item.name, tags])?;
        tx.prepare_cached("INSERT INTO media_trigram(rowid, name, tags) VALUES (?1, ?2, ?3)")?
            .execute(params![rowid, fold_text(&item.name), fold_text(&tags)])?;
    }

    {
        let mut syn_insert =
            tx.prepare("INSERT OR IGNORE INTO search_synonyms (term, synonym) VALUES (?1, ?2)")?;
        for &(a, b) in DEFAULT_SYNONYMS {
            let (left, right) = if a <= b { (a, b) } else { (b, a) };
            syn_insert.execute(params![left, right])?;
        }
    }

    tx.execute(
        "INSERT INTO search_meta (key, value) VALUES ('index_version', ?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![SEARCH_INDEX_VERSION],
    )?;
    tx.commit()?;
    Ok(())
}

/// Optimal string alignment (restricted Damerau-Levenshtein) distance on Unicode scalar chars.
pub(crate) fn damerau_levenshtein(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let n = a.len();
    let m = b.len();
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let width = m + 1;
    let mut dp = vec![0usize; (n + 1) * width];
    for i in 0..=n {
        dp[i * width] = i;
    }
    for (j, cell) in dp.iter_mut().enumerate().take(m + 1) {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (dp[(i - 1) * width + j] + 1)
                .min(dp[i * width + (j - 1)] + 1)
                .min(dp[(i - 1) * width + (j - 1)] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(dp[(i - 2) * width + (j - 2)] + 1);
            }
            dp[i * width + j] = best;
        }
    }
    dp[n * width + m]
}

fn max_fuzzy_edits(token_chars: usize) -> usize {
    match token_chars {
        0..=2 => 0,
        3..=5 => 1,
        _ => 2,
    }
}

/// Similarity in `[0.0, 1.0]` between a folded query token (+ its synonyms) and tokens of a field.
fn field_token_similarity(token: &str, synonyms: &[String], field_tokens: &[String]) -> f64 {
    let token_len = token.chars().count();
    let max_edits = max_fuzzy_edits(token_len);
    let mut best = 0.0_f64;

    for word in field_tokens {
        if word == token {
            return 1.0;
        }
        if token_len >= 2 && word.starts_with(token) {
            best = best.max(0.90);
            continue;
        }
        if synonyms.iter().any(|syn| {
            let syn_len = syn.chars().count();
            word == syn || (syn_len >= 3 && word.starts_with(syn.as_str()))
        }) {
            best = best.max(0.78);
            continue;
        }
        if max_edits > 0 {
            let word_len = word.chars().count();
            if token_len.abs_diff(word_len) <= max_edits {
                let dist = damerau_levenshtein(token, word);
                if dist <= max_edits {
                    let denom = token_len.max(word_len) as f64;
                    let sim = (1.0 - dist as f64 / denom) * 0.45;
                    if sim >= 0.22 {
                        best = best.max(sim);
                    }
                }
            }
        }
    }
    best
}

fn score_token_match(sim: f64, field_weight: f64) -> f64 {
    if sim <= 0.0 {
        return 0.0;
    }
    let tier_bonus = if sim >= 0.99 {
        6.0
    } else if sim >= 0.89 {
        3.0
    } else if sim >= 0.77 {
        1.5
    } else {
        0.0
    };
    sim * field_weight + tier_bonus
}

fn fts_term(token: &str) -> String {
    let clean = token.replace('"', "");
    if clean.chars().count() >= 3 {
        format!("\"{clean}\"*")
    } else {
        format!("\"{clean}\"")
    }
}

fn trigram_token_clauses(tokens: &[String]) -> Vec<String> {
    let mut clauses = Vec::new();
    for token in tokens {
        let chars: Vec<char> = token.chars().filter(|&c| c != '"').collect();
        if chars.len() < 3 {
            continue;
        }
        let mut parts = Vec::new();
        let mut seen = HashSet::new();
        for window in chars.windows(3) {
            let clean: String = window.iter().collect();
            if seen.insert(clean.clone()) {
                parts.push(format!("\"{clean}\""));
            }
        }
        if !parts.is_empty() {
            clauses.push(format!("({})", parts.join(" OR ")));
        }
    }
    clauses
}

fn trigram_expr_for_tokens(tokens: &[String]) -> Option<String> {
    let mut parts = Vec::new();
    let mut seen = HashSet::new();
    for token in tokens {
        let chars: Vec<char> = token.chars().filter(|&c| c != '"').collect();
        if chars.len() < 3 {
            continue;
        }
        for window in chars.windows(3) {
            let clean: String = window.iter().collect();
            if seen.insert(clean.clone()) {
                parts.push(format!("\"{clean}\""));
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" OR "))
    }
}

fn truncate_snippet(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    for (count, (idx, _)) in trimmed.char_indices().enumerate() {
        if count == max_chars {
            let end = trimmed[..idx]
                .rfind(char::is_whitespace)
                .filter(|&ws| ws > idx / 2)
                .unwrap_or(idx);
            let mut out = trimmed[..end].trim_end().to_owned();
            out.push_str("...");
            return out;
        }
    }
    trimmed.to_owned()
}

fn extract_song_snippet(song: &Song, tokens: &[String], syn_groups: &[Vec<String>]) -> String {
    let mut terms: Vec<&str> = tokens.iter().map(String::as_str).collect();
    for syns in syn_groups {
        for syn in syns {
            terms.push(syn.as_str());
        }
    }
    let matches_terms = |text: &str| -> bool {
        let folded = fold_text(text);
        let words = tokenize_folded(&folded);
        terms.iter().any(|t| {
            let t_chars = t.chars().count();
            words.iter().any(|w| {
                w == t
                    || (t_chars >= 3 && w.starts_with(t))
                    || (t_chars >= 4
                        && t_chars.abs_diff(w.chars().count()) <= 1
                        && damerau_levenshtein(t, w) <= 1)
            })
        })
    };

    for section in &song.sections {
        for line in &section.lines {
            if !line.text.trim().is_empty() && matches_terms(&line.text) {
                return truncate_snippet(&line.text, 160);
            }
        }
        for trans in section.translations.values() {
            for line in &trans.lines {
                if !line.trim().is_empty() && matches_terms(line) {
                    return truncate_snippet(line, 160);
                }
            }
        }
    }
    for author in &song.authors {
        if matches_terms(author) {
            return truncate_snippet(&song.authors.join(", "), 160);
        }
    }
    if !song.ccli_number.is_empty() && matches_terms(&song.ccli_number) {
        return format!("CCLI {}", song.ccli_number);
    }
    for tag in &song.tags {
        if matches_terms(tag) {
            return truncate_snippet(&song.tags.join(", "), 160);
        }
    }
    for section in &song.sections {
        for line in &section.lines {
            if !line.text.trim().is_empty() {
                return truncate_snippet(&line.text, 160);
            }
        }
    }
    song.title.clone()
}

fn confirm_song_lyrics_match(song: &Song, tokens: &[String], syn_groups: &[Vec<String>]) -> bool {
    let mut all_lyric_words = Vec::new();
    for section in &song.sections {
        for line in &section.lines {
            all_lyric_words.extend(tokenize_folded(&fold_text(&line.text)));
        }
        for trans in section.translations.values() {
            for line in &trans.lines {
                all_lyric_words.extend(tokenize_folded(&fold_text(line)));
            }
        }
    }
    if all_lyric_words.is_empty() {
        return false;
    }
    tokens
        .iter()
        .enumerate()
        .any(|(idx, tok)| field_token_similarity(tok, &syn_groups[idx], &all_lyric_words) > 0.0)
}

#[derive(Clone, Debug)]
struct SongCandidate {
    rowid: i64,
    title: String,
    authors: String,
    ccli_number: String,
    tags: String,
    strict_fts: bool,
    fts_bm25: Option<f64>,
    trigram_bm25: Option<f64>,
}

struct RankedSongRow {
    rowid: i64,
    score: f64,
    title_hit: bool,
    author_hit: bool,
    ccli_hit: bool,
    tag_hit: bool,
}

impl Library {
    pub fn add_synonym(&mut self, term: &str, synonym: &str) -> anyhow::Result<()> {
        let a = validate_synonym_term(term)?;
        let b = validate_synonym_term(synonym)?;
        ensure!(a != b, "synonym pair must be two different terms");
        let (left, right) = if a <= b { (a, b) } else { (b, a) };
        self.db.execute(
            "INSERT OR IGNORE INTO search_synonyms (term, synonym) VALUES (?1, ?2)",
            params![left, right],
        )?;
        Ok(())
    }

    pub fn synonyms_for(&self, term: &str) -> anyhow::Result<Vec<String>> {
        let normalized = validate_synonym_term(term)?;
        self.lookup_synonyms(&normalized)
    }

    fn lookup_synonyms(&self, normalized_token: &str) -> anyhow::Result<Vec<String>> {
        let mut stmt = self.db.prepare_cached(
            "SELECT synonym FROM search_synonyms WHERE term=?1
             UNION
             SELECT term FROM search_synonyms WHERE synonym=?1
             ORDER BY 1",
        )?;
        let rows = stmt
            .query_map([normalized_token], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn search_songs(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<SongSearchResults, CommandBusError> {
        let trimmed = validate_query(query, limit)?;
        if trimmed.is_empty() {
            return Ok(SongSearchResults::default());
        }
        let folded_query = fold_text(&trimmed);
        let tokens = tokenize_query(&folded_query);
        if tokens.is_empty() {
            return Ok(SongSearchResults::default());
        }

        let mut syn_groups = Vec::with_capacity(tokens.len());
        let mut fts_clauses = Vec::with_capacity(tokens.len());
        for token in &tokens {
            let syns = self.lookup_synonyms(token)?;
            let mut terms = Vec::with_capacity(1 + syns.len());
            terms.push(fts_term(token));
            for syn in &syns {
                terms.push(fts_term(syn));
            }
            fts_clauses.push(format!("({})", terms.join(" OR ")));
            syn_groups.push(syns);
        }

        let needed = (offset as usize).saturating_add(limit as usize);
        let gather_limit = (needed * 4).clamp(64, 400) as i64;
        let strict_match = fts_clauses.join(" AND ");

        let mut candidates: BTreeMap<i64, SongCandidate> = BTreeMap::new();
        {
            let mut stmt = self.db.prepare_cached(
                "SELECT s.rowid, s.title, s.authors, s.ccli_number, s.tags,
                        bm25(songs_fts, 0.0, 10.0, 1.0, 4.0, 8.0, 5.0) AS rank
                 FROM songs_fts
                 JOIN songs s ON s.rowid = songs_fts.rowid
                 WHERE songs_fts MATCH ?1
                 ORDER BY rank ASC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![strict_match, gather_limit], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, f64>(5)?,
                ))
            })?;
            for row in rows {
                let (rowid, title, authors, ccli_number, tags, bm25) = row?;
                candidates.insert(
                    rowid,
                    SongCandidate {
                        rowid,
                        title,
                        authors,
                        ccli_number,
                        tags,
                        strict_fts: true,
                        fts_bm25: Some(bm25),
                        trigram_bm25: None,
                    },
                );
            }
        }

        let strict_count = candidates.len();
        if strict_count < needed.max(10) {
            if tokens.len() > 1 {
                let partial_match = fts_clauses.join(" OR ");
                let mut stmt = self.db.prepare_cached(
                    "SELECT s.rowid, s.title, s.authors, s.ccli_number, s.tags,
                            bm25(songs_fts, 0.0, 10.0, 1.0, 4.0, 8.0, 5.0) AS rank
                     FROM songs_fts
                     JOIN songs s ON s.rowid = songs_fts.rowid
                     WHERE songs_fts MATCH ?1
                     ORDER BY rank ASC
                     LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![partial_match, gather_limit], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, f64>(5)?,
                    ))
                })?;
                for row in rows {
                    let (rowid, title, authors, ccli_number, tags, bm25) = row?;
                    candidates
                        .entry(rowid)
                        .and_modify(|existing| {
                            existing.fts_bm25 = Some(match existing.fts_bm25 {
                                Some(prev) => prev.min(bm25),
                                None => bm25,
                            });
                        })
                        .or_insert(SongCandidate {
                            rowid,
                            title,
                            authors,
                            ccli_number,
                            tags,
                            strict_fts: false,
                            fts_bm25: Some(bm25),
                            trigram_bm25: None,
                        });
                }
            }

            let trigram_clauses = trigram_token_clauses(&tokens);
            let mut has_full_meta = has_full_metadata_match(&candidates, &tokens, &syn_groups);
            if !trigram_clauses.is_empty() {
                let mut meta_hits = 0usize;
                let run_meta_trigram = |candidates: &mut BTreeMap<i64, SongCandidate>,
                                        expr: &str|
                 -> anyhow::Result<usize> {
                    let meta_trigram = format!("{{title authors ccli_number tags}} : ({expr})");
                    let mut count = 0usize;
                    let mut stmt = self.db.prepare_cached(
                        "SELECT s.rowid, s.title, s.authors, s.ccli_number, s.tags,
                                    bm25(songs_trigram, 10.0, 4.0, 8.0, 5.0, 1.0) AS rank
                             FROM songs_trigram
                             JOIN songs s ON s.rowid = songs_trigram.rowid
                             WHERE songs_trigram MATCH ?1
                             ORDER BY rank ASC
                             LIMIT 200",
                    )?;
                    let rows = stmt.query_map([&meta_trigram], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, f64>(5)?,
                        ))
                    })?;
                    for row in rows {
                        count += 1;
                        let (rowid, title, authors, ccli_number, tags, t_bm25) = row?;
                        candidates
                            .entry(rowid)
                            .and_modify(|existing| {
                                existing.trigram_bm25 = Some(match existing.trigram_bm25 {
                                    Some(prev) => prev.min(t_bm25),
                                    None => t_bm25,
                                });
                            })
                            .or_insert(SongCandidate {
                                rowid,
                                title,
                                authors,
                                ccli_number,
                                tags,
                                strict_fts: false,
                                fts_bm25: None,
                                trigram_bm25: Some(t_bm25),
                            });
                    }
                    Ok(count)
                };

                if trigram_clauses.len() >= 2 {
                    let and_expr = trigram_clauses.join(" AND ");
                    meta_hits += run_meta_trigram(&mut candidates, &and_expr)?;
                    has_full_meta = has_full_metadata_match(&candidates, &tokens, &syn_groups);
                }

                if !has_full_meta
                    && candidates.len() < needed.max(10)
                    && let Some(or_expr) = trigram_expr_for_tokens(&tokens)
                {
                    meta_hits += run_meta_trigram(&mut candidates, &or_expr)?;
                    has_full_meta = has_full_metadata_match(&candidates, &tokens, &syn_groups);
                }

                // Query lyric trigrams only when strict hits didn't satisfy the query and
                // metadata/partial FTS did not already yield a full-coverage candidate.
                let need_lyric_trigram = strict_count == 0
                    && !has_full_meta
                    && (candidates.len() < needed.max(10) || meta_hits == 0 || tokens.len() > 1);
                if need_lyric_trigram {
                    let lyric_expr = if trigram_clauses.len() >= 2 {
                        trigram_clauses.join(" AND ")
                    } else {
                        trigram_expr_for_tokens(&tokens).unwrap_or_default()
                    };
                    let mut stmt = self.db.prepare_cached(
                        "SELECT s.rowid, s.title, s.authors, s.ccli_number, s.tags,
                                bm25(songs_trigram, 10.0, 4.0, 8.0, 5.0, 1.0) AS rank
                         FROM songs_trigram
                         JOIN songs s ON s.rowid = songs_trigram.rowid
                         WHERE songs_trigram MATCH ?1
                         ORDER BY rank ASC
                         LIMIT 100",
                    )?;
                    let rows = stmt.query_map([&lyric_expr], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, f64>(5)?,
                        ))
                    })?;
                    for row in rows {
                        let (rowid, title, authors, ccli_number, tags, t_bm25) = row?;
                        candidates
                            .entry(rowid)
                            .and_modify(|existing| {
                                existing.trigram_bm25 = Some(match existing.trigram_bm25 {
                                    Some(prev) => prev.min(t_bm25),
                                    None => t_bm25,
                                });
                            })
                            .or_insert(SongCandidate {
                                rowid,
                                title,
                                authors,
                                ccli_number,
                                tags,
                                strict_fts: false,
                                fts_bm25: None,
                                trigram_bm25: Some(t_bm25),
                            });
                    }
                }
            }
        }

        let has_full_meta = has_full_metadata_match(&candidates, &tokens, &syn_groups);
        let mut lyric_stmt = self
            .db
            .prepare_cached("SELECT lyrics FROM songs_trigram WHERE rowid=?1")?;
        let mut ranked = Vec::with_capacity(candidates.len());
        for candidate in candidates.into_values() {
            let check_lyrics = candidate.strict_fts || !has_full_meta;
            if let Some(scored) = score_song_candidate(
                &candidate,
                &folded_query,
                &tokens,
                &syn_groups,
                check_lyrics.then_some(&mut lyric_stmt),
            )? {
                ranked.push(scored);
            }
        }

        // Last-resort typo fallback for short transposed words sharing zero trigrams (e.g. "grcae").
        if ranked.is_empty() && tokens.iter().any(|t| (3..=8).contains(&t.chars().count())) {
            let mut stmt = self.db.prepare_cached(
                "SELECT rowid, title, authors, ccli_number, tags FROM songs ORDER BY rowid",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?;
            for row in rows {
                let (rowid, title, authors, ccli_number, tags) = row?;
                let candidate = SongCandidate {
                    rowid,
                    title,
                    authors,
                    ccli_number,
                    tags,
                    strict_fts: false,
                    fts_bm25: None,
                    trigram_bm25: None,
                };
                if let Some(scored) =
                    score_song_candidate(&candidate, &folded_query, &tokens, &syn_groups, None)?
                {
                    ranked.push(scored);
                }
            }
        }

        ranked.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.rowid.cmp(&b.rowid))
        });

        let total = u32::try_from(ranked.len()).unwrap_or(u32::MAX);
        let page = ranked
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize);

        let mut data_stmt = self
            .db
            .prepare_cached("SELECT data FROM songs WHERE rowid=?1")?;
        let mut hits = Vec::new();
        for item in page {
            let data: Vec<u8> = data_stmt.query_row([item.rowid], |r| r.get(0))?;
            if data.len() > super::MAX_DOCUMENT_BYTES {
                return Err(CommandBusError::Internal(anyhow::anyhow!(
                    "stored song exceeds maximum size"
                )));
            }
            let song =
                Song::decode(data.as_slice()).map_err(|e| CommandBusError::Internal(e.into()))?;
            let lyrics_hit = confirm_song_lyrics_match(&song, &tokens, &syn_groups);
            let mut matched_fields = Vec::with_capacity(5);
            if item.title_hit {
                matched_fields.push("title".into());
            }
            if lyrics_hit {
                matched_fields.push("lyrics".into());
            }
            if item.author_hit {
                matched_fields.push("authors".into());
            }
            if item.ccli_hit {
                matched_fields.push("ccli_number".into());
            }
            if item.tag_hit {
                matched_fields.push("tags".into());
            }
            let snippet = extract_song_snippet(&song, &tokens, &syn_groups);
            hits.push(SongSearchHit {
                song: Some(song),
                score: item.score,
                snippet,
                matched_fields,
            });
        }

        Ok(SongSearchResults { hits, total })
    }

    pub fn search_media(
        &self,
        query: &str,
        kind: MediaKind,
        limit: u32,
        offset: u32,
    ) -> Result<MediaSearchResults, CommandBusError> {
        let trimmed = validate_query(query, limit)?;
        if trimmed.is_empty() {
            return Ok(MediaSearchResults::default());
        }
        let folded_query = fold_text(&trimmed);
        let tokens = tokenize_query(&folded_query);
        if tokens.is_empty() {
            return Ok(MediaSearchResults::default());
        }

        let mut syn_groups = Vec::with_capacity(tokens.len());
        let mut fts_clauses = Vec::with_capacity(tokens.len());
        for token in &tokens {
            let syns = self.lookup_synonyms(token)?;
            let mut terms = Vec::with_capacity(1 + syns.len());
            terms.push(fts_term(token));
            for syn in &syns {
                terms.push(fts_term(syn));
            }
            fts_clauses.push(format!("({})", terms.join(" OR ")));
            syn_groups.push(syns);
        }

        let needed = (offset as usize).saturating_add(limit as usize);
        let gather_limit = (needed * 6).clamp(80, 400) as i64;
        let strict_match = fts_clauses.join(" AND ");

        #[derive(Clone)]
        struct MediaCandidate {
            rowid: i64,
            name: String,
            tags: String,
            data: Vec<u8>,
            strict_fts: bool,
            fts_bm25: Option<f64>,
            trigram_bm25: Option<f64>,
        }

        let mut candidates: BTreeMap<i64, MediaCandidate> = BTreeMap::new();
        {
            let mut stmt = self.db.prepare_cached(
                "SELECT m.rowid, m.name,
                        COALESCE((SELECT group_concat(mt.tag, char(10)) FROM media_tags mt WHERE mt.media_id = m.id), ''),
                        m.data, bm25(media_fts, 0.0, 10.0, 5.0) AS rank
                 FROM media_fts
                 JOIN media_items m ON m.rowid = media_fts.rowid
                 WHERE media_fts MATCH ?1
                 ORDER BY rank ASC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![strict_match, gather_limit], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                    r.get::<_, f64>(4)?,
                ))
            })?;
            for row in rows {
                let (rowid, name, tags, data, bm25) = row?;
                candidates.insert(
                    rowid,
                    MediaCandidate {
                        rowid,
                        name,
                        tags,
                        data,
                        strict_fts: true,
                        fts_bm25: Some(bm25),
                        trigram_bm25: None,
                    },
                );
            }
        }

        if candidates.len() < needed.max(10) {
            if tokens.len() > 1 {
                let partial_match = fts_clauses.join(" OR ");
                let mut stmt = self.db.prepare_cached(
                    "SELECT m.rowid, m.name,
                            COALESCE((SELECT group_concat(mt.tag, char(10)) FROM media_tags mt WHERE mt.media_id = m.id), ''),
                            m.data, bm25(media_fts, 0.0, 10.0, 5.0) AS rank
                     FROM media_fts
                     JOIN media_items m ON m.rowid = media_fts.rowid
                     WHERE media_fts MATCH ?1
                     ORDER BY rank ASC
                     LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![partial_match, gather_limit], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, f64>(4)?,
                    ))
                })?;
                for row in rows {
                    let (rowid, name, tags, data, bm25) = row?;
                    candidates
                        .entry(rowid)
                        .and_modify(|existing| {
                            existing.fts_bm25 = Some(match existing.fts_bm25 {
                                Some(prev) => prev.min(bm25),
                                None => bm25,
                            });
                        })
                        .or_insert(MediaCandidate {
                            rowid,
                            name,
                            tags,
                            data,
                            strict_fts: false,
                            fts_bm25: Some(bm25),
                            trigram_bm25: None,
                        });
                }
            }

            if let Some(gram_expr) = trigram_expr_for_tokens(&tokens) {
                let mut stmt = self.db.prepare_cached(
                    "SELECT m.rowid, m.name,
                            COALESCE((SELECT group_concat(mt.tag, char(10)) FROM media_tags mt WHERE mt.media_id = m.id), ''),
                            m.data, bm25(media_trigram, 10.0, 5.0) AS rank
                     FROM media_trigram
                     JOIN media_items m ON m.rowid = media_trigram.rowid
                     WHERE media_trigram MATCH ?1
                     ORDER BY rank ASC
                     LIMIT 200",
                )?;
                let rows = stmt.query_map([&gram_expr], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, f64>(4)?,
                    ))
                })?;
                for row in rows {
                    let (rowid, name, tags, data, t_bm25) = row?;
                    candidates
                        .entry(rowid)
                        .and_modify(|existing| {
                            existing.trigram_bm25 = Some(match existing.trigram_bm25 {
                                Some(prev) => prev.min(t_bm25),
                                None => t_bm25,
                            });
                        })
                        .or_insert(MediaCandidate {
                            rowid,
                            name,
                            tags,
                            data,
                            strict_fts: false,
                            fts_bm25: None,
                            trigram_bm25: Some(t_bm25),
                        });
                }
            }
        }

        if candidates.is_empty() && tokens.iter().any(|t| t.chars().count() >= 3) {
            let mut stmt = self.db.prepare_cached(
                "SELECT m.rowid, m.name,
                        COALESCE((SELECT group_concat(mt.tag, char(10)) FROM media_tags mt WHERE mt.media_id = m.id), ''),
                        m.data
                 FROM media_items m
                 ORDER BY m.rowid",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Vec<u8>>(3)?,
                ))
            })?;
            for row in rows {
                let (rowid, name, tags, data) = row?;
                candidates.insert(
                    rowid,
                    MediaCandidate {
                        rowid,
                        name,
                        tags,
                        data,
                        strict_fts: false,
                        fts_bm25: None,
                        trigram_bm25: None,
                    },
                );
            }
        }

        let mut scored_hits: Vec<(i64, MediaSearchHit)> = Vec::new();
        for candidate in candidates.into_values() {
            if candidate.data.len() > super::MAX_DOCUMENT_BYTES {
                return Err(CommandBusError::Internal(anyhow::anyhow!(
                    "stored media item exceeds maximum size"
                )));
            }
            let item = MediaItem::decode(candidate.data.as_slice())
                .map_err(|e| CommandBusError::Internal(e.into()))?;
            if kind != MediaKind::Unspecified && item.kind != kind as i32 {
                continue;
            }

            let name_folded = fold_text(&candidate.name);
            let name_tokens = tokenize_folded(&name_folded);
            let tag_tokens = tokenize_folded(&fold_text(&candidate.tags));

            let mut matched_tokens = 0usize;
            let mut token_sum = 0.0_f64;
            let mut name_hit = false;
            let mut tag_hit = false;

            for (idx, token) in tokens.iter().enumerate() {
                let name_sim = field_token_similarity(token, &syn_groups[idx], &name_tokens);
                let tag_sim = field_token_similarity(token, &syn_groups[idx], &tag_tokens);
                if name_sim > 0.0 {
                    name_hit = true;
                }
                if tag_sim > 0.0 {
                    tag_hit = true;
                }
                let best = score_token_match(name_sim, 10.0).max(score_token_match(tag_sim, 5.0));
                if best > 0.0 {
                    matched_tokens += 1;
                    token_sum += best;
                }
            }

            if candidate.strict_fts {
                matched_tokens = matched_tokens.max(tokens.len());
            }
            if matched_tokens == 0 && !name_hit && !tag_hit {
                continue;
            }

            let coverage = matched_tokens as f64 / tokens.len() as f64;
            let phrase_bonus = if name_folded == folded_query {
                40.0
            } else if name_folded.starts_with(&folded_query) {
                20.0
            } else if name_folded.contains(&folded_query) {
                10.0
            } else {
                0.0
            };
            let bm25_bonus = candidate
                .fts_bm25
                .map(|b| (-b).clamp(0.0, 20.0) * 0.15)
                .or_else(|| candidate.trigram_bm25.map(|b| (-b).clamp(0.0, 10.0) * 0.05))
                .unwrap_or(0.0);
            let score = token_sum + coverage * 25.0 + phrase_bonus + bm25_bonus;

            let mut matched_fields = Vec::new();
            if name_hit {
                matched_fields.push("name".into());
            }
            if tag_hit {
                matched_fields.push("tags".into());
            }
            let snippet = if tag_hit && !name_hit && !item.tags.is_empty() {
                truncate_snippet(&item.tags.join(", "), 160)
            } else {
                truncate_snippet(&item.name, 160)
            };

            scored_hits.push((
                candidate.rowid,
                MediaSearchHit {
                    media_item: Some(item),
                    score,
                    snippet,
                    matched_fields,
                },
            ));
        }

        scored_hits.sort_by(|a, b| b.1.score.total_cmp(&a.1.score).then_with(|| a.0.cmp(&b.0)));

        let total = u32::try_from(scored_hits.len()).unwrap_or(u32::MAX);
        let hits = scored_hits
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|(_, hit)| hit)
            .collect();

        Ok(MediaSearchResults { hits, total })
    }

    pub fn search_scripture(
        &self,
        query: &str,
        translation: &str,
        limit: u32,
        offset: u32,
    ) -> Result<ScriptureSearchResults, CommandBusError> {
        let trimmed = validate_query(query, limit)?;
        let requested_translation = translation.trim();
        if !requested_translation.is_empty() && !is_valid_translation_code(requested_translation) {
            return Err(CommandBusError::Invalid(
                "invalid Bible translation code".into(),
            ));
        }
        if trimmed.is_empty() {
            return Ok(ScriptureSearchResults::default());
        }

        let bibles = self
            .list_validated_bible_files()
            .map_err(CommandBusError::Internal)?;
        let selected: Vec<(String, PathBuf)> = if requested_translation.is_empty() {
            bibles
        } else {
            let matching: Vec<_> = bibles
                .into_iter()
                .filter(|(code, _)| code.eq_ignore_ascii_case(requested_translation))
                .collect();
            if matching.is_empty() {
                return Err(CommandBusError::NotFound(
                    "Bible translation not found".into(),
                ));
            }
            matching
        };
        if selected.is_empty() {
            return Ok(ScriptureSearchResults::default());
        }

        let folded_query = fold_text(&trimmed);
        let tokens = tokenize_query(&folded_query);
        if tokens.is_empty() {
            return Ok(ScriptureSearchResults::default());
        }

        let mut syn_groups = Vec::with_capacity(tokens.len());
        let mut fts_clauses = Vec::with_capacity(tokens.len());
        for token in &tokens {
            let syns = self.lookup_synonyms(token)?;
            let mut terms = Vec::with_capacity(1 + syns.len());
            terms.push(fts_term(token));
            for syn in &syns {
                terms.push(fts_term(syn));
            }
            fts_clauses.push(format!("({})", terms.join(" OR ")));
            syn_groups.push(syns);
        }

        let strict_match = fts_clauses.join(" AND ");
        let partial_match = fts_clauses.join(" OR ");
        let gram_expr = trigram_expr_for_tokens(&tokens);
        let needed = (offset as usize).saturating_add(limit as usize);
        let gather_limit = (needed * 4).clamp(64, 400) as i64;

        let mut all_hits: Vec<ScriptureVerseHit> = Vec::new();
        for (code, path) in selected {
            let bible_db = open_bible_db(&path).map_err(CommandBusError::Internal)?;
            #[derive(Clone)]
            struct VerseCandidate {
                book_code: String,
                book_name: String,
                chapter: u32,
                verse: u32,
                text: String,
                strict_fts: bool,
                bm25: Option<f64>,
            }

            let mut candidates: BTreeMap<i64, VerseCandidate> = BTreeMap::new();
            {
                let mut stmt = bible_db.prepare_cached(
                    "SELECT v.id, v.book_code, v.book_name, v.chapter, v.verse, v.text, bm25(verses_fts) AS rank
                     FROM verses_fts
                     JOIN verses v ON v.id = verses_fts.rowid
                     WHERE verses_fts MATCH ?1
                     ORDER BY rank ASC
                     LIMIT ?2",
                )?;
                let rows = stmt.query_map(params![&strict_match, gather_limit], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, u32>(3)?,
                        r.get::<_, u32>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, f64>(6)?,
                    ))
                })?;
                for row in rows {
                    let (id, book_code, book_name, chapter, verse, text, rank) = row?;
                    candidates.insert(
                        id,
                        VerseCandidate {
                            book_code,
                            book_name,
                            chapter,
                            verse,
                            text,
                            strict_fts: true,
                            bm25: Some(rank),
                        },
                    );
                }
            }

            if candidates.len() < needed.max(10) {
                if tokens.len() > 1 {
                    let mut stmt = bible_db.prepare_cached(
                        "SELECT v.id, v.book_code, v.book_name, v.chapter, v.verse, v.text, bm25(verses_fts) AS rank
                         FROM verses_fts
                         JOIN verses v ON v.id = verses_fts.rowid
                         WHERE verses_fts MATCH ?1
                         ORDER BY rank ASC
                         LIMIT ?2",
                    )?;
                    let rows = stmt.query_map(params![&partial_match, gather_limit], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, u32>(3)?,
                            r.get::<_, u32>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, f64>(6)?,
                        ))
                    })?;
                    for row in rows {
                        let (id, book_code, book_name, chapter, verse, text, rank) = row?;
                        candidates.entry(id).or_insert(VerseCandidate {
                            book_code,
                            book_name,
                            chapter,
                            verse,
                            text,
                            strict_fts: false,
                            bm25: Some(rank),
                        });
                    }
                }

                if let Some(ref grams) = gram_expr {
                    let mut stmt = bible_db.prepare_cached(
                        "SELECT v.id, v.book_code, v.book_name, v.chapter, v.verse, v.text, bm25(verses_trigram) AS rank
                         FROM verses_trigram
                         JOIN verses v ON v.id = verses_trigram.rowid
                         WHERE verses_trigram MATCH ?1
                         ORDER BY rank ASC
                         LIMIT 200",
                    )?;
                    let rows = stmt.query_map([grams], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, u32>(3)?,
                            r.get::<_, u32>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, f64>(6)?,
                        ))
                    })?;
                    for row in rows {
                        let (id, book_code, book_name, chapter, verse, text, rank) = row?;
                        candidates.entry(id).or_insert(VerseCandidate {
                            book_code,
                            book_name,
                            chapter,
                            verse,
                            text,
                            strict_fts: false,
                            bm25: Some(rank),
                        });
                    }
                }
            }

            if candidates.is_empty() && tokens.iter().any(|t| t.chars().count() >= 3) {
                let mut stmt = bible_db.prepare_cached(
                    "SELECT id, book_code, book_name, chapter, verse, text FROM verses ORDER BY id",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, u32>(3)?,
                        r.get::<_, u32>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                })?;
                for row in rows {
                    let (id, book_code, book_name, chapter, verse, text) = row?;
                    candidates.insert(
                        id,
                        VerseCandidate {
                            book_code,
                            book_name,
                            chapter,
                            verse,
                            text,
                            strict_fts: false,
                            bm25: None,
                        },
                    );
                }
            }

            for candidate in candidates.into_values() {
                let text_folded = fold_text(&candidate.text);
                let text_tokens = tokenize_folded(&text_folded);
                let mut matched = 0usize;
                let mut token_sum = 0.0_f64;
                for (idx, tok) in tokens.iter().enumerate() {
                    let sim = field_token_similarity(tok, &syn_groups[idx], &text_tokens);
                    if sim > 0.0 {
                        matched += 1;
                        token_sum += score_token_match(sim, 10.0);
                    }
                }
                if candidate.strict_fts {
                    matched = matched.max(tokens.len());
                }
                if matched == 0 {
                    continue;
                }
                let coverage = matched as f64 / tokens.len() as f64;
                let phrase_bonus = if text_folded.contains(&folded_query) {
                    15.0
                } else {
                    0.0
                };
                let bm25_bonus = candidate
                    .bm25
                    .map(|b| (-b).clamp(0.0, 20.0) * 0.15)
                    .unwrap_or(0.0);
                let score = token_sum + coverage * 25.0 + phrase_bonus + bm25_bonus;
                all_hits.push(ScriptureVerseHit {
                    translation: code.clone(),
                    book: candidate.book_code,
                    book_name: candidate.book_name,
                    chapter: candidate.chapter,
                    verse: candidate.verse,
                    text: candidate.text,
                    score,
                });
            }
        }

        all_hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.translation.cmp(&b.translation))
                .then_with(|| a.book.cmp(&b.book))
                .then_with(|| a.chapter.cmp(&b.chapter))
                .then_with(|| a.verse.cmp(&b.verse))
        });

        let total = u32::try_from(all_hits.len()).unwrap_or(u32::MAX);
        let hits = all_hits
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect();
        Ok(ScriptureSearchResults { hits, total })
    }

    pub fn search_library(
        &self,
        query: &str,
        scope: SearchScope,
        translation: &str,
        limit: u32,
    ) -> Result<LibrarySearchResults, CommandBusError> {
        let trimmed = validate_query(query, limit)?;
        let requested_translation = translation.trim();
        if !requested_translation.is_empty() && !is_valid_translation_code(requested_translation) {
            return Err(CommandBusError::Invalid(
                "invalid Bible translation code".into(),
            ));
        }
        if trimmed.is_empty() {
            return Ok(LibrarySearchResults::default());
        }
        let scope = match scope {
            SearchScope::Unspecified => SearchScope::All,
            other => other,
        };
        let songs = if matches!(scope, SearchScope::All | SearchScope::Songs) {
            self.search_songs(&trimmed, limit, 0)?.hits
        } else {
            Vec::new()
        };
        let media = if matches!(scope, SearchScope::All | SearchScope::Media) {
            self.search_media(&trimmed, MediaKind::Unspecified, limit, 0)?
                .hits
        } else {
            Vec::new()
        };
        let scripture = if matches!(scope, SearchScope::All | SearchScope::Scripture) {
            self.search_scripture(&trimmed, requested_translation, limit, 0)?
                .hits
        } else {
            Vec::new()
        };
        Ok(LibrarySearchResults {
            songs,
            media,
            scripture,
        })
    }

    pub(crate) fn list_validated_bible_files(&self) -> anyhow::Result<Vec<(String, PathBuf)>> {
        let bibles_root = self.paths.bibles.canonicalize()?;
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&bibles_root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("db") {
                continue;
            }
            let meta = std::fs::symlink_metadata(&path)?;
            ensure!(
                !meta.file_type().is_symlink() && meta.is_file(),
                "Bible database must be a regular file inside bibles/"
            );
            let canonical = path.canonicalize()?;
            ensure!(
                canonical.starts_with(&bibles_root),
                "Bible database escaped library/bibles"
            );
            let db = open_bible_db(&canonical)?;
            let code: String =
                db.query_row("SELECT value FROM bible_meta WHERE key='code'", [], |r| {
                    r.get(0)
                })?;
            ensure!(
                is_valid_translation_code(&code),
                "invalid stored Bible code"
            );
            out.push((code, canonical));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }
}

fn has_full_metadata_match(
    candidates: &BTreeMap<i64, SongCandidate>,
    tokens: &[String],
    syn_groups: &[Vec<String>],
) -> bool {
    candidates.values().any(|candidate| {
        let title_tokens = tokenize_folded(&fold_text(&candidate.title));
        let author_tokens = tokenize_folded(&fold_text(&candidate.authors));
        let ccli_tokens = tokenize_folded(&fold_text(&candidate.ccli_number));
        let tag_tokens = tokenize_folded(&fold_text(&candidate.tags));
        tokens.iter().enumerate().all(|(idx, token)| {
            field_token_similarity(token, &syn_groups[idx], &title_tokens) > 0.0
                || field_token_similarity(token, &syn_groups[idx], &ccli_tokens) > 0.0
                || field_token_similarity(token, &syn_groups[idx], &tag_tokens) > 0.0
                || field_token_similarity(token, &syn_groups[idx], &author_tokens) > 0.0
        })
    })
}

fn score_song_candidate(
    candidate: &SongCandidate,
    folded_query: &str,
    tokens: &[String],
    syn_groups: &[Vec<String>],
    lyric_stmt: Option<&mut rusqlite::CachedStatement<'_>>,
) -> anyhow::Result<Option<RankedSongRow>> {
    let title_folded = fold_text(&candidate.title);
    let title_tokens = tokenize_folded(&title_folded);
    let author_tokens = tokenize_folded(&fold_text(&candidate.authors));
    let ccli_tokens = tokenize_folded(&fold_text(&candidate.ccli_number));
    let tag_tokens = tokenize_folded(&fold_text(&candidate.tags));

    let mut matched_tokens = 0usize;
    let mut token_score_sum = 0.0_f64;
    let mut title_hit = false;
    let mut author_hit = false;
    let mut ccli_hit = false;
    let mut tag_hit = false;
    let mut per_token_meta = Vec::with_capacity(tokens.len());

    for (idx, token) in tokens.iter().enumerate() {
        let t_sim = field_token_similarity(token, &syn_groups[idx], &title_tokens);
        let c_sim = field_token_similarity(token, &syn_groups[idx], &ccli_tokens);
        let g_sim = field_token_similarity(token, &syn_groups[idx], &tag_tokens);
        let a_sim = field_token_similarity(token, &syn_groups[idx], &author_tokens);

        if t_sim > 0.0 {
            title_hit = true;
        }
        if c_sim > 0.0 {
            ccli_hit = true;
        }
        if g_sim > 0.0 {
            tag_hit = true;
        }
        if a_sim > 0.0 {
            author_hit = true;
        }

        let best_meta = score_token_match(t_sim, 10.0)
            .max(score_token_match(c_sim, 8.0))
            .max(score_token_match(g_sim, 5.0))
            .max(score_token_match(a_sim, 4.0));
        if best_meta > 0.0 {
            matched_tokens += 1;
            token_score_sum += best_meta;
        }
        per_token_meta.push(best_meta);
    }

    let mut lyric_phrase_bonus = 0.0_f64;
    if matched_tokens < tokens.len()
        && let Some(stmt) = lyric_stmt
        && let Some(lyrics_folded) = stmt
            .query_row([candidate.rowid], |r| r.get::<_, String>(0))
            .optional()?
            .filter(|s| !s.is_empty())
    {
        if tokens.len() > 1 && lyrics_folded.contains(folded_query) {
            lyric_phrase_bonus = 4.0;
        }
        let lyric_tokens = tokenize_folded(&lyrics_folded);
        matched_tokens = 0;
        token_score_sum = 0.0;
        for (idx, token) in tokens.iter().enumerate() {
            let l_sim = field_token_similarity(token, &syn_groups[idx], &lyric_tokens);
            let best = per_token_meta[idx].max(score_token_match(l_sim, 3.0));
            if best > 0.0 {
                matched_tokens += 1;
                token_score_sum += best;
            }
        }
    }

    if candidate.strict_fts {
        matched_tokens = matched_tokens.max(tokens.len());
    }

    if matched_tokens == 0 {
        return Ok(None);
    }

    let coverage = matched_tokens as f64 / tokens.len() as f64;
    let phrase_bonus = if title_folded == folded_query {
        40.0
    } else if title_folded.starts_with(folded_query) {
        20.0
    } else if title_folded.contains(folded_query) {
        10.0
    } else {
        0.0
    } + lyric_phrase_bonus;
    let concise_bonus = if matched_tokens == tokens.len() && title_hit {
        2.0 / title_tokens.len().max(1) as f64
    } else {
        0.0
    };
    let bm25_bonus = candidate
        .fts_bm25
        .map(|b| (-b).clamp(0.0, 20.0) * 0.15)
        .or_else(|| candidate.trigram_bm25.map(|b| (-b).clamp(0.0, 10.0) * 0.05))
        .unwrap_or(0.0);

    let score = token_score_sum + coverage * 25.0 + phrase_bonus + concise_bonus + bm25_bonus;

    Ok(Some(RankedSongRow {
        rowid: candidate.rowid,
        score,
        title_hit,
        author_hit,
        ccli_hit,
        tag_hit,
    }))
}

pub(crate) fn open_bible_db(path: &Path) -> anyhow::Result<Connection> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.execute_batch("PRAGMA trusted_schema = OFF;")?;
    let schema: String = db.query_row(
        "SELECT value FROM bible_meta WHERE key='schema_version'",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        schema == BIBLE_SCHEMA_VERSION,
        "unsupported Bible schema version {schema}"
    );
    Ok(db)
}

/// Write a per-translation Bible SQLite database (`library/bibles/<code>.db`) atomically.
/// Used by M2.3 search tests and M2.5 bundled/imported Bibles.
pub fn write_bible_database(
    path: &Path,
    code: &str,
    name: &str,
    language: &str,
    verses: &[BibleVerseRecord<'_>],
) -> anyhow::Result<()> {
    write_bible_database_with_meta(path, code, name, language, false, verses)
}

pub(crate) fn write_bible_database_with_meta(
    path: &Path,
    code: &str,
    name: &str,
    language: &str,
    is_bundled: bool,
    verses: &[BibleVerseRecord<'_>],
) -> anyhow::Result<()> {
    ensure!(is_valid_translation_code(code), "invalid translation code");
    ensure!(!name.trim().is_empty(), "Bible name must not be empty");
    ensure!(
        !language.trim().is_empty(),
        "Bible language must not be empty"
    );

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let temp_path = tempfile::Builder::new()
        .prefix(".bible-")
        .suffix(".db.tmp")
        .tempfile_in(parent)?
        .into_temp_path();

    {
        let mut db = Connection::open(&temp_path)?;
        db.execute_batch(
            "PRAGMA journal_mode = OFF;
             PRAGMA foreign_keys = ON;
             PRAGMA trusted_schema = OFF;
             PRAGMA user_version = 1;
             CREATE TABLE bible_meta (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             ) STRICT;
             CREATE TABLE books (
                 book_num INTEGER PRIMARY KEY,
                 code TEXT NOT NULL UNIQUE,
                 name TEXT NOT NULL
             ) STRICT;
             CREATE TABLE verses (
                 id INTEGER PRIMARY KEY,
                 book_code TEXT NOT NULL,
                 book_name TEXT NOT NULL,
                 chapter INTEGER NOT NULL CHECK(chapter > 0),
                 verse INTEGER NOT NULL CHECK(verse > 0),
                 text TEXT NOT NULL,
                 UNIQUE(book_code, chapter, verse)
             ) STRICT;
             CREATE VIRTUAL TABLE verses_fts USING fts5(
                 text,
                 content='verses',
                 content_rowid='id',
                 tokenize = 'unicode61 remove_diacritics 2'
             );
             CREATE VIRTUAL TABLE verses_trigram USING fts5(
                 text,
                 content='verses',
                 content_rowid='id',
                 tokenize = 'trigram'
             );",
        )?;

        let tx = db.transaction()?;
        for (k, v) in [
            ("schema_version", BIBLE_SCHEMA_VERSION),
            ("code", code),
            ("name", name),
            ("language", language),
            ("is_bundled", if is_bundled { "1" } else { "0" }),
        ] {
            tx.execute(
                "INSERT INTO bible_meta (key, value) VALUES (?1, ?2)",
                params![k, v],
            )?;
        }

        let mut seen_books = BTreeSet::new();
        {
            let mut insert_book = tx.prepare(
                "INSERT OR IGNORE INTO books (book_num, code, name) VALUES (?1, ?2, ?3)",
            )?;
            let mut insert_verse = tx.prepare(
                "INSERT INTO verses (book_code, book_name, chapter, verse, text) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for v in verses {
                if seen_books.insert(v.book_num) {
                    insert_book.execute(params![v.book_num, v.book_code, v.book_name])?;
                }
                insert_verse.execute(params![
                    v.book_code,
                    v.book_name,
                    v.chapter,
                    v.verse,
                    v.text
                ])?;
            }
        }
        tx.execute_batch(
            "INSERT INTO verses_fts(verses_fts) VALUES ('rebuild');
             INSERT INTO verses_trigram(verses_trigram) VALUES ('rebuild');
             INSERT INTO verses_fts(verses_fts) VALUES ('optimize');",
        )?;
        tx.commit()?;
    }

    let sync_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&temp_path)?;
    sync_file.sync_all()?;
    drop(sync_file);
    temp_path.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_text_normalises_diacritics_ligatures_and_typography() {
        // Spanish, Portuguese, French (precomposed + ligatures), Yoruba (precomposed + combining)
        assert_eq!(fold_text("Señor Corazón"), "senor corazon");
        assert_eq!(fold_text("Bênção São"), "bencao sao");
        assert_eq!(fold_text("Grâce Cœur Œuvre"), "grace coeur oeuvre");
        assert_eq!(fold_text("Ọlọ́run"), "olorun");
        assert_eq!(fold_text("O\u{0323}lo\u{0323}\u{0301}run"), "olorun");
        // Typographic apostrophes/dashes and German eszett
        assert_eq!(fold_text("King’s—Straße"), "king's-strasse");
    }

    #[test]
    fn damerau_levenshtein_counts_adjacent_transpositions_as_one_edit() {
        assert_eq!(damerau_levenshtein("grace", "grace"), 0);
        assert_eq!(damerau_levenshtein("grace", "grcae"), 1);
        assert_eq!(damerau_levenshtein("amazing", "amzing"), 1);
        assert_eq!(damerau_levenshtein("wretch", "wrecth"), 1);
        assert_eq!(damerau_levenshtein("abc", ""), 3);
        assert_eq!(damerau_levenshtein("", "xyz"), 3);
    }

    #[test]
    fn helpers_validate_synonyms_translation_codes_snippets_and_trigrams() {
        assert_eq!(validate_synonym_term("  Șalom ").unwrap(), "salom");
        assert!(validate_synonym_term("two words").is_err());
        assert!(validate_synonym_term("bad\0byte").is_err());
        assert!(validate_synonym_term("").is_err());

        for valid in ["KJV", "WEB", "rvr-1960", "nvi_pt"] {
            assert!(is_valid_translation_code(valid), "{valid}");
        }
        for invalid in ["", "../evil", "a/b", "kjv.db", "bad code", &"x".repeat(33)] {
            assert!(!is_valid_translation_code(invalid), "{invalid}");
        }

        assert_eq!(truncate_snippet("  short line  ", 50), "short line");
        let truncated =
            truncate_snippet("Amazing grace how sweet the sound that saved a wretch", 24);
        assert!(truncated.ends_with("..."));
        assert!(truncated.len() <= 27);

        // Adversarial double quotes are stripped when building trigram FTS5 queries
        let expr = trigram_expr_for_tokens(&[r#"gr"ac"e"#.into(), "hi".into()]).unwrap();
        assert!(!expr.contains(r#""""#));
        assert_eq!(expr, r#""gra" OR "rac" OR "ace""#);
    }
}
