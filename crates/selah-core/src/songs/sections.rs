use std::collections::HashSet;

use anyhow::{Context, ensure};
use selah_proto::v1::{Arrangement, Color, Id, Section, SectionKind, Song};

/// DESIGN-SYSTEM §3 & tokens.json hex values per SectionKind.
pub fn section_color_hex(kind: SectionKind) -> &'static str {
    match kind {
        SectionKind::Verse => "#2378C3",
        SectionKind::Chorus => "#C84281",
        SectionKind::Bridge => "#E66F0E",
        SectionKind::Tag => "#009EC1",
        SectionKind::Intro | SectionKind::Outro | SectionKind::Other | SectionKind::Unspecified => {
            "#71767A"
        }
    }
}

pub fn section_color(kind: SectionKind) -> Color {
    match kind {
        SectionKind::Verse => rgb(0x23, 0x78, 0xC3),
        SectionKind::Chorus => rgb(0xC8, 0x42, 0x81),
        SectionKind::Bridge => rgb(0xE6, 0x6F, 0x0E),
        SectionKind::Tag => rgb(0x00, 0x9E, 0xC1),
        SectionKind::Intro | SectionKind::Outro | SectionKind::Other | SectionKind::Unspecified => {
            rgb(0x71, 0x76, 0x7A)
        }
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color {
        red: f32::from(r) / 255.0,
        green: f32::from(g) / 255.0,
        blue: f32::from(b) / 255.0,
        alpha: 1.0,
    }
}

pub fn section_kind_key(kind: SectionKind) -> &'static str {
    match kind {
        SectionKind::Verse => "verse",
        SectionKind::Chorus => "chorus",
        SectionKind::Bridge => "bridge",
        SectionKind::Tag => "tag",
        SectionKind::Intro => "intro",
        SectionKind::Outro => "outro",
        SectionKind::Other | SectionKind::Unspecified => "other",
    }
}

pub fn color_to_hex(color: &Color) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        byte(color.red),
        byte(color.green),
        byte(color.blue)
    )
}

/// Deterministic preferred single-key shortcut avoiding F9–F12 live keys.
pub fn preferred_hotkey(kind: SectionKind, label: &str) -> Option<String> {
    let lower = label.trim().to_ascii_lowercase();
    if let Some(num) = lower
        .strip_prefix("verse")
        .or_else(|| lower.strip_prefix('v'))
        .map(str::trim)
        && let Ok(n) = num.parse::<u32>()
        && (1..=9).contains(&n)
    {
        return Some(n.to_string());
    }
    if lower == "verse" {
        return Some("1".into());
    }
    if lower.starts_with("pre-chorus") || lower.starts_with("prechorus") || lower == "pc" {
        return Some("P".into());
    }
    if lower.starts_with("interlude") || lower.starts_with("instrumental") {
        return Some("N".into());
    }
    if lower.starts_with("ending") {
        return Some("E".into());
    }
    match kind {
        SectionKind::Chorus => Some("C".into()),
        SectionKind::Bridge => Some("B".into()),
        SectionKind::Tag => Some("T".into()),
        SectionKind::Intro => Some("I".into()),
        SectionKind::Outro => Some("O".into()),
        SectionKind::Verse => None,
        SectionKind::Other | SectionKind::Unspecified => Some("X".into()),
    }
}

pub fn assign_default_hotkeys_and_colors(sections: &mut [Section]) {
    let mut used = HashSet::new();
    for section in sections.iter() {
        let existing = section.hotkey.trim();
        if !existing.is_empty() {
            used.insert(existing.to_ascii_uppercase());
        }
    }
    // Pass 1: assign section colours and reserve primary preferred hotkeys before allocating
    // fallback keys for collisions (e.g. `Chorus 2` must not steal `B` before `Bridge` runs).
    for section in sections.iter_mut() {
        let kind = SectionKind::try_from(section.kind).unwrap_or(SectionKind::Other);
        if section.color.is_none() {
            section.color = Some(section_color(kind));
        }
        if section.hotkey.trim().is_empty()
            && let Some(key) = preferred_hotkey(kind, &section.label)
            && used.insert(key.to_ascii_uppercase())
        {
            section.hotkey = key;
        }
    }
    let fallback_pool: [char; 35] = [
        'A', 'D', 'F', 'G', 'H', 'J', 'K', 'L', 'M', 'Q', 'R', 'S', 'U', 'V', 'W', 'X', 'Y', 'Z',
        '1', '2', '3', '4', '5', '6', '7', '8', '9', 'C', 'B', 'T', 'P', 'I', 'O', 'E', 'N',
    ];
    // Pass 2: assign remaining sections an unused key from the fallback pool.
    for section in sections.iter_mut() {
        if section.hotkey.trim().is_empty() {
            let chosen = fallback_pool
                .iter()
                .map(|c| c.to_string())
                .find(|k| used.insert(k.to_ascii_uppercase()))
                .unwrap_or_default();
            section.hotkey = chosen;
        }
    }
}

pub fn resolve_arrangement_sections<'a>(
    song: &'a Song,
    arrangement_id: Option<&Id>,
) -> anyhow::Result<Vec<&'a Section>> {
    if song.sections.is_empty() {
        return Ok(Vec::new());
    }
    let arrangement: Option<&Arrangement> = match arrangement_id {
        Some(target_id) if !target_id.value.is_empty() => Some(
            song.arrangements
                .iter()
                .find(|a| a.id.as_ref() == Some(target_id))
                .context("requested arrangement does not exist on song")?,
        ),
        _ => song.arrangements.first(),
    };
    let Some(arrangement) = arrangement else {
        return Ok(song.sections.iter().collect());
    };
    if arrangement.section_ids.is_empty() {
        return Ok(song.sections.iter().collect());
    }
    let mut out = Vec::with_capacity(arrangement.section_ids.len());
    for id in &arrangement.section_ids {
        let sec = song
            .sections
            .iter()
            .find(|s| s.id.as_ref() == Some(id))
            .context("arrangement refers to a missing section")?;
        ensure!(
            out.len() < crate::library::MAX_CHILDREN,
            "arrangement too long"
        );
        out.push(sec);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::new_id;

    #[test]
    fn section_colors_match_design_system() {
        assert_eq!(color_to_hex(&section_color(SectionKind::Verse)), "#2378C3");
        assert_eq!(color_to_hex(&section_color(SectionKind::Chorus)), "#C84281");
        assert_eq!(color_to_hex(&section_color(SectionKind::Bridge)), "#E66F0E");
        assert_eq!(color_to_hex(&section_color(SectionKind::Tag)), "#009EC1");
        assert_eq!(color_to_hex(&section_color(SectionKind::Intro)), "#71767A");
        assert_eq!(color_to_hex(&section_color(SectionKind::Outro)), "#71767A");
        assert_eq!(color_to_hex(&section_color(SectionKind::Other)), "#71767A");
    }

    #[test]
    fn default_hotkeys_are_unique_across_sections() {
        let mut sections = vec![
            Section {
                id: Some(new_id()),
                label: "Verse 1".into(),
                kind: SectionKind::Verse as i32,
                ..Default::default()
            },
            Section {
                id: Some(new_id()),
                label: "Chorus".into(),
                kind: SectionKind::Chorus as i32,
                ..Default::default()
            },
            Section {
                id: Some(new_id()),
                label: "Chorus 2".into(),
                kind: SectionKind::Chorus as i32,
                ..Default::default()
            },
            Section {
                id: Some(new_id()),
                label: "Pre-Chorus".into(),
                kind: SectionKind::Other as i32,
                ..Default::default()
            },
            Section {
                id: Some(new_id()),
                label: "Bridge".into(),
                kind: SectionKind::Bridge as i32,
                ..Default::default()
            },
            Section {
                id: Some(new_id()),
                label: "Tag".into(),
                kind: SectionKind::Tag as i32,
                ..Default::default()
            },
        ];
        assign_default_hotkeys_and_colors(&mut sections);
        let keys: Vec<&str> = sections.iter().map(|s| s.hotkey.as_str()).collect();
        assert_eq!(keys[0], "1");
        assert_eq!(keys[1], "C");
        assert_ne!(keys[2], "C");
        assert_eq!(keys[3], "P");
        assert_eq!(keys[4], "B");
        assert_eq!(keys[5], "T");
        let unique: HashSet<&str> = keys.iter().copied().collect();
        assert_eq!(unique.len(), keys.len());
    }
}
