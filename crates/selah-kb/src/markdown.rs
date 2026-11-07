//! The article Markdown subset (KNOWLEDGE-BASE §6.3) parsed into the AST the UI renders with
//! native controls. Raw HTML is kept only as a marker so the lint can refuse it.
use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inline {
    Text {
        text: String,
    },
    /// `**Next slide**`: a UI label (bold is only for UI labels).
    Label {
        text: String,
    },
    /// `[[Space]]`: a keyboard key.
    Key {
        text: String,
    },
    Emphasis {
        text: String,
    },
    Code {
        text: String,
    },
    /// `kb:`, `app:`, `tour:` or a web link.
    Link {
        kind: String,
        target: String,
        inlines: Vec<Inline>,
    },
    Image {
        src: String,
        alt: String,
    },
    Break,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
        #[serde(skip)]
        line: usize,
    },
    Paragraph {
        inlines: Vec<Inline>,
        #[serde(skip)]
        line: usize,
    },
    List {
        ordered: bool,
        items: Vec<Vec<Block>>,
        /// `- [ ]` checkboxes (checklists).
        tasks: bool,
        #[serde(skip)]
        line: usize,
    },
    /// `> [!TIP]`, `> [!NOTE]`, `> [!WARNING]`.
    Callout {
        kind: String,
        blocks: Vec<Block>,
        #[serde(skip)]
        line: usize,
    },
    Quote {
        blocks: Vec<Block>,
        #[serde(skip)]
        line: usize,
    },
    Table {
        header: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
        #[serde(skip)]
        line: usize,
    },
    Code {
        text: String,
        #[serde(skip)]
        line: usize,
    },
    /// Raw HTML, which isn't allowed (lint KB-HTML); never rendered.
    Html {
        #[serde(skip)]
        line: usize,
    },
    Rule,
}

impl Block {
    pub fn line(&self) -> usize {
        match self {
            Block::Heading { line, .. }
            | Block::Paragraph { line, .. }
            | Block::List { line, .. }
            | Block::Callout { line, .. }
            | Block::Quote { line, .. }
            | Block::Table { line, .. }
            | Block::Code { line, .. }
            | Block::Html { line } => *line,
            Block::Rule => 0,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Document {
    pub blocks: Vec<Block>,
}

struct Cursor<'a> {
    events: Vec<(Event<'a>, usize)>,
    at: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<&Event<'a>> {
        self.events.get(self.at).map(|(e, _)| e)
    }
    fn line(&self) -> usize {
        self.events.get(self.at).map_or(0, |(_, l)| *l)
    }
    fn next(&mut self) -> Option<Event<'a>> {
        let event = self.events.get(self.at).map(|(e, _)| e.clone());
        self.at += 1;
        event
    }
}

fn is_inline_start(event: &Event<'_>) -> bool {
    match event {
        Event::Text(_)
        | Event::Code(_)
        | Event::SoftBreak
        | Event::HardBreak
        | Event::InlineHtml(_) => true,
        Event::TaskListMarker(_) => true,
        Event::Start(tag) => matches!(
            tag,
            Tag::Strong | Tag::Emphasis | Tag::Link { .. } | Tag::Image { .. } | Tag::Strikethrough
        ),
        _ => false,
    }
}

fn flatten(inlines: &[Inline]) -> String {
    inlines.iter().map(inline_text).collect()
}

/// The words a reader sees in an inline.
pub fn inline_text(inline: &Inline) -> String {
    match inline {
        Inline::Text { text }
        | Inline::Label { text }
        | Inline::Key { text }
        | Inline::Emphasis { text }
        | Inline::Code { text } => text.clone(),
        Inline::Link { inlines, .. } => flatten(inlines),
        Inline::Image { alt, .. } => alt.clone(),
        Inline::Break => " ".into(),
    }
}

/// Split `[[Key]]` markers out of a text run.
fn push_text(out: &mut Vec<Inline>, text: &str) {
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let Some(len) = rest[start + 2..].find("]]") else {
            break;
        };
        if start > 0 {
            out.push(Inline::Text {
                text: rest[..start].into(),
            });
        }
        out.push(Inline::Key {
            text: rest[start + 2..start + 2 + len].into(),
        });
        rest = &rest[start + 2 + len + 2..];
    }
    if !rest.is_empty() {
        out.push(Inline::Text { text: rest.into() });
    }
}

fn merge_texts(inlines: Vec<Inline>) -> Vec<Inline> {
    // pulldown-cmark splits `[[Space]]` into several text events; join runs, then find keys.
    let mut out = Vec::new();
    let mut run = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text } => run.push_str(&text),
            other => {
                if !run.is_empty() {
                    push_text(&mut out, &std::mem::take(&mut run));
                }
                out.push(other);
            }
        }
    }
    if !run.is_empty() {
        push_text(&mut out, &run);
    }
    out
}

fn link_kind(target: &str) -> (String, String) {
    for kind in ["kb", "app", "tour", "media"] {
        if let Some(rest) = target.strip_prefix(&format!("{kind}:")) {
            return (kind.into(), rest.into());
        }
    }
    ("web".into(), target.into())
}

fn inlines_until(cursor: &mut Cursor<'_>, stop: fn(&Event<'_>) -> bool) -> (Vec<Inline>, bool) {
    let mut out = Vec::new();
    let mut html = false;
    while let Some(event) = cursor.peek() {
        if stop(event) {
            break;
        }
        match cursor.next().expect("peeked") {
            Event::Text(text) => out.push(Inline::Text {
                text: text.to_string(),
            }),
            Event::Code(text) => out.push(Inline::Code {
                text: text.to_string(),
            }),
            Event::SoftBreak => out.push(Inline::Text { text: " ".into() }),
            Event::HardBreak => out.push(Inline::Break),
            Event::InlineHtml(_) | Event::Html(_) => html = true,
            Event::TaskListMarker(_) => {}
            Event::Start(Tag::Strong) => {
                let (inner, h) = inlines_until(cursor, |e| matches!(e, Event::End(TagEnd::Strong)));
                cursor.next();
                html |= h;
                out.push(Inline::Label {
                    text: flatten(&inner),
                });
            }
            Event::Start(Tag::Emphasis) => {
                let (inner, h) =
                    inlines_until(cursor, |e| matches!(e, Event::End(TagEnd::Emphasis)));
                cursor.next();
                html |= h;
                out.push(Inline::Emphasis {
                    text: flatten(&inner),
                });
            }
            Event::Start(Tag::Strikethrough) => {
                let (inner, h) =
                    inlines_until(cursor, |e| matches!(e, Event::End(TagEnd::Strikethrough)));
                cursor.next();
                html |= h;
                out.extend(inner);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                let (inner, h) = inlines_until(cursor, |e| matches!(e, Event::End(TagEnd::Link)));
                cursor.next();
                html |= h;
                let (kind, target) = link_kind(&dest_url);
                out.push(Inline::Link {
                    kind,
                    target,
                    inlines: merge_texts(inner),
                });
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                let (inner, h) = inlines_until(cursor, |e| matches!(e, Event::End(TagEnd::Image)));
                cursor.next();
                html |= h;
                out.push(Inline::Image {
                    src: dest_url.to_string(),
                    alt: flatten(&inner),
                });
            }
            _ => {}
        }
    }
    (merge_texts(out), html)
}

fn block_end(event: &Event<'_>) -> bool {
    matches!(event, Event::End(_)) || (matches!(event, Event::Start(_)) && !is_inline_start(event))
}

fn blocks_until(cursor: &mut Cursor<'_>, end: fn(&Event<'_>) -> bool) -> Vec<Block> {
    let mut blocks = Vec::new();
    while let Some(event) = cursor.peek() {
        if end(event) {
            break;
        }
        let line = cursor.line();
        if is_inline_start(event) {
            // Tight list items hold inlines directly; treat them as a paragraph.
            let (inlines, html) = inlines_until(cursor, block_end);
            if html {
                blocks.push(Block::Html { line });
            }
            if !inlines.is_empty() {
                blocks.push(Block::Paragraph { inlines, line });
            }
            continue;
        }
        match cursor.next().expect("peeked") {
            Event::Start(Tag::Heading { level, .. }) => {
                let (inlines, html) =
                    inlines_until(cursor, |e| matches!(e, Event::End(TagEnd::Heading(_))));
                cursor.next();
                if html {
                    blocks.push(Block::Html { line });
                }
                let level = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                blocks.push(Block::Heading {
                    level,
                    inlines,
                    line,
                });
            }
            Event::Start(Tag::Paragraph) => {
                let (inlines, html) =
                    inlines_until(cursor, |e| matches!(e, Event::End(TagEnd::Paragraph)));
                cursor.next();
                if html {
                    blocks.push(Block::Html { line });
                }
                blocks.push(Block::Paragraph { inlines, line });
            }
            Event::Start(Tag::List(first)) => {
                let mut items = Vec::new();
                let mut tasks = false;
                while let Some(event) = cursor.peek() {
                    match event {
                        Event::Start(Tag::Item) => {
                            cursor.next();
                            if matches!(cursor.peek(), Some(Event::TaskListMarker(_))) {
                                tasks = true;
                            }
                            items.push(blocks_until(cursor, |e| {
                                matches!(e, Event::End(TagEnd::Item))
                            }));
                            cursor.next();
                        }
                        Event::End(TagEnd::List(_)) => {
                            cursor.next();
                            break;
                        }
                        _ => {
                            cursor.next();
                        }
                    }
                }
                blocks.push(Block::List {
                    ordered: first.is_some(),
                    items,
                    tasks,
                    line,
                });
            }
            Event::Start(Tag::BlockQuote(kind)) => {
                let inner =
                    blocks_until(cursor, |e| matches!(e, Event::End(TagEnd::BlockQuote(_))));
                cursor.next();
                let kind = match kind {
                    Some(BlockQuoteKind::Tip) => Some("tip"),
                    Some(BlockQuoteKind::Note) => Some("note"),
                    Some(BlockQuoteKind::Warning) => Some("warning"),
                    Some(BlockQuoteKind::Important) => Some("important"),
                    Some(BlockQuoteKind::Caution) => Some("caution"),
                    None => None,
                };
                blocks.push(match kind {
                    Some(kind) => Block::Callout {
                        kind: kind.into(),
                        blocks: inner,
                        line,
                    },
                    None => Block::Quote {
                        blocks: inner,
                        line,
                    },
                });
            }
            Event::Start(Tag::Table(_)) => {
                let mut header = Vec::new();
                let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
                let mut in_head = false;
                while let Some(event) = cursor.next() {
                    match event {
                        Event::Start(Tag::TableHead) => in_head = true,
                        Event::End(TagEnd::TableHead) => in_head = false,
                        Event::Start(Tag::TableRow) => rows.push(Vec::new()),
                        Event::Start(Tag::TableCell) => {
                            let (cell, _) = inlines_until(cursor, |e| {
                                matches!(e, Event::End(TagEnd::TableCell))
                            });
                            cursor.next();
                            if in_head {
                                header.push(cell);
                            } else if let Some(row) = rows.last_mut() {
                                row.push(cell);
                            }
                        }
                        Event::End(TagEnd::Table) => break,
                        _ => {}
                    }
                }
                blocks.push(Block::Table { header, rows, line });
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let _ = matches!(kind, CodeBlockKind::Fenced(_));
                let mut text = String::new();
                while let Some(event) = cursor.next() {
                    match event {
                        Event::Text(t) => text.push_str(&t),
                        Event::End(TagEnd::CodeBlock) => break,
                        _ => {}
                    }
                }
                blocks.push(Block::Code { text, line });
            }
            Event::Html(_) => blocks.push(Block::Html { line }),
            Event::Start(Tag::HtmlBlock) => {
                while let Some(event) = cursor.next() {
                    if matches!(event, Event::End(TagEnd::HtmlBlock)) {
                        break;
                    }
                }
                blocks.push(Block::Html { line });
            }
            Event::Rule => blocks.push(Block::Rule),
            _ => {}
        }
    }
    blocks
}

/// Parse an article body. `first_line` is the file line where the body starts.
pub fn parse(body: &str, first_line: usize) -> Document {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_GFM
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH;
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(body.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let line_of = |offset: usize| line_starts.partition_point(|&s| s <= offset) + first_line - 1;
    let events = Parser::new_ext(body, options)
        .into_offset_iter()
        .map(|(event, range)| (event, line_of(range.start)))
        .collect();
    let mut cursor = Cursor { events, at: 0 };
    Document {
        blocks: blocks_until(&mut cursor, |_| false),
    }
}

/// Every block, depth first (list items and callouts included).
pub fn walk(blocks: &[Block]) -> Vec<&Block> {
    let mut out = Vec::new();
    for block in blocks {
        out.push(block);
        match block {
            Block::List { items, .. } => {
                for item in items {
                    out.extend(walk(item));
                }
            }
            Block::Callout { blocks, .. } | Block::Quote { blocks, .. } => out.extend(walk(blocks)),
            _ => {}
        }
    }
    out
}

/// Every inline in a block (not descending into child blocks).
pub fn block_inlines(block: &Block) -> Vec<&Inline> {
    match block {
        Block::Heading { inlines, .. } | Block::Paragraph { inlines, .. } => {
            inlines.iter().collect()
        }
        Block::Table { header, rows, .. } => header
            .iter()
            .flatten()
            .chain(rows.iter().flatten().flatten())
            .collect(),
        _ => Vec::new(),
    }
}

/// The plain text of some inlines.
pub fn text_of(inlines: &[Inline]) -> String {
    flatten(inlines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_keys_links_and_callouts() {
        let doc = parse(
            "# Title\n\nPress [[Space]] or select **Next slide**. See [Clear](kb:live.clear-the-screens).\n\n> [!TIP]\n> Look first.\n\n1. One\n2. Two\n",
            10,
        );
        let Block::Paragraph { inlines, line } = &doc.blocks[1] else {
            panic!("{:?}", doc.blocks)
        };
        assert_eq!(*line, 12);
        assert!(inlines.contains(&Inline::Key {
            text: "Space".into()
        }));
        assert!(inlines.contains(&Inline::Label {
            text: "Next slide".into()
        }));
        assert!(inlines.iter().any(|i| matches!(i, Inline::Link { kind, target, .. } if kind == "kb" && target == "live.clear-the-screens")));
        assert!(matches!(&doc.blocks[2], Block::Callout { kind, .. } if kind == "tip"));
        assert!(
            matches!(&doc.blocks[3], Block::List { ordered: true, items, .. } if items.len() == 2)
        );
    }

    #[test]
    fn html_is_marked() {
        let doc = parse("Hello <b>there</b>\n\n<div>x</div>\n", 1);
        assert!(
            walk(&doc.blocks)
                .iter()
                .any(|b| matches!(b, Block::Html { .. }))
        );
    }

    #[test]
    fn ast_json_has_no_line_numbers() {
        let doc = parse("# Title\n", 1);
        let json = serde_json::to_string(&doc).unwrap();
        assert_eq!(
            json,
            r#"{"blocks":[{"type":"heading","level":1,"inlines":[{"type":"text","text":"Title"}]}]}"#
        );
    }
}
