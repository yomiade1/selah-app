//! Everything an article can refer to: terms, anchors, settings, error codes, planned items,
//! collections, UI strings and finished milestones (KNOWLEDGE-BASE §6.1, §7.1).
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use yaml_rust2::{Yaml, YamlLoader};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub id: String,
    pub kind: String,
    pub milestone: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingDef {
    pub key: String,
    pub hint: String,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorDef {
    pub code: String,
    pub message: String,
    pub fix_article: String,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiAnchorUse {
    pub anchor: String,
    pub path: String,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TourStepUse {
    pub anchor: String,
    pub path: String,
    pub line: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Context {
    pub root: PathBuf,
    pub banned_words: Vec<String>,
    /// (term, banned synonyms)
    pub product_terms: Vec<(String, Vec<String>)>,
    pub readability_ignore: Vec<String>,
    pub proper_nouns: Vec<String>,
    pub explain: Vec<String>,
    pub anchors: BTreeSet<String>,
    pub anchor_lines: BTreeMap<String, usize>,
    pub settings: BTreeSet<String>,
    pub setting_defs: Vec<SettingDef>,
    pub error_codes: BTreeSet<String>,
    pub error_defs: Vec<ErrorDef>,
    pub collections: BTreeMap<String, String>,
    pub planned: Vec<Planned>,
    /// Every English UI string's text (R8).
    pub labels: BTreeSet<String>,
    /// Every English UI string id (`id = ...`).
    pub label_keys: BTreeSet<String>,
    /// Milestones marked Done in PROGRESS.md (planned references to them are errors).
    pub done: BTreeSet<String>,
    /// Whether `app/Selah.UI` is present under `root`.
    pub has_ui: bool,
    /// Anchors discovered in `app/Selah.UI`.
    pub ui_anchors: BTreeSet<String>,
    /// Every anchor usage location in `app/Selah.UI`.
    pub ui_anchor_uses: Vec<UiAnchorUse>,
    /// Tour step anchor references in `content/help/tours/*.yaml`.
    pub tour_steps: Vec<TourStepUse>,
}

fn load_yaml_with_text(path: &Path) -> anyhow::Result<(Yaml, String)> {
    let text =
        std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let mut docs =
        YamlLoader::load_from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    Ok((docs.pop().unwrap_or(Yaml::Null), text))
}

fn load_yaml(path: &Path) -> anyhow::Result<Yaml> {
    Ok(load_yaml_with_text(path)?.0)
}

fn find_key_value_line(text: &str, key: &str, value: &str, from_line: usize) -> usize {
    let needle = format!("{key}: {value}");
    let quoted_needle = format!("{key}: \"{value}\"");
    for (idx, line) in text.lines().enumerate().skip(from_line) {
        let trimmed = line.trim().trim_start_matches('-').trim();
        if trimmed == needle || trimmed == quoted_needle || trimmed.starts_with(&needle) {
            return idx + 1;
        }
    }
    1
}

/// Convert `shell.show-test-pattern` to `ShellShowTestPattern` (`gen_help_anchors.py` convention).
pub fn anchor_pascal_name(anchor_id: &str) -> String {
    let mut out = String::new();
    for part in anchor_id.split(['.', '-', '_']) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// Message keys and texts from Fluent files: `id = value`.
pub fn ftl_entries(text: &str) -> (Vec<String>, Vec<String>) {
    let mut keys = Vec::new();
    let mut texts = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        if let Some((key, value)) = line.split_once(" = ") {
            let trimmed_key = key.trim();
            if !trimmed_key.is_empty() {
                keys.push(trimmed_key.to_owned());
            }
            texts.push(value.trim().to_owned());
        }
    }
    (keys, texts)
}

/// Message texts from Fluent files: the value after `id =`, with placeables kept as written.
pub fn ftl_texts(text: &str) -> Vec<String> {
    ftl_entries(text).1
}

fn extract_quoted_values(line: &str, prefix: &str, require_prefix_boundary: bool) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut start = 0;
    while let Some(pos) = line[start..].find(prefix) {
        let abs = start + pos;
        if require_prefix_boundary && abs > 0 {
            let prev = bytes[abs - 1] as char;
            if prev.is_alphanumeric() || prev == '.' || prev == '_' || prev == ':' {
                start = abs + prefix.len();
                continue;
            }
        }
        let val_start = abs + prefix.len();
        if let Some(end_rel) = line[val_start..].find('"') {
            let val = &line[val_start..val_start + end_rel];
            if !val.trim().is_empty() {
                out.push(val.trim().to_owned());
            }
            start = val_start + end_rel + 1;
        } else {
            break;
        }
    }
    out
}

fn scan_ui_anchors(
    root: &Path,
    pascal_map: &BTreeMap<String, String>,
) -> anyhow::Result<(BTreeSet<String>, Vec<UiAnchorUse>)> {
    let ui_root = root.join("app/Selah.UI");
    let mut ui_anchors = BTreeSet::new();
    let mut uses = Vec::new();
    let mut stack = vec![ui_root];
    let mut files = Vec::new();

    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if name != "bin" && name != "obj" {
                    stack.push(path);
                }
                continue;
            }
            if name == "HelpAnchors.g.cs" || name.ends_with(".g.axaml") {
                continue;
            }
            if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e == "axaml" || e == "cs")
            {
                files.push(path);
            }
        }
    }
    files.sort();

    for path in files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&path)?;
        let is_axaml = path.extension().and_then(|e| e.to_str()) == Some("axaml");

        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if is_axaml {
                if trimmed.starts_with("<!--") {
                    continue;
                }
                for val in extract_quoted_values(line, "Help.Anchor=\"", false)
                    .into_iter()
                    .chain(extract_quoted_values(line, "HelpAnchor=\"", false))
                    .chain(extract_quoted_values(line, "Anchor=\"", true))
                {
                    if !val.starts_with('{') {
                        ui_anchors.insert(val.clone());
                        uses.push(UiAnchorUse {
                            anchor: val,
                            path: rel.clone(),
                            line: idx + 1,
                        });
                    }
                }
            } else {
                if trimmed.starts_with("//") {
                    continue;
                }
                let mut from = 0;
                while let Some(pos) = line[from..].find("HelpAnchors.") {
                    let ident_start = from + pos + "HelpAnchors.".len();
                    let ident_len = line[ident_start..]
                        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .unwrap_or(line.len() - ident_start);
                    if ident_len > 0 {
                        let ident = &line[ident_start..ident_start + ident_len];
                        let anchor = pascal_map
                            .get(ident)
                            .cloned()
                            .unwrap_or_else(|| format!("HelpAnchors.{ident}"));
                        ui_anchors.insert(anchor.clone());
                        uses.push(UiAnchorUse {
                            anchor,
                            path: rel.clone(),
                            line: idx + 1,
                        });
                    }
                    from = ident_start + ident_len.max(1);
                }
                if let Some(pos) = line.find("SetAnchor(")
                    && let Some(first_quote) = line[pos..].find('"')
                {
                    let q_start = pos + first_quote + 1;
                    if let Some(q_end) = line[q_start..].find('"') {
                        let anchor = line[q_start..q_start + q_end].trim().to_owned();
                        if !anchor.is_empty() {
                            ui_anchors.insert(anchor.clone());
                            uses.push(UiAnchorUse {
                                anchor,
                                path: rel.clone(),
                                line: idx + 1,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok((ui_anchors, uses))
}

fn scan_tours(root: &Path) -> anyhow::Result<Vec<TourStepUse>> {
    let tours_dir = root.join("content/help/tours");
    if !tours_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&tours_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e == "yaml" || e == "yml")
        })
        .collect();
    paths.sort();

    let mut out = Vec::new();
    for path in paths {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let (doc, text) = load_yaml_with_text(&path)?;
        let mut cursor = 0usize;
        for step in doc["steps"].as_vec().into_iter().flatten() {
            let anchor = field(step, "anchor");
            let line = if anchor.is_empty() {
                1
            } else {
                let found = find_key_value_line(&text, "anchor", &anchor, cursor);
                cursor = found;
                found
            };
            out.push(TourStepUse {
                anchor,
                path: rel.clone(),
                line,
            });
        }
    }
    Ok(out)
}

fn strings(value: &Yaml) -> Vec<String> {
    value
        .as_vec()
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn field(map: &Yaml, key: &str) -> String {
    map[key].as_str().unwrap_or_default().to_owned()
}

/// Milestones whose status in the PROGRESS.md table starts with "Done".
pub fn done_milestones(progress: &str) -> BTreeSet<String> {
    progress
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let name = cells.get(1)?;
            let status = cells.get(2)?;
            let id = name.split('.').next()?;
            (id.starts_with('M')
                && id[1..].chars().all(|c| c.is_ascii_digit())
                && status.starts_with("Done"))
            .then(|| id.to_owned())
        })
        .collect()
}

impl Context {
    /// Load from a repository root (`content/`, `PROGRESS.md`).
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let content = root.join("content");
        let terms = load_yaml(&content.join("terms.yaml"))?;
        let mut context = Context {
            root: root.to_owned(),
            banned_words: strings(&terms["banned_words"]),
            readability_ignore: strings(&terms["readability_ignore"]),
            proper_nouns: strings(&terms["proper_nouns"]),
            explain: strings(&terms["explain"]),
            ..Context::default()
        };
        if let Some(map) = terms["product_terms"].as_hash() {
            for (term, synonyms) in map {
                if let Some(term) = term.as_str() {
                    context
                        .product_terms
                        .push((term.to_owned(), strings(synonyms)));
                }
            }
        }
        let (anchors, anchors_text) = load_yaml_with_text(&content.join("help/anchors.yaml"))?;
        let mut cursor = 0usize;
        for anchor in anchors["anchors"].as_vec().into_iter().flatten() {
            let id = field(anchor, "id");
            let line = find_key_value_line(&anchors_text, "id", &id, cursor);
            cursor = line;
            context.anchor_lines.insert(id.clone(), line);
            context.anchors.insert(id);
        }
        let (settings, settings_text) = load_yaml_with_text(&content.join("help/settings.yaml"))?;
        let mut cursor = 0usize;
        for setting in settings["settings"].as_vec().into_iter().flatten() {
            let key = field(setting, "key");
            let hint = field(setting, "hint");
            let line = find_key_value_line(&settings_text, "key", &key, cursor);
            cursor = line;
            context.settings.insert(key.clone());
            context.setting_defs.push(SettingDef { key, hint, line });
        }
        let (errors, errors_text) = load_yaml_with_text(&content.join("errors/errors.yaml"))?;
        let mut cursor = 0usize;
        for error in errors["errors"].as_vec().into_iter().flatten() {
            let code = field(error, "code");
            let message = field(error, "message");
            let fix_article = field(error, "fix_article");
            let line = find_key_value_line(&errors_text, "code", &code, cursor);
            cursor = line;
            context.error_codes.insert(code.clone());
            context.error_defs.push(ErrorDef {
                code,
                message,
                fix_article,
                line,
            });
        }
        let collections = load_yaml(&content.join("kb/en/collections.yaml"))?;
        for collection in collections["collections"].as_vec().into_iter().flatten() {
            context
                .collections
                .insert(field(collection, "id"), field(collection, "title"));
        }
        let planned = load_yaml(&content.join("kb/planned.yaml"))?;
        for item in planned["planned"].as_vec().into_iter().flatten() {
            context.planned.push(Planned {
                id: field(item, "id"),
                kind: field(item, "kind"),
                milestone: field(item, "milestone"),
            });
        }
        let strings_dir = content.join("i18n/en");
        if let Ok(entries) = std::fs::read_dir(&strings_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|e| e == "ftl") {
                    let text = std::fs::read_to_string(entry.path())?;
                    let (keys, texts) = ftl_entries(&text);
                    context.label_keys.extend(keys);
                    context.labels.extend(texts);
                }
            }
        }
        if let Ok(progress) = std::fs::read_to_string(root.join("PROGRESS.md")) {
            context.done = done_milestones(&progress);
        }
        if root.join("app/Selah.UI").is_dir() {
            context.has_ui = true;
            let mut pascal_map = BTreeMap::new();
            for id in &context.anchors {
                pascal_map.insert(anchor_pascal_name(id), id.clone());
            }
            for item in &context.planned {
                if item.kind == "anchor" {
                    pascal_map.insert(anchor_pascal_name(&item.id), item.id.clone());
                }
            }
            let (ui_anchors, ui_anchor_uses) = scan_ui_anchors(root, &pascal_map)?;
            context.ui_anchors = ui_anchors;
            context.ui_anchor_uses = ui_anchor_uses;
        }
        context.tour_steps = scan_tours(root)?;
        Ok(context)
    }

    pub fn planned(&self, kind: &str, id: &str) -> Option<&Planned> {
        self.planned.iter().find(|p| p.kind == kind && p.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_milestones_come_from_the_status_table() {
        let progress = "| Milestone | Status |\n|---|---|\n| M1. Foundations | Done, H-checks pending | x |\n| M2. Library | In progress |\n| M3. Render | Done |\n";
        let done = done_milestones(progress);
        assert!(done.contains("M1") && done.contains("M3") && !done.contains("M2"));
    }

    #[test]
    fn ftl_texts_skip_comments_and_ids() {
        let texts = ftl_texts("# Comment\napp-name = Selah\nstatus-connected = Connected\n");
        assert_eq!(texts, ["Selah", "Connected"]);
    }
}
