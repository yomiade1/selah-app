//! `selah-kb-lint` rules (KNOWLEDGE-BASE §3.1, §7.1). Rule ids are the R numbers of the style
//! guide, plus `KB-*` ids for schema, references and length.
use std::collections::{BTreeMap, BTreeSet};

use crate::content::Context;
use crate::front::{self, FrontMatter};
use crate::markdown::{self, Block, Document, Inline};
use crate::readability::{self, Stats};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub rule: &'static str,
    pub severity: Severity,
    /// Path relative to the repository root.
    pub path: String,
    pub line: usize,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Options {
    /// Release mode (M10): drafts, missing reviews and planned references are errors.
    pub release: bool,
    /// Today as days since 1970-01-01 (for `last_reviewed`).
    pub today: i64,
}

impl Default for Options {
    fn default() -> Self {
        let days = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| (d.as_secs() / 86_400) as i64)
            .unwrap_or(0);
        Self {
            release: false,
            today: days,
        }
    }
}

/// One parsed article.
#[derive(Clone, Debug)]
pub struct Article {
    /// Repository-relative path, for findings.
    pub path: String,
    /// The language folder (`en`).
    pub language: String,
    /// The id the path implies (`live/show-a-song.md` → `live.show-a-song`).
    pub path_id: String,
    pub front: FrontMatter,
    pub doc: Document,
    pub front_line: usize,
}

pub const MAX_SUMMARY_WORDS: usize = 25;
pub const MAX_POPOVER_WORDS: usize = 50;
pub const MAX_TIP_WORDS: usize = 40;
pub const MAX_ARTICLE_WORDS: usize = 600;
pub const MAX_GRADE: f64 = 6.0;
pub const MAX_AVERAGE_SENTENCE: f64 = 14.0;
pub const MAX_SENTENCE: usize = 25;
pub const MAX_PARAGRAPH_SENTENCES: usize = 3;
pub const MAX_PARAGRAPH_WORDS: usize = 60;
pub const MAX_TITLE_WORDS: usize = 8;
pub const MAX_STEPS: usize = 10;
pub const MAX_STEP_WORDS: usize = 25;

/// Parse an article; a broken file yields KB-SCHEMA findings instead.
pub fn parse_article(
    path: &str,
    language: &str,
    relative: &str,
    source: &str,
) -> Result<Article, Vec<Finding>> {
    let finding = |message: String| Finding {
        rule: "KB-SCHEMA",
        severity: Severity::Error,
        path: path.to_owned(),
        line: 1,
        message,
    };
    let split = front::split(source).map_err(|e| vec![finding(e.to_owned())])?;
    let front = front::parse(split.front)
        .map_err(|errors| errors.into_iter().map(finding).collect::<Vec<_>>())?;
    let doc = markdown::parse(split.body, split.body_line);
    let path_id = relative.trim_end_matches(".md").replace(['/', '\\'], ".");
    Ok(Article {
        path: path.to_owned(),
        language: language.to_owned(),
        path_id,
        front,
        doc,
        front_line: 1,
    })
}

fn days_from_date(date: &str) -> Option<i64> {
    let mut parts = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    // Days from civil (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn word_count(text: &str) -> usize {
    readability::words(text).len()
}

/// The words a reader reads, for readability and word lists: labels, keys and code removed.
fn prose(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Label { .. }
            | Inline::Key { .. }
            | Inline::Code { .. }
            | Inline::Image { .. } => " ".to_owned(),
            Inline::Link { inlines, .. } => prose(inlines),
            other => markdown::inline_text(other),
        })
        .collect()
}

fn phrase_found(text: &str, phrase: &str) -> bool {
    let lower = text.to_lowercase();
    let phrase = phrase.to_lowercase();
    let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
    let mut from = 0;
    while let Some(at) = lower[from..].find(&phrase) {
        let start = from + at;
        let end = start + phrase.len();
        let before = lower[..start].chars().next_back();
        let after = lower[end..].chars().next();
        let first_alnum = phrase.chars().next().is_some_and(char::is_alphanumeric);
        let last_alnum = phrase
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric);
        if (!first_alnum || boundary(before)) && (!last_alnum || boundary(after)) {
            return true;
        }
        from = start + phrase.len().max(1);
    }
    false
}

fn is_sentence_case(text: &str, context: &Context) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    for word in words.iter().skip(1) {
        let clean: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect();
        let mut chars = clean.chars();
        let Some(first) = chars.next() else { continue };
        let rest: String = chars.collect();
        let capitalised = first.is_uppercase()
            && rest.chars().any(char::is_lowercase)
            && !rest.chars().any(char::is_uppercase);
        let allowed = context
            .proper_nouns
            .iter()
            .chain(&context.readability_ignore)
            .any(|n| n == &clean);
        if capitalised && !allowed {
            return Some(clean);
        }
    }
    None
}

struct Linter<'a> {
    article: &'a Article,
    context: &'a Context,
    options: &'a Options,
    known_ids: &'a BTreeSet<String>,
    findings: Vec<Finding>,
}

impl Linter<'_> {
    fn push(
        &mut self,
        rule: &'static str,
        severity: Severity,
        line: usize,
        message: impl Into<String>,
    ) {
        self.findings.push(Finding {
            rule,
            severity,
            path: self.article.path.clone(),
            line,
            message: message.into(),
        });
    }

    fn error(&mut self, rule: &'static str, line: usize, message: impl Into<String>) {
        self.push(rule, Severity::Error, line, message);
    }

    fn warn(&mut self, rule: &'static str, line: usize, message: impl Into<String>) {
        self.push(rule, Severity::Warning, line, message);
    }

    /// A reference to something not built yet: a warning until its milestone is done.
    fn planned_or(
        &mut self,
        kind: &str,
        id: &str,
        line: usize,
        missing_rule: &'static str,
        what: &str,
    ) {
        match self.context.planned(kind, id).cloned() {
            Some(planned) => {
                let severity =
                    if self.options.release || self.context.done.contains(&planned.milestone) {
                        Severity::Error
                    } else {
                        Severity::Warning
                    };
                self.push(
                    "KB-PLANNED",
                    severity,
                    line,
                    format!(
                        "{what} `{id}` is planned for {} and doesn't exist yet",
                        planned.milestone
                    ),
                );
            }
            None => self.error(missing_rule, line, format!("unknown {what} `{id}`")),
        }
    }

    fn run(mut self) -> Vec<Finding> {
        let front = &self.article.front;
        let doc = &self.article.doc;
        let blocks = markdown::walk(&doc.blocks);

        // Schema and identity.
        if front.id != self.article.path_id {
            self.error(
                "KB-ID",
                2,
                format!(
                    "id `{}` doesn't match the path (`{}`)",
                    front.id, self.article.path_id
                ),
            );
        }
        // R1: a user need.
        let need = front.user_need.trim();
        let well_formed = (need.starts_with("As a ") || need.starts_with("As an "))
            && need.contains(", I want to ")
            && need.contains(" so that ");
        if !well_formed {
            self.error(
                "R1",
                1,
                "`user_need` must say: As a <role>, I want to <task> so that <goal>.",
            );
        }
        // Lengths.
        let summary_words = word_count(&front.summary);
        if summary_words == 0 || summary_words > MAX_SUMMARY_WORDS {
            self.error(
                "KB-LENGTH",
                1,
                format!("the summary has {summary_words} words (1 to {MAX_SUMMARY_WORDS})"),
            );
        }
        if word_count(&front.popover) > MAX_POPOVER_WORDS {
            self.error(
                "KB-LENGTH",
                1,
                format!("the popover is over {MAX_POPOVER_WORDS} words"),
            );
        }
        // R5: titles front-load and stay short.
        if word_count(&front.title) > MAX_TITLE_WORDS {
            self.warn(
                "R5",
                1,
                format!("the title is over {MAX_TITLE_WORDS} words"),
            );
        }
        // R18: sentence case.
        if let Some(word) = is_sentence_case(&front.title, self.context) {
            self.error(
                "R18",
                1,
                format!("use sentence case in the title (`{word}`)"),
            );
        }

        // Body structure: one H1 that matches the title.
        let mut prose_pieces = Vec::new();
        let mut body_words = 0;
        let mut has_h1 = false;
        for block in &blocks {
            match block {
                Block::Heading {
                    level,
                    inlines,
                    line,
                } => {
                    let text = markdown::text_of(inlines);
                    if *level == 1 {
                        has_h1 = true;
                        if text != front.title {
                            self.error(
                                "KB-SCHEMA",
                                *line,
                                "the first heading must match the title",
                            );
                        }
                    }
                    if let Some(word) = is_sentence_case(&text, self.context) {
                        self.error(
                            "R18",
                            *line,
                            format!("use sentence case in headings (`{word}`)"),
                        );
                    }
                    let lower = text.to_lowercase();
                    if lower.contains("faq")
                        || lower.starts_with("frequently asked")
                        || lower.contains("questions and answers")
                    {
                        self.error("R11", *line, "no FAQs: write a task or a fix instead");
                    }
                }
                Block::Paragraph { inlines, line } => {
                    let text = prose(inlines);
                    body_words += word_count(&markdown::text_of(inlines));
                    prose_pieces.push((text, *line));
                }
                Block::Html { line } => self.error("KB-HTML", *line, "raw HTML isn't allowed"),
                _ => {}
            }
        }
        if !has_h1 {
            self.error(
                "KB-SCHEMA",
                1,
                "the article needs a # heading with its title",
            );
        }
        if body_words > MAX_ARTICLE_WORDS {
            self.error(
                "KB-LENGTH",
                1,
                format!(
                    "the article has {body_words} words; split it (at most {MAX_ARTICLE_WORDS})"
                ),
            );
        }
        if front.kind == "tip" && body_words > MAX_TIP_WORDS {
            self.error(
                "KB-LENGTH",
                1,
                format!("a tip is at most {MAX_TIP_WORDS} words"),
            );
        }

        // R2, R3: reading level and sentence length over the whole body.
        let ignore = &self.context.readability_ignore;
        let cleaned: Vec<String> = prose_pieces
            .iter()
            .map(|(text, _)| {
                let mut text = text.clone();
                for term in ignore {
                    text = text.replace(term.as_str(), " ");
                }
                text
            })
            .collect();
        let stats = Stats::of(&cleaned);
        if stats.grade() > MAX_GRADE {
            self.error(
                "R2",
                1,
                format!(
                    "reading level is grade {:.1}; aim for {MAX_GRADE} or lower",
                    stats.grade()
                ),
            );
        }
        if stats.average_sentence() > MAX_AVERAGE_SENTENCE {
            self.error(
                "R3",
                1,
                format!(
                    "sentences average {:.1} words (at most {MAX_AVERAGE_SENTENCE})",
                    stats.average_sentence()
                ),
            );
        }
        for (text, line) in &prose_pieces {
            for sentence in readability::sentences(text) {
                let n = word_count(&sentence);
                if n > MAX_SENTENCE {
                    self.error(
                        "R3",
                        *line,
                        format!("a sentence has {n} words (at most {MAX_SENTENCE})"),
                    );
                }
            }
        }
        // R4: short paragraphs (top-level and callout paragraphs; list items are steps).
        let mut paragraphs: Vec<&Block> = doc.blocks.iter().collect();
        for block in &doc.blocks {
            if let Block::Callout { blocks, .. } | Block::Quote { blocks, .. } = block {
                paragraphs.extend(blocks.iter());
            }
        }
        for block in paragraphs {
            if let Block::Paragraph { inlines, line } = block {
                let text = markdown::text_of(inlines);
                let (sentences, words) = (readability::sentences(&text).len(), word_count(&text));
                if sentences > MAX_PARAGRAPH_SENTENCES || words > MAX_PARAGRAPH_WORDS {
                    self.error("R4", *line, format!("a paragraph has {sentences} sentences and {words} words (at most {MAX_PARAGRAPH_SENTENCES} and {MAX_PARAGRAPH_WORDS})"));
                }
            }
        }
        // R6: passive voice (warning).
        for (text, line) in &prose_pieces {
            let words: Vec<String> = readability::words(text)
                .iter()
                .map(|w| w.to_lowercase())
                .collect();
            for window in words.windows(3) {
                if ["is", "are", "was", "were", "be", "been"].contains(&window[0].as_str())
                    && window[1].ends_with("ed")
                    && window[2].trim_matches(|c: char| !c.is_alphanumeric()) == "by"
                {
                    self.warn(
                        "R6",
                        *line,
                        format!("passive voice: `{} {} by`", window[0], window[1]),
                    );
                }
            }
        }
        // R7: steps (the first ordered list after a "Steps" heading, or the first ordered list).
        if front.kind == "task" {
            let mut after_steps = false;
            let mut steps = None;
            for block in &doc.blocks {
                match block {
                    Block::Heading { inlines, .. } => {
                        after_steps = markdown::text_of(inlines).eq_ignore_ascii_case("steps")
                    }
                    Block::List { ordered: true, .. } if after_steps || steps.is_none() => {
                        steps = Some(block);
                        if after_steps {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            match steps {
                Some(Block::List { items, line, .. }) => {
                    if items.len() > MAX_STEPS {
                        self.error(
                            "R7",
                            *line,
                            format!(
                                "{} steps; split the task (at most {MAX_STEPS})",
                                items.len()
                            ),
                        );
                    }
                    for (number, item) in items.iter().enumerate() {
                        let text: String = item
                            .iter()
                            .flat_map(|b| markdown::block_inlines(b))
                            .map(markdown::inline_text)
                            .collect();
                        let n = word_count(&text);
                        if n > MAX_STEP_WORDS {
                            self.error(
                                "R7",
                                *line + number,
                                format!(
                                    "step {} has {n} words (at most {MAX_STEP_WORDS})",
                                    number + 1
                                ),
                            );
                        }
                    }
                }
                _ => self.error("R7", 1, "a task needs numbered steps"),
            }
        }
        // R9, R12, R13: banned words; product terms.
        let mut checked: Vec<(String, usize)> = prose_pieces.clone();
        checked.push((front.title.clone(), 1));
        checked.push((front.summary.clone(), 1));
        for block in &blocks {
            if let Block::Heading { inlines, line, .. } = block {
                checked.push((prose(inlines), *line));
            }
        }
        for (text, line) in &checked {
            for word in self.context.banned_words.clone() {
                if phrase_found(text, &word) {
                    let rule = match word.as_str() {
                        "simply" | "just" | "easy" | "easily" | "obviously" | "of course" => "R12",
                        "click" | "clicking" | "tap" => "R9",
                        "click here" | "read more" => "R16",
                        _ => "R13",
                    };
                    self.error(
                        rule,
                        *line,
                        format!("don't use `{word}` (KNOWLEDGE-BASE §3.2)"),
                    );
                }
            }
            for (term, synonyms) in self.context.product_terms.clone() {
                for synonym in synonyms {
                    if phrase_found(text, &synonym) {
                        self.error("R13", *line, format!("say `{term}`, not `{synonym}`"));
                    }
                }
            }
        }
        // R10: explain jargon the first time (warning).
        let all_text: String = prose_pieces
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for term in self.context.explain.clone() {
            if let Some(at) = all_text.find(&term) {
                let after = all_text[at + term.len()..].trim_start();
                let linked = blocks.iter().flat_map(|b| markdown::block_inlines(b)).any(|i| {
                    matches!(i, Inline::Link { kind, target, .. } if kind == "kb" && target.starts_with("glossary."))
                });
                if !after.starts_with('(') && !linked && !front.title.contains(&term) {
                    self.warn(
                        "R10",
                        1,
                        format!("explain `{term}` the first time, or link to the glossary"),
                    );
                }
            }
        }
        // Inline checks: labels (R8), links (R16, KB-LINK), images (R17, KB-IMAGE).
        let mut labels = BTreeMap::new();
        let mut links = Vec::new();
        let mut images = Vec::new();
        for block in &blocks {
            let line = block.line();
            let mut stack: Vec<&Inline> = markdown::block_inlines(block);
            while let Some(inline) = stack.pop() {
                match inline {
                    Inline::Label { text } => {
                        labels.entry(text.clone()).or_insert(line);
                    }
                    Inline::Link {
                        kind,
                        target,
                        inlines,
                    } => {
                        links.push((
                            kind.clone(),
                            target.clone(),
                            markdown::text_of(inlines),
                            line,
                        ));
                        stack.extend(inlines.iter());
                    }
                    Inline::Image { src, alt } => images.push((src.clone(), alt.clone(), line)),
                    _ => {}
                }
            }
        }
        for (label, line) in labels {
            if !self.context.labels.contains(&label) {
                self.planned_or("label", &label, line, "R8", "UI label");
            }
        }
        for (kind, target, text, line) in links {
            let lower = text.trim().to_lowercase();
            if [
                "here",
                "click here",
                "read more",
                "more",
                "link",
                "this",
                "this page",
            ]
            .contains(&lower.as_str())
            {
                self.error(
                    "R16",
                    line,
                    format!("link text `{text}` doesn't say where it goes"),
                );
            }
            match kind.as_str() {
                "kb" if !self.known_ids.contains(&target) => {
                    self.planned_or("article", &target, line, "KB-LINK", "article")
                }
                "web" if !target.starts_with("https://") => self.error(
                    "KB-LINK",
                    line,
                    format!("`{target}` isn't a kb:, app:, tour: or https:// link"),
                ),
                _ => {}
            }
        }
        for (src, alt, line) in images {
            let alt_lower = alt.trim().to_lowercase();
            if alt_lower.is_empty()
                || ["screenshot", "image", "picture", "photo"].contains(&alt_lower.as_str())
            {
                self.error(
                    "R17",
                    line,
                    "describe what matters in the image in its alt text",
                );
            }
            match src.strip_prefix("media:") {
                Some(file)
                    if self
                        .context
                        .root
                        .join("content/kb/media")
                        .join(file)
                        .is_file() => {}
                _ => self.error(
                    "KB-IMAGE",
                    line,
                    format!("image `{src}` doesn't exist in content/kb/media"),
                ),
            }
        }
        // Front matter references.
        for id in front.related.clone() {
            if !self.known_ids.contains(&id) {
                self.planned_or("article", &id, 1, "KB-LINK", "related article");
            }
        }
        for anchor in front.context.clone() {
            if !self.context.anchors.contains(&anchor) {
                self.planned_or("anchor", &anchor, 1, "KB-ANCHOR", "help anchor");
            }
        }
        for setting in front.settings.clone() {
            if !self.context.settings.contains(&setting) {
                self.planned_or("setting", &setting, 1, "KB-SETTING", "setting");
            }
        }
        for code in front.error_codes.clone() {
            if !self.context.error_codes.contains(&code) {
                self.planned_or("error", &code, 1, "KB-ERROR", "error code");
            }
        }
        for collection in &front.collections {
            if !self.context.collections.contains_key(collection) {
                self.error("KB-SCHEMA", 1, format!("unknown collection `{collection}`"));
            }
        }
        // Review state.
        if let Some(reviewed) = days_from_date(&front.last_reviewed)
            && self.options.today - reviewed > 365
        {
            self.warn("KB-REVIEW", 1, "last reviewed more than 12 months ago");
        }
        if front.related.is_empty() && front.collections.is_empty() {
            self.warn("KB-ORPHAN", 1, "no related articles and no collection");
        }
        if self.options.release {
            if front.status == "draft" {
                self.error("KB-REVIEW", 1, "drafts can't ship in a release");
            }
            if front.reviewed_by.trim().is_empty() {
                self.error(
                    "KB-REVIEW",
                    1,
                    "`reviewed_by` is empty (2i review, KNOWLEDGE-BASE §7.3)",
                );
            }
        }
        self.findings
    }
}

/// Lint one article against the content context and the ids of every article.
pub fn lint_article(
    article: &Article,
    context: &Context,
    known_ids: &BTreeSet<String>,
    options: &Options,
) -> Vec<Finding> {
    Linter {
        article,
        context,
        options,
        known_ids,
        findings: Vec::new(),
    }
    .run()
}

fn article_qualifies(article: &Article, options: &Options) -> bool {
    if article.language != "en" {
        return false;
    }
    if options.release {
        article.front.status == "published"
    } else {
        article.front.status != "draft"
    }
}

/// Coverage checks 1–5 (KNOWLEDGE-BASE §7.2). Check 6 (UI labels) is R8 in [`lint_article`].
pub fn lint_coverage(context: &Context, articles: &[Article], options: &Options) -> Vec<Finding> {
    let mut findings = Vec::new();
    let active: Vec<&Article> = articles
        .iter()
        .filter(|a| article_qualifies(a, options))
        .collect();
    let active_ids: BTreeMap<&str, &Article> =
        active.iter().map(|a| (a.front.id.as_str(), *a)).collect();

    let planned_severity = |kind: &str, id: &str| -> Option<(Severity, String)> {
        let planned = context.planned(kind, id)?;
        let sev = if options.release || context.done.contains(&planned.milestone) {
            Severity::Error
        } else {
            Severity::Warning
        };
        Some((sev, planned.milestone.clone()))
    };

    // 1. Every anchor in anchors.yaml is used in the UI, and every anchor used in the UI is declared.
    if context.has_ui {
        for anchor in &context.anchors {
            if !context.ui_anchors.contains(anchor) {
                let line = context.anchor_lines.get(anchor).copied().unwrap_or(1);
                findings.push(Finding {
                    rule: "KB-COVER",
                    severity: Severity::Error,
                    path: "content/help/anchors.yaml".to_owned(),
                    line,
                    message: format!(
                        "anchor `{anchor}` is declared in anchors.yaml but not used in app/Selah.UI"
                    ),
                });
            }
        }
        for usage in &context.ui_anchor_uses {
            if !context.anchors.contains(&usage.anchor) {
                if let Some((sev, milestone)) = planned_severity("anchor", &usage.anchor) {
                    findings.push(Finding {
                        rule: "KB-PLANNED",
                        severity: sev,
                        path: usage.path.clone(),
                        line: usage.line,
                        message: format!(
                            "help anchor `{}` is planned for {milestone} and isn't active yet",
                            usage.anchor
                        ),
                    });
                } else {
                    findings.push(Finding {
                        rule: "KB-COVER",
                        severity: Severity::Error,
                        path: usage.path.clone(),
                        line: usage.line,
                        message: format!(
                            "help anchor `{}` used in the UI isn't declared in content/help/anchors.yaml",
                            usage.anchor
                        ),
                    });
                }
            }
        }
    }

    // 2. Every anchor has at least one active article listing it in `context`.
    let covered_anchors: BTreeSet<&str> = active
        .iter()
        .flat_map(|a| a.front.context.iter().map(String::as_str))
        .collect();
    for anchor in &context.anchors {
        if !covered_anchors.contains(anchor.as_str()) {
            let line = context.anchor_lines.get(anchor).copied().unwrap_or(1);
            if let Some((sev, milestone)) = planned_severity("anchor", anchor) {
                findings.push(Finding {
                    rule: "KB-PLANNED",
                    severity: sev,
                    path: "content/help/anchors.yaml".to_owned(),
                    line,
                    message: format!("anchor `{anchor}` is planned for {milestone}"),
                });
            } else {
                findings.push(Finding {
                    rule: "KB-COVER",
                    severity: Severity::Error,
                    path: "content/help/anchors.yaml".to_owned(),
                    line,
                    message: format!(
                        "anchor `{anchor}` has no active article listing it in `context`"
                    ),
                });
            }
        }
    }

    // 3. Every error code in errors.yaml has a valid UI string key and an active fix article.
    let mut covered_errors: BTreeMap<&str, Vec<&Article>> = BTreeMap::new();
    for article in &active {
        for code in &article.front.error_codes {
            covered_errors
                .entry(code.as_str())
                .or_default()
                .push(*article);
        }
    }
    for err in &context.error_defs {
        if !err.message.is_empty() && !context.label_keys.contains(&err.message) {
            findings.push(Finding {
                rule: "KB-COVER",
                severity: Severity::Error,
                path: "content/errors/errors.yaml".to_owned(),
                line: err.line,
                message: format!(
                    "error `{}` message key `{}` isn't in content/i18n/en/*.ftl",
                    err.code, err.message
                ),
            });
        }
        if err.fix_article.is_empty() {
            findings.push(Finding {
                rule: "KB-COVER",
                severity: Severity::Error,
                path: "content/errors/errors.yaml".to_owned(),
                line: err.line,
                message: format!("error `{}` has no `fix_article`", err.code),
            });
            continue;
        }
        if !active_ids.contains_key(err.fix_article.as_str()) {
            if let Some((sev, milestone)) = planned_severity("article", &err.fix_article)
                .or_else(|| planned_severity("error", &err.code))
            {
                findings.push(Finding {
                    rule: "KB-PLANNED",
                    severity: sev,
                    path: "content/errors/errors.yaml".to_owned(),
                    line: err.line,
                    message: format!(
                        "error `{}` fix_article `{}` is planned for {milestone}",
                        err.code, err.fix_article
                    ),
                });
            } else {
                findings.push(Finding {
                    rule: "KB-COVER",
                    severity: Severity::Error,
                    path: "content/errors/errors.yaml".to_owned(),
                    line: err.line,
                    message: format!(
                        "error `{}` references missing or inactive fix article `{}`",
                        err.code, err.fix_article
                    ),
                });
            }
        } else if !covered_errors.contains_key(err.code.as_str()) {
            findings.push(Finding {
                rule: "KB-COVER",
                severity: Severity::Error,
                path: "content/errors/errors.yaml".to_owned(),
                line: err.line,
                message: format!(
                    "error `{}` isn't listed in any active article's `error_codes`",
                    err.code
                ),
            });
        }
    }

    // 4. Every setting has a hint string and an active article listing it in `settings`.
    let covered_settings: BTreeSet<&str> = active
        .iter()
        .flat_map(|a| a.front.settings.iter().map(String::as_str))
        .collect();
    for setting in &context.setting_defs {
        let valid_hint = !setting.hint.is_empty()
            && (setting.hint.ends_with("-hint") || setting.hint.ends_with(".hint"))
            && context.label_keys.contains(&setting.hint);
        if !valid_hint {
            findings.push(Finding {
                rule: "KB-COVER",
                severity: Severity::Error,
                path: "content/help/settings.yaml".to_owned(),
                line: setting.line,
                message: format!(
                    "setting `{}` hint string `{}` isn't a `-hint` key in content/i18n/en/*.ftl",
                    setting.key, setting.hint
                ),
            });
        }
        if !covered_settings.contains(setting.key.as_str()) {
            if let Some((sev, milestone)) = planned_severity("setting", &setting.key) {
                findings.push(Finding {
                    rule: "KB-PLANNED",
                    severity: sev,
                    path: "content/help/settings.yaml".to_owned(),
                    line: setting.line,
                    message: format!("setting `{}` is planned for {milestone}", setting.key),
                });
            } else {
                findings.push(Finding {
                    rule: "KB-COVER",
                    severity: Severity::Error,
                    path: "content/help/settings.yaml".to_owned(),
                    line: setting.line,
                    message: format!(
                        "setting `{}` has no active article listing it in `settings`",
                        setting.key
                    ),
                });
            }
        }
    }

    // 5. Every tour step points to a real anchor.
    for step in &context.tour_steps {
        if step.anchor.is_empty() {
            findings.push(Finding {
                rule: "KB-COVER",
                severity: Severity::Error,
                path: step.path.clone(),
                line: step.line,
                message: "tour step has no `anchor`".to_owned(),
            });
        } else if !context.anchors.contains(&step.anchor) {
            if let Some((sev, milestone)) = planned_severity("anchor", &step.anchor) {
                findings.push(Finding {
                    rule: "KB-PLANNED",
                    severity: sev,
                    path: step.path.clone(),
                    line: step.line,
                    message: format!(
                        "tour step anchor `{}` is planned for {milestone}",
                        step.anchor
                    ),
                });
            } else {
                findings.push(Finding {
                    rule: "KB-COVER",
                    severity: Severity::Error,
                    path: step.path.clone(),
                    line: step.line,
                    message: format!("tour step references unknown anchor `{}`", step.anchor),
                });
            }
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_convert_to_days() {
        assert_eq!(days_from_date("1970-01-01"), Some(0));
        assert_eq!(
            days_from_date("2026-11-07").unwrap() - days_from_date("2025-11-07").unwrap(),
            365
        );
    }

    #[test]
    fn phrases_match_whole_words() {
        assert!(phrase_found("Just select it", "just"));
        assert!(!phrase_found("Adjust the size", "just"));
        assert!(phrase_found("Songs, e.g. hymns", "e.g."));
    }
}
