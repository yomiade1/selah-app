#![forbid(unsafe_code)]
//! The knowledge base (KNOWLEDGE-BASE §6–7): parse and validate articles, lint them
//! ([`lint`]), build `kb.db` and query it for core's help.proto answers ([`db::HelpIndex`]).
pub mod content;
pub mod db;
pub mod front;
pub mod lint;
pub mod markdown;
pub mod readability;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub use content::Context;
pub use db::{Entry, HelpIndex};
pub use lint::{Article, Finding, Options, Severity};

/// Every article under `content/kb/<lang>/` (templates and `media/` left out), sorted by path.
pub fn article_files(root: &Path) -> Vec<(String, PathBuf)> {
    let kb = root.join("content/kb");
    let mut out = Vec::new();
    let Ok(languages) = std::fs::read_dir(&kb) else {
        return out;
    };
    for language in languages.flatten() {
        let name = language.file_name().to_string_lossy().into_owned();
        if !language.path().is_dir() || name.starts_with('_') || name == "media" {
            continue;
        }
        let mut stack = vec![language.path()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "md") {
                    out.push((name.clone(), path));
                }
            }
        }
    }
    out.sort();
    out
}

/// The result of reading and linting the whole knowledge base.
pub struct Corpus {
    pub articles: Vec<Article>,
    pub findings: Vec<Finding>,
}

impl Corpus {
    pub fn errors(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Read, parse and lint every article.
pub fn load(root: &Path, context: &Context, options: &Options) -> Corpus {
    let mut articles = Vec::new();
    let mut findings = Vec::new();
    for (language, path) in article_files(root) {
        let repo_path = relative(root, &path);
        let within = relative(&root.join("content/kb").join(&language), &path);
        match std::fs::read_to_string(&path) {
            Ok(source) => match lint::parse_article(&repo_path, &language, &within, &source) {
                Ok(article) => articles.push(article),
                Err(errors) => findings.extend(errors),
            },
            Err(error) => findings.push(Finding {
                rule: "KB-SCHEMA",
                severity: Severity::Error,
                path: repo_path,
                line: 1,
                message: format!("can't read the file: {error}"),
            }),
        }
    }
    let mut seen = BTreeSet::new();
    for article in &articles {
        if !seen.insert((article.language.clone(), article.front.id.clone())) {
            findings.push(Finding {
                rule: "KB-ID",
                severity: Severity::Error,
                path: article.path.clone(),
                line: 2,
                message: format!("id `{}` is used by another article", article.front.id),
            });
        }
    }
    let known: BTreeSet<String> = articles.iter().map(|a| a.front.id.clone()).collect();
    for article in &articles {
        findings.extend(lint::lint_article(article, context, &known, options));
    }
    findings.extend(lint::lint_coverage(context, &articles, options));
    Corpus { articles, findings }
}

/// Lint and, if there are no errors, write `kb.db`.
pub fn build(root: &Path, out: &Path, options: &Options) -> anyhow::Result<Corpus> {
    let context = Context::load(root)?;
    let corpus = load(root, &context, options);
    if corpus.errors().next().is_none() {
        let mut synonyms = Vec::new();
        for (term, words) in &context.product_terms {
            for word in words {
                synonyms.push((term.clone(), word.clone()));
            }
        }
        for article in &corpus.articles {
            for keyword in &article.front.keywords {
                for word in db::normalise(keyword) {
                    synonyms.push((article.front.title.to_lowercase(), word));
                }
            }
        }
        let collections: Vec<(String, String)> = context.collections.clone().into_iter().collect();
        db::build(out, &corpus.articles, &synonyms, &collections)?;
    }
    Ok(corpus)
}

/// The repository root, found from a starting folder (the one holding `content/kb`).
pub fn find_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|p| p.join("content/kb").is_dir())
        .map(Path::to_owned)
}
