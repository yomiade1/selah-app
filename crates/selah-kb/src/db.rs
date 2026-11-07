//! `kb.db` (KNOWLEDGE-BASE §6.4): FTS5 search (title 10, keywords 6, summary 4, body 1), a
//! trigram table for typos, the anchor → article map, collections and each article's AST.
use std::path::Path;

use rusqlite::{Connection, OpenFlags, params};

use crate::lint::Article;
use crate::markdown;

pub const SCHEMA_VERSION: i64 = 1;

/// One search result or article.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub language: String,
    pub title: String,
    pub summary: String,
    pub kind: String,
    pub ast_json: String,
}

fn body_text(article: &Article) -> String {
    markdown::walk(&article.doc.blocks)
        .iter()
        .flat_map(|b| markdown::block_inlines(b))
        .map(markdown::inline_text)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Write `kb.db` from parsed articles. Synonyms come from terms and keywords.
pub fn build(
    path: &Path,
    articles: &[Article],
    synonyms: &[(String, String)],
    collections: &[(String, String)],
) -> anyhow::Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut db = Connection::open(path)?;
    db.execute_batch(
        "PRAGMA journal_mode = OFF;
         CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE articles (id TEXT NOT NULL, language TEXT NOT NULL, title TEXT NOT NULL,
             summary TEXT NOT NULL, kind TEXT NOT NULL, status TEXT NOT NULL, keywords TEXT NOT NULL,
             audience TEXT NOT NULL, modes TEXT NOT NULL, ast_json TEXT NOT NULL,
             PRIMARY KEY (id, language));
         CREATE VIRTUAL TABLE articles_fts USING fts5(id UNINDEXED, language UNINDEXED, title, keywords,
             summary, body, tokenize = 'porter unicode61');
         CREATE VIRTUAL TABLE titles_trigram USING fts5(id UNINDEXED, language UNINDEXED, text,
             tokenize = 'trigram');
         CREATE TABLE anchors (anchor TEXT NOT NULL, article_id TEXT NOT NULL);
         CREATE TABLE collections (id TEXT PRIMARY KEY, title TEXT NOT NULL);
         CREATE TABLE collection_articles (collection_id TEXT NOT NULL, article_id TEXT NOT NULL);
         CREATE TABLE synonyms (term TEXT NOT NULL, synonym TEXT NOT NULL);",
    )?;
    let tx = db.transaction()?;
    tx.execute(
        "INSERT INTO meta VALUES ('schema_version', ?1)",
        params![SCHEMA_VERSION.to_string()],
    )?;
    for article in articles {
        let f = &article.front;
        let ast = serde_json::to_string(&article.doc)?;
        tx.execute(
            "INSERT INTO articles VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                f.id,
                article.language,
                f.title,
                f.summary,
                f.kind,
                f.status,
                f.keywords.join(", "),
                f.audience.join(","),
                f.modes.join(","),
                ast
            ],
        )?;
        tx.execute(
            "INSERT INTO articles_fts VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                f.id,
                article.language,
                f.title,
                f.keywords.join(" "),
                f.summary,
                body_text(article)
            ],
        )?;
        tx.execute(
            "INSERT INTO titles_trigram VALUES (?1, ?2, ?3)",
            params![
                f.id,
                article.language,
                format!("{} {}", f.title, f.keywords.join(" "))
            ],
        )?;
        for anchor in &f.context {
            tx.execute("INSERT INTO anchors VALUES (?1, ?2)", params![anchor, f.id])?;
        }
        for collection in &f.collections {
            tx.execute(
                "INSERT INTO collection_articles VALUES (?1, ?2)",
                params![collection, f.id],
            )?;
        }
    }
    for (id, title) in collections {
        tx.execute(
            "INSERT INTO collections VALUES (?1, ?2)",
            params![id, title],
        )?;
    }
    for (term, synonym) in synonyms {
        tx.execute(
            "INSERT INTO synonyms VALUES (?1, ?2)",
            params![term, synonym],
        )?;
    }
    tx.commit()?;
    db.execute_batch("INSERT INTO articles_fts(articles_fts) VALUES ('optimize'); VACUUM;")?;
    Ok(())
}

/// Read-only access to `kb.db`, used by core to answer help.proto queries.
pub struct HelpIndex {
    db: Connection,
}

/// Question words removed before ranking ("how do I", "why is my").
const QUESTION_WORDS: &[&str] = &[
    "how", "do", "does", "i", "can", "why", "is", "are", "my", "the", "a", "an", "what", "to",
    "where", "when", "it", "won't", "doesn't", "isn't",
];

/// Words of a question that matter for search.
pub fn normalise(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\'').to_lowercase())
        .filter(|w| !w.is_empty() && !QUESTION_WORDS.contains(&w.as_str()))
        .take(12)
        .collect()
}

impl HelpIndex {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let version: String = db.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            version == SCHEMA_VERSION.to_string(),
            "kb.db schema {version} isn't {SCHEMA_VERSION}"
        );
        Ok(Self { db })
    }

    fn entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
        Ok(Entry {
            id: row.get(0)?,
            language: row.get(1)?,
            title: row.get(2)?,
            summary: row.get(3)?,
            kind: row.get(4)?,
            ast_json: row.get(5)?,
        })
    }

    pub fn article(&self, id: &str, language: &str) -> anyhow::Result<Option<Entry>> {
        let mut statement = self.db.prepare_cached(
            "SELECT id, language, title, summary, kind, ast_json FROM articles WHERE id = ?1 AND language IN (?2, 'en')
             ORDER BY language = ?2 DESC LIMIT 1",
        )?;
        let mut rows = statement.query_map(params![id, language], Self::entry)?;
        Ok(rows.next().transpose()?)
    }

    /// Articles offered at these anchors, in id order.
    pub fn for_context(&self, anchors: &[String], language: &str) -> anyhow::Result<Vec<Entry>> {
        let mut out = Vec::new();
        let mut statement = self.db.prepare_cached(
            "SELECT DISTINCT article_id FROM anchors WHERE anchor = ?1 ORDER BY article_id",
        )?;
        for anchor in anchors {
            let ids: Vec<String> = statement
                .query_map(params![anchor], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            for id in ids {
                if !out.iter().any(|e: &Entry| e.id == id)
                    && let Some(entry) = self.article(&id, language)?
                {
                    out.push(entry);
                }
            }
        }
        Ok(out)
    }

    /// Search: FTS5 with weights, synonyms, an anchor boost, and trigram matching for typos.
    pub fn search(
        &self,
        query: &str,
        language: &str,
        anchors: &[String],
        limit: usize,
    ) -> anyhow::Result<Vec<Entry>> {
        let mut words = normalise(query);
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut synonyms = self
            .db
            .prepare_cached("SELECT term FROM synonyms WHERE synonym = ?1")?;
        for word in words.clone() {
            let extra: Vec<String> = synonyms
                .query_map(params![word], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            words.extend(extra.into_iter().flat_map(|t| normalise(&t)));
        }
        let fts_query = words
            .iter()
            .map(|w| format!("\"{}\"*", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ");
        let boosted: std::collections::HashSet<String> = self
            .for_context(anchors, language)?
            .into_iter()
            .map(|e| e.id)
            .collect();
        let mut scored: Vec<(f64, Entry)> = Vec::new();
        let mut statement = self.db.prepare_cached(
            "SELECT a.id, a.language, a.title, a.summary, a.kind, a.ast_json, bm25(articles_fts, 0, 0, 10, 6, 4, 1)
             FROM articles_fts JOIN articles a ON a.id = articles_fts.id AND a.language = articles_fts.language
             WHERE articles_fts MATCH ?1 AND a.language = ?2",
        )?;
        let rows = statement.query_map(params![fts_query, language], |r| {
            Ok((r.get::<_, f64>(6)?, Self::entry(r)?))
        })?;
        for row in rows {
            let (rank, entry) = row?;
            scored.push((rank, entry));
        }
        if scored.is_empty() {
            // Typos: trigram matches on titles and keywords ("amzing" still finds "amazing").
            let mut trigram = self.db.prepare_cached(
                "SELECT a.id, a.language, a.title, a.summary, a.kind, a.ast_json, bm25(titles_trigram)
                 FROM titles_trigram JOIN articles a ON a.id = titles_trigram.id AND a.language = titles_trigram.language
                 WHERE titles_trigram MATCH ?1 AND a.language = ?2",
            )?;
            for word in words.iter().filter(|w| w.chars().count() >= 3) {
                let grams: Vec<String> = {
                    let chars: Vec<char> = word.chars().collect();
                    chars
                        .windows(3)
                        .map(|w| format!("\"{}\"", w.iter().collect::<String>()))
                        .collect()
                };
                let rows = trigram.query_map(params![grams.join(" OR "), language], |r| {
                    Ok((r.get::<_, f64>(6)?, Self::entry(r)?))
                })?;
                for row in rows {
                    let (rank, entry) = row?;
                    scored.push((rank, entry));
                }
            }
        }
        // bm25 is lower-is-better; anchors visible on screen get a strong boost.
        for (rank, entry) in &mut scored {
            if boosted.contains(&entry.id) {
                *rank -= 10.0;
            }
        }
        scored.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out: Vec<Entry> = Vec::new();
        for (_, entry) in scored {
            if !out.iter().any(|e| e.id == entry.id) {
                out.push(entry);
            }
            if out.len() == limit {
                break;
            }
        }
        Ok(out)
    }
}
