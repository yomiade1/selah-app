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
    pub settings: BTreeSet<String>,
    pub error_codes: BTreeSet<String>,
    pub collections: BTreeMap<String, String>,
    pub planned: Vec<Planned>,
    /// Every English UI string's text (R8).
    pub labels: BTreeSet<String>,
    /// Milestones marked Done in PROGRESS.md (planned references to them are errors).
    pub done: BTreeSet<String>,
}

fn load_yaml(path: &Path) -> anyhow::Result<Yaml> {
    let text =
        std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let mut docs =
        YamlLoader::load_from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    Ok(docs.pop().unwrap_or(Yaml::Null))
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

/// Message texts from Fluent files: the value after `id =`, with placeables kept as written.
pub fn ftl_texts(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with(' '))
        .filter_map(|line| {
            line.split_once(" = ")
                .map(|(_, value)| value.trim().to_owned())
        })
        .collect()
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
        let anchors = load_yaml(&content.join("help/anchors.yaml"))?;
        for anchor in anchors["anchors"].as_vec().into_iter().flatten() {
            context.anchors.insert(field(anchor, "id"));
        }
        let settings = load_yaml(&content.join("help/settings.yaml"))?;
        for setting in settings["settings"].as_vec().into_iter().flatten() {
            context.settings.insert(field(setting, "key"));
        }
        let errors = load_yaml(&content.join("errors/errors.yaml"))?;
        for error in errors["errors"].as_vec().into_iter().flatten() {
            context.error_codes.insert(field(error, "code"));
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
                    context.labels.extend(ftl_texts(&text));
                }
            }
        }
        if let Ok(progress) = std::fs::read_to_string(root.join("PROGRESS.md")) {
            context.done = done_milestones(&progress);
        }
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
