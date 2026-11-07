//! Article front matter (KNOWLEDGE-BASE §6.2).
use yaml_rust2::{Yaml, YamlLoader};

pub const TYPES: &[&str] = &[
    "task",
    "concept",
    "troubleshooting",
    "reference",
    "tip",
    "checklist",
    "glossary",
];
pub const STATUSES: &[&str] = &["draft", "review", "published"];
pub const AUDIENCES: &[&str] = &["volunteer", "producer", "admin", "it"];
pub const MODES: &[&str] = &["simple", "producer"];
pub const PLATFORMS: &[&str] = &["windows", "macos"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrontMatter {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub audience: Vec<String>,
    pub modes: Vec<String>,
    pub context: Vec<String>,
    pub settings: Vec<String>,
    pub error_codes: Vec<String>,
    pub summary: String,
    pub popover: String,
    pub user_need: String,
    pub keywords: Vec<String>,
    pub related: Vec<String>,
    pub collections: Vec<String>,
    pub applies_to: String,
    pub platforms: Vec<String>,
    pub last_reviewed: String,
    pub reviewed_by: String,
    pub translation_of: String,
    pub source_hash: String,
}

/// A source file split into front matter and body. `body_line` is the 1-based line where the
/// body starts, so findings in the body can point at the right line.
pub struct Split<'a> {
    pub front: &'a str,
    pub body: &'a str,
    pub body_line: usize,
}

pub fn split(source: &str) -> Result<Split<'_>, String> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let rest = source
        .strip_prefix("---\n")
        .or_else(|| source.strip_prefix("---\r\n"))
        .ok_or("the file must start with front matter between --- lines")?;
    let end = rest
        .find("\n---\n")
        .or_else(|| rest.find("\n---\r\n"))
        .or_else(|| rest.strip_suffix("\n---").map(|r| r.len()))
        .ok_or("the front matter has no closing --- line")?;
    let front = &rest[..end];
    let after = &rest[end + 1..];
    let body = after
        .strip_prefix("---\r\n")
        .or_else(|| after.strip_prefix("---\n"))
        .or_else(|| after.strip_prefix("---"))
        .unwrap_or(after);
    let body_line = front.lines().count() + 3;
    Ok(Split {
        front,
        body,
        body_line,
    })
}

fn text(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        Yaml::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

fn list(value: &Yaml) -> Option<Vec<String>> {
    match value {
        Yaml::Array(items) => items.iter().map(text).collect(),
        _ => None,
    }
}

/// Parse front matter. Errors name the field (lint rule KB-SCHEMA).
pub fn parse(front: &str) -> Result<FrontMatter, Vec<String>> {
    let docs = YamlLoader::load_from_str(front).map_err(|e| vec![format!("invalid YAML: {e}")])?;
    let Some(Yaml::Hash(map)) = docs.into_iter().next() else {
        return Err(vec!["front matter must be a YAML map".into()]);
    };
    let mut errors = Vec::new();
    let get = |key: &str| map.get(&Yaml::String(key.into()));
    let mut string = |key: &str, required: bool| -> String {
        match get(key) {
            Some(value) => text(value).unwrap_or_else(|| {
                errors.push(format!("`{key}` must be text"));
                String::new()
            }),
            None if required => {
                errors.push(format!("`{key}` is required"));
                String::new()
            }
            None => String::new(),
        }
    };
    let id = string("id", true);
    let title = string("title", true);
    let kind = string("type", true);
    let status = string("status", true);
    let summary = string("summary", true);
    let popover = string("popover", false);
    let user_need = string("user_need", false);
    let applies_to = string("applies_to", true);
    let last_reviewed = string("last_reviewed", true);
    let reviewed_by = string("reviewed_by", false);
    let translation_of = string("translation_of", false);
    let source_hash = string("source_hash", false);
    let mut lists = |key: &str, required: bool| -> Vec<String> {
        match get(key) {
            Some(value) => list(value).unwrap_or_else(|| {
                errors.push(format!("`{key}` must be a list"));
                Vec::new()
            }),
            None if required => {
                errors.push(format!("`{key}` is required"));
                Vec::new()
            }
            None => Vec::new(),
        }
    };
    let front = FrontMatter {
        audience: lists("audience", true),
        modes: lists("modes", true),
        context: lists("context", true),
        settings: lists("settings", false),
        error_codes: lists("error_codes", false),
        keywords: lists("keywords", true),
        related: lists("related", true),
        collections: lists("collections", true),
        platforms: lists("platforms", true),
        id,
        title,
        kind,
        status,
        summary,
        popover,
        user_need,
        applies_to,
        last_reviewed,
        reviewed_by,
        translation_of,
        source_hash,
    };
    let known = [
        "id",
        "title",
        "type",
        "status",
        "audience",
        "modes",
        "context",
        "settings",
        "error_codes",
        "summary",
        "popover",
        "user_need",
        "keywords",
        "related",
        "collections",
        "applies_to",
        "platforms",
        "last_reviewed",
        "reviewed_by",
        "translation_of",
        "source_hash",
    ];
    for key in map.keys() {
        if let Some(key) = text(key)
            && !known.contains(&key.as_str())
        {
            errors.push(format!("unknown field `{key}`"));
        }
    }
    let check = |errors: &mut Vec<String>, field: &str, value: &str, allowed: &[&str]| {
        if !allowed.contains(&value) {
            errors.push(format!(
                "`{field}: {value}` must be one of {}",
                allowed.join(", ")
            ));
        }
    };
    check(&mut errors, "type", &front.kind, TYPES);
    check(&mut errors, "status", &front.status, STATUSES);
    for value in &front.audience {
        check(&mut errors, "audience", value, AUDIENCES);
    }
    for value in &front.modes {
        check(&mut errors, "modes", value, MODES);
    }
    for value in &front.platforms {
        check(&mut errors, "platforms", value, PLATFORMS);
    }
    if !is_date(&front.last_reviewed) {
        errors.push("`last_reviewed` must be a date (YYYY-MM-DD)".into());
    }
    if errors.is_empty() {
        Ok(front)
    } else {
        Err(errors)
    }
}

pub fn is_date(value: &str) -> bool {
    let parts: Vec<&str> = value.split('-').collect();
    parts.len() == 3
        && parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "---\nid: a.b\ntitle: Do a thing\ntype: task\nstatus: review\naudience: [volunteer]\nmodes: [simple]\ncontext: []\nsummary: Short.\nuser_need: As a volunteer, I want to do it so that it works.\nkeywords: []\nrelated: []\ncollections: []\napplies_to: \">=1.0\"\nplatforms: [windows]\nlast_reviewed: 2026-11-07\n---\n\n# Do a thing\n";

    #[test]
    fn good_front_matter_parses() {
        let split = split(GOOD).unwrap();
        let front = parse(split.front).unwrap();
        assert_eq!(front.id, "a.b");
        assert_eq!(front.kind, "task");
        assert!(split.body.contains("# Do a thing"));
        assert_eq!(split.body_line, 18);
    }

    #[test]
    fn bad_values_are_named() {
        let source = GOOD
            .replace("type: task", "type: essay")
            .replace("audience: [volunteer]", "audience: volunteer");
        let errors = parse(split(&source).unwrap().front).unwrap_err();
        assert!(
            errors.iter().any(|e| e.contains("`type: essay`")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|e| e.contains("`audience` must be a list")),
            "{errors:?}"
        );
    }

    #[test]
    fn missing_front_matter_is_an_error() {
        assert!(split("# No front matter\n").is_err());
    }
}
