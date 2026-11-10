//! Bundled KJV/WEB Bible seeding (H-06), metadata listing, passage lookup, and deletion.
use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, ensure};
use flate2::read::GzDecoder;
use selah_proto::v1::{
    BibleBookInfo, BibleList, BibleTranslation, ScripturePassages, ScriptureSpan, ScriptureVerse,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::books::book_by_num;
use super::reference::{MAX_REFERENCE_SPANS, format_span, format_spans_summary, parse_reference};
use crate::library::{
    BibleVerseRecord, CommandBusError, Library, is_valid_translation_code, open_bible_db,
    write_bible_database_with_meta,
};

pub const KJV_TSV_GZ: &[u8] = include_bytes!("../../../../content/bibles/kjv.tsv.gz");
pub const KJV_TSV_GZ_SHA256: &str =
    "d47745bced43d6a23bd15846fcb5ecfff4e0b942f76cbb4e6ed3c503c0f796d7";

pub const WEB_TSV_GZ: &[u8] = include_bytes!("../../../../content/bibles/web.tsv.gz");
pub const WEB_TSV_GZ_SHA256: &str =
    "a3a811b6dc9c2fbd7d725af2ee762ea250999f3bf8773532cdd718a0766310d1";

pub const MAX_PASSAGE_VERSES: usize = 500;
const MAX_BUNDLED_UNCOMPRESSED_BYTES: u64 = 16 * 1024 * 1024;
const BUNDLED_MARKER_FILE: &str = ".bundled-bibles.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct BundledSeedMarker {
    seeded: Vec<String>,
}

struct BundledSpec {
    code: &'static str,
    name: &'static str,
    language: &'static str,
    gz_bytes: &'static [u8],
    expected_sha256: &'static str,
}

const BUNDLED_SPECS: [BundledSpec; 2] = [
    BundledSpec {
        code: "KJV",
        name: "King James Version",
        language: "en",
        gz_bytes: KJV_TSV_GZ,
        expected_sha256: KJV_TSV_GZ_SHA256,
    },
    BundledSpec {
        code: "WEB",
        name: "World English Bible",
        language: "en",
        gz_bytes: WEB_TSV_GZ,
        expected_sha256: WEB_TSV_GZ_SHA256,
    },
];

struct BundledRow {
    book_num: u32,
    book_code: &'static str,
    book_name: &'static str,
    chapter: u32,
    verse: u32,
    text: String,
}

fn verify_and_decode_tsv(
    gz_bytes: &[u8],
    expected_sha256: &str,
) -> anyhow::Result<Vec<BundledRow>> {
    let mut hasher = Sha256::new();
    hasher.update(gz_bytes);
    let actual = format!("{:x}", hasher.finalize());
    ensure!(
        actual == expected_sha256,
        "bundled Bible SHA-256 mismatch: expected {expected_sha256}, got {actual}"
    );

    let decoder = GzDecoder::new(gz_bytes).take(MAX_BUNDLED_UNCOMPRESSED_BYTES + 1);
    let mut reader = BufReader::new(decoder);
    let mut total_read = 0_u64;
    let mut rows = Vec::with_capacity(31_150);
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        total_read = total_read.saturating_add(n as u64);
        ensure!(
            total_read <= MAX_BUNDLED_UNCOMPRESSED_BYTES,
            "bundled Bible decompressed stream exceeded limit"
        );
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.splitn(4, '\t');
        let book_num: u32 = parts
            .next()
            .context("missing book_num in bundled TSV")?
            .parse()
            .context("invalid book_num in bundled TSV")?;
        let book = book_by_num(book_num)
            .with_context(|| format!("unknown book_num {book_num} in bundled TSV"))?;
        let chapter: u32 = parts
            .next()
            .context("missing chapter in bundled TSV")?
            .parse()
            .context("invalid chapter in bundled TSV")?;
        let verse: u32 = parts
            .next()
            .context("missing verse in bundled TSV")?
            .parse()
            .context("invalid verse in bundled TSV")?;
        let text = parts
            .next()
            .context("missing text in bundled TSV")?
            .to_owned();
        rows.push(BundledRow {
            book_num,
            book_code: book.code,
            book_name: book.name,
            chapter,
            verse,
            text,
        });
    }

    ensure!(!rows.is_empty(), "bundled Bible TSV was empty");
    Ok(rows)
}

fn read_seed_marker(bibles_root: &Path) -> anyhow::Result<BundledSeedMarker> {
    let marker_path = bibles_root.join(BUNDLED_MARKER_FILE);
    match std::fs::symlink_metadata(&marker_path) {
        Ok(meta) => {
            ensure!(
                !meta.file_type().is_symlink() && meta.is_file(),
                "bundled Bible marker must be a regular non-symlink file"
            );
            let bytes = std::fs::read(&marker_path)?;
            Ok(serde_json::from_slice(&bytes).unwrap_or_default())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BundledSeedMarker::default()),
        Err(e) => Err(e.into()),
    }
}

fn write_seed_marker(bibles_root: &Path, marker: &BundledSeedMarker) -> anyhow::Result<()> {
    let marker_path = bibles_root.join(BUNDLED_MARKER_FILE);
    let json = serde_json::to_vec_pretty(marker)?;
    let mut temp = tempfile::Builder::new()
        .prefix(".bundled-")
        .suffix(".json.tmp")
        .tempfile_in(bibles_root)?;
    temp.write_all(&json)?;
    temp.as_file().sync_all()?;
    temp.persist(&marker_path).map_err(|e| e.error)?;
    Ok(())
}

/// Ensure bundled public-domain KJV & WEB Bibles (H-06) are materialized once in `bibles_dir`.
/// Already-seeded translations that the user deleted are tracked in `.bundled-bibles.json` and
/// are not resurrected on subsequent startups. Existing files are never overwritten.
pub fn ensure_bundled_bibles(bibles_dir: &Path) -> anyhow::Result<bool> {
    std::fs::create_dir_all(bibles_dir)?;
    let root = bibles_dir.canonicalize()?;
    let mut marker = read_seed_marker(&root)?;
    let already_seeded: HashSet<String> = marker
        .seeded
        .iter()
        .map(|s| s.to_ascii_uppercase())
        .collect();

    if BUNDLED_SPECS
        .iter()
        .all(|spec| already_seeded.contains(spec.code))
    {
        return Ok(false);
    }

    let mut existing_codes = HashSet::new();
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("db")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            existing_codes.insert(stem.to_ascii_uppercase());
        }
    }

    let mut seeded_any = false;
    let mut marker_dirty = false;
    for spec in &BUNDLED_SPECS {
        if already_seeded.contains(spec.code) {
            continue;
        }
        if existing_codes.contains(spec.code) {
            marker.seeded.push(spec.code.to_owned());
            marker_dirty = true;
            continue;
        }
        let target = root.join(format!("{}.db", spec.code));
        if std::fs::symlink_metadata(&target).is_err() {
            let rows = verify_and_decode_tsv(spec.gz_bytes, spec.expected_sha256)?;
            let records: Vec<BibleVerseRecord<'_>> = rows
                .iter()
                .map(|r| BibleVerseRecord {
                    book_num: r.book_num,
                    book_code: r.book_code,
                    book_name: r.book_name,
                    chapter: r.chapter,
                    verse: r.verse,
                    text: &r.text,
                })
                .collect();
            write_bible_database_with_meta(
                &target,
                spec.code,
                spec.name,
                spec.language,
                true,
                &records,
            )?;
            seeded_any = true;
        }
        marker.seeded.push(spec.code.to_owned());
        marker_dirty = true;
    }

    if marker_dirty {
        marker.seeded.sort();
        marker.seeded.dedup();
        write_seed_marker(&root, &marker)?;
    }

    Ok(seeded_any)
}

#[derive(Clone, Debug)]
struct BibleHeader {
    code: String,
    name: String,
    language: String,
    is_bundled: bool,
    book_count: u32,
    verse_count: u32,
}

fn read_bible_header(db: &rusqlite::Connection) -> anyhow::Result<BibleHeader> {
    let mut code = String::new();
    let mut name = String::new();
    let mut language = String::new();
    let mut is_bundled = false;

    let mut stmt = db.prepare("SELECT key, value FROM bible_meta")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (k, v) = row?;
        match k.as_str() {
            "code" => code = v,
            "name" => name = v,
            "language" => language = v,
            "is_bundled" => is_bundled = v == "1" || v.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    ensure!(!code.is_empty(), "Bible metadata missing code");
    if name.is_empty() {
        name = code.clone();
    }
    if language.is_empty() {
        language = "en".into();
    }
    let (book_count, verse_count): (u32, u32) = db.query_row(
        "SELECT (SELECT COUNT(*) FROM books), (SELECT COUNT(*) FROM verses)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(BibleHeader {
        code,
        name,
        language,
        is_bundled,
        book_count,
        verse_count,
    })
}

fn read_bible_books(db: &rusqlite::Connection) -> anyhow::Result<Vec<BibleBookInfo>> {
    let mut stmt = db.prepare("SELECT book_num, code, name FROM books ORDER BY book_num")?;
    let mut by_code: BTreeMap<String, BibleBookInfo> = BTreeMap::new();
    let mut book_order: Vec<String> = Vec::new();
    let book_rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in book_rows {
        let (book_num, code, name) = row?;
        book_order.push(code.clone());
        by_code.insert(
            code.clone(),
            BibleBookInfo {
                book_num,
                code,
                name,
                chapter_count: 0,
                verse_counts: Vec::new(),
            },
        );
    }

    let mut ch_stmt = db.prepare(
        "SELECT book_code, chapter, MAX(verse) FROM verses GROUP BY book_code, chapter ORDER BY chapter",
    )?;
    let ch_rows = ch_stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, u32>(1)?,
            r.get::<_, u32>(2)?,
        ))
    })?;
    for row in ch_rows {
        let (book_code, chapter, max_v) = row?;
        if let Some(info) = by_code.get_mut(&book_code) {
            let idx = chapter.saturating_sub(1) as usize;
            if idx >= info.verse_counts.len() {
                info.verse_counts.resize(idx + 1, 0);
            }
            info.verse_counts[idx] = max_v;
            info.chapter_count = info.chapter_count.max(chapter);
        }
    }

    Ok(book_order
        .into_iter()
        .filter_map(|code| by_code.remove(&code))
        .collect())
}

pub fn list_bibles(library: &Library, include_books: bool) -> Result<BibleList, CommandBusError> {
    let files = library
        .list_validated_bible_files()
        .map_err(CommandBusError::Internal)?;
    let mut bibles = Vec::with_capacity(files.len());
    for (_, path) in files {
        let db = open_bible_db(&path).map_err(CommandBusError::Internal)?;
        let header = read_bible_header(&db).map_err(CommandBusError::Internal)?;
        let books = if include_books {
            read_bible_books(&db).map_err(CommandBusError::Internal)?
        } else {
            Vec::new()
        };
        bibles.push(BibleTranslation {
            code: header.code,
            name: header.name,
            language: header.language,
            book_count: header.book_count,
            verse_count: header.verse_count,
            is_bundled: header.is_bundled,
            books,
        });
    }
    Ok(BibleList { bibles })
}

fn find_validated_bible(
    library: &Library,
    translation: &str,
) -> Result<(String, PathBuf), CommandBusError> {
    let code = translation.trim();
    if code.is_empty() || !is_valid_translation_code(code) {
        return Err(CommandBusError::Invalid(
            "invalid Bible translation code".into(),
        ));
    }
    let files = library
        .list_validated_bible_files()
        .map_err(CommandBusError::Internal)?;
    files
        .into_iter()
        .find(|(stored, _)| stored.eq_ignore_ascii_case(code))
        .ok_or_else(|| CommandBusError::NotFound(format!("Bible translation '{code}' not found")))
}

fn validate_external_spans(spans: &[ScriptureSpan]) -> anyhow::Result<Vec<ScriptureSpan>> {
    ensure!(!spans.is_empty(), "scripture spans cannot be empty");
    ensure!(
        spans.len() <= MAX_REFERENCE_SPANS,
        "too many scripture spans (max {MAX_REFERENCE_SPANS})"
    );
    let mut out = Vec::with_capacity(spans.len());
    for s in spans {
        let book = if s.book_num != 0 {
            book_by_num(s.book_num).context("invalid book_num in scripture span")?
        } else {
            super::books::book_by_code(&s.book_code)
                .context("missing or unknown book_code in scripture span")?
        };
        ensure!(
            s.start_chapter > 0 && s.end_chapter >= s.start_chapter,
            "invalid chapter range in scripture span"
        );
        if s.start_chapter == s.end_chapter && (s.start_verse != 0 || s.end_verse != 0) {
            ensure!(
                s.start_verse > 0 && s.end_verse >= s.start_verse,
                "invalid verse range in scripture span"
            );
        }
        out.push(ScriptureSpan {
            book_num: book.book_num,
            book_code: book.code.to_owned(),
            book_name: book.name.to_owned(),
            start_chapter: s.start_chapter,
            start_verse: s.start_verse,
            end_chapter: s.end_chapter,
            end_verse: s.end_verse,
            normalized: format_span(
                book.name,
                s.start_chapter,
                s.start_verse,
                s.end_chapter,
                s.end_verse,
            ),
        });
    }
    Ok(out)
}

pub fn get_passages(
    library: &Library,
    translation: &str,
    reference: &str,
    spans: &[ScriptureSpan],
    language_hint: &str,
) -> Result<ScripturePassages, CommandBusError> {
    let (_, path) = find_validated_bible(library, translation)?;
    let db = open_bible_db(&path).map_err(CommandBusError::Internal)?;
    let header = read_bible_header(&db).map_err(CommandBusError::Internal)?;

    let resolved_spans = if !spans.is_empty() {
        validate_external_spans(spans).map_err(|e| CommandBusError::Invalid(e.to_string()))?
    } else if !reference.trim().is_empty() {
        let hint = if language_hint.trim().is_empty() {
            header.language.as_str()
        } else {
            language_hint.trim()
        };
        parse_reference(reference, Some(hint))
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?
            .spans
    } else {
        return Err(CommandBusError::Invalid(
            "either reference text or scripture spans must be provided".into(),
        ));
    };

    let normalized_reference = format_spans_summary(&resolved_spans);
    let mut verses = Vec::new();
    let mut seen: HashSet<(u32, u32, u32)> = HashSet::new();

    let mut book_check = db.prepare_cached("SELECT name FROM books WHERE book_num = ?1")?;
    let mut ch_stmt = db.prepare_cached(
        "SELECT book_name, chapter, verse, text FROM verses
         WHERE book_code = ?1 AND chapter >= ?2 AND chapter <= ?3
         ORDER BY chapter, verse",
    )?;
    let mut same_ch_stmt = db.prepare_cached(
        "SELECT book_name, chapter, verse, text FROM verses
         WHERE book_code = ?1 AND chapter = ?2 AND verse >= ?3 AND verse <= ?4
         ORDER BY verse",
    )?;
    let mut cross_ch_stmt = db.prepare_cached(
        "SELECT book_name, chapter, verse, text FROM verses
         WHERE book_code = ?1 AND (
             (chapter = ?2 AND verse >= ?3)
             OR (chapter > ?2 AND chapter < ?4)
             OR (chapter = ?4 AND (?5 = 0 OR verse <= ?5))
         )
         ORDER BY chapter, verse",
    )?;

    for span in &resolved_spans {
        let book_exists: Option<String> = book_check.query_row([span.book_num], |r| r.get(0)).ok();
        if book_exists.is_none() {
            return Err(CommandBusError::NotFound(format!(
                "book '{}' is not in translation '{}'",
                span.book_name, header.code
            )));
        }

        let rows: Vec<(String, u32, u32, String)> = if span.start_verse == 0 && span.end_verse == 0
        {
            ch_stmt
                .query_map(
                    rusqlite::params![&span.book_code, span.start_chapter, span.end_chapter],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else if span.start_chapter == span.end_chapter {
            same_ch_stmt
                .query_map(
                    rusqlite::params![
                        &span.book_code,
                        span.start_chapter,
                        span.start_verse,
                        span.end_verse
                    ],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            let sv = span.start_verse.max(1);
            cross_ch_stmt
                .query_map(
                    rusqlite::params![
                        &span.book_code,
                        span.start_chapter,
                        sv,
                        span.end_chapter,
                        span.end_verse
                    ],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };

        if rows.is_empty() {
            return Err(CommandBusError::NotFound(format!(
                "passage '{}' not found in translation '{}'",
                span.normalized, header.code
            )));
        }

        for (book_name, chapter, verse, text) in rows {
            if seen.insert((span.book_num, chapter, verse)) {
                if verses.len() >= MAX_PASSAGE_VERSES {
                    return Err(CommandBusError::Invalid(format!(
                        "passage exceeds maximum {MAX_PASSAGE_VERSES} verses"
                    )));
                }
                verses.push(ScriptureVerse {
                    translation: header.code.clone(),
                    book_num: span.book_num,
                    book_code: span.book_code.clone(),
                    book_name,
                    chapter,
                    verse,
                    text,
                });
            }
        }
    }

    Ok(ScripturePassages {
        translation: Some(BibleTranslation {
            code: header.code,
            name: header.name,
            language: header.language,
            book_count: header.book_count,
            verse_count: header.verse_count,
            is_bundled: header.is_bundled,
            books: Vec::new(),
        }),
        normalized_reference,
        verses,
    })
}

pub fn delete_bible(library: &Library, code: &str) -> Result<bool, CommandBusError> {
    let (_, canonical) = find_validated_bible(library, code)?;
    let bibles_root = library
        .paths()
        .bibles
        .canonicalize()
        .map_err(|e| CommandBusError::Internal(e.into()))?;
    let meta =
        std::fs::symlink_metadata(&canonical).map_err(|e| CommandBusError::Internal(e.into()))?;
    if meta.file_type().is_symlink() || !meta.is_file() || !canonical.starts_with(&bibles_root) {
        return Err(CommandBusError::Invalid(
            "Bible database path is outside library/bibles".into(),
        ));
    }
    std::fs::remove_file(&canonical).map_err(|e| CommandBusError::Internal(e.into()))?;
    for suffix in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{suffix}", canonical.display()));
        if side.exists() {
            let _ = std::fs::remove_file(&side);
        }
    }
    Ok(true)
}
