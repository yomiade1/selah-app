use anyhow::ensure;
use selah_proto::v1::{
    CopyrightPlacement, GeneratedSlide, GeneratedSongSlides, Id, LyricLine, OutputFittedSongSlides,
    SectionKind, Song,
};
use selah_render::fit::{FitTarget, SongFitInput, SongSectionInput, fit_song_slides};

use super::sections::{resolve_arrangement_sections, section_color};

pub const DEFAULT_MAX_LINES: u32 = 4;
pub const MAX_SLIDE_LINES: u32 = 100;

#[derive(Clone, Debug, Default)]
pub struct SplitOptions {
    pub arrangement_id: Option<Id>,
    pub max_lines: u32,
    pub copyright_placement: CopyrightPlacement,
    pub title_slide: bool,
    pub blank_slide_at_end: bool,
    pub translation_language: Option<String>,
}

pub fn format_copyright_line(song: &Song) -> String {
    let mut parts = Vec::new();
    let title = song.title.trim();
    if !title.is_empty() {
        parts.push(format!("\u{201C}{title}\u{201D}"));
    }
    let authors: Vec<&str> = song
        .authors
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if !authors.is_empty() {
        parts.push(authors.join(", "));
    }
    let copyright = song.copyright.trim();
    if !copyright.is_empty() {
        parts.push(copyright.to_owned());
    } else if song.public_domain {
        parts.push("Public Domain".to_owned());
    }
    let ccli = song.ccli_number.trim();
    if !ccli.is_empty() {
        parts.push(format!("CCLI #{ccli}"));
    }
    parts.join(" \u{00B7} ")
}

/// Rule-based slide generator (M2 / ARCHITECTURE §4.3):
/// breaks at sections, splits each section into chunks of at most `max_lines` while preserving
/// aligned chords and optional translation lines, then applies title/blank/copyright rules.
pub fn generate_slides(song: &Song, options: &SplitOptions) -> anyhow::Result<GeneratedSongSlides> {
    let max_lines = if options.max_lines == 0 {
        DEFAULT_MAX_LINES
    } else {
        options.max_lines
    };
    ensure!(
        (1..=MAX_SLIDE_LINES).contains(&max_lines),
        "max_lines must be between 1 and {MAX_SLIDE_LINES}"
    );
    let max_per_slide = max_lines as usize;

    let trans_lang = options
        .translation_language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let sections = resolve_arrangement_sections(song, options.arrangement_id.as_ref())?;
    let mut slides: Vec<GeneratedSlide> = Vec::new();

    if options.title_slide {
        let mut lines = vec![LyricLine {
            text: song.title.trim().to_owned(),
            chords: Vec::new(),
        }];
        let authors = song
            .authors
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if !authors.is_empty() {
            lines.push(LyricLine {
                text: authors,
                chords: Vec::new(),
            });
        }
        slides.push(GeneratedSlide {
            section_id: None,
            section_label: "Title".into(),
            section_kind: SectionKind::Other as i32,
            section_color: Some(section_color(SectionKind::Other)),
            section_hotkey: String::new(),
            part_index: 1,
            part_count: 1,
            lines,
            translation_lines: Vec::new(),
            footer_copyright: String::new(),
            is_title: true,
            is_blank: false,
            applied_size: 0.0,
        });
    }

    for section in sections {
        let kind = SectionKind::try_from(section.kind).unwrap_or(SectionKind::Other);
        let color = section.color.unwrap_or_else(|| section_color(kind));
        let translations_for_lang = trans_lang
            .and_then(|lang| section.translations.get(lang))
            .map(|t| t.lines.as_slice());

        if section.lines.is_empty() {
            slides.push(GeneratedSlide {
                section_id: section.id.clone(),
                section_label: section.label.clone(),
                section_kind: kind as i32,
                section_color: Some(color),
                section_hotkey: section.hotkey.clone(),
                part_index: 1,
                part_count: 1,
                lines: Vec::new(),
                translation_lines: Vec::new(),
                footer_copyright: String::new(),
                is_title: false,
                is_blank: false,
                applied_size: 0.0,
            });
            continue;
        }

        let total_lines = section.lines.len();
        let part_count = total_lines.div_ceil(max_per_slide) as u32;
        for (chunk_idx, start) in (0..total_lines).step_by(max_per_slide).enumerate() {
            let end = (start + max_per_slide).min(total_lines);
            let lines = section.lines[start..end].to_vec();
            let translation_lines = match translations_for_lang {
                Some(t_lines) => (start..end)
                    .map(|idx| t_lines.get(idx).cloned().unwrap_or_default())
                    .collect(),
                None => Vec::new(),
            };
            slides.push(GeneratedSlide {
                section_id: section.id.clone(),
                section_label: section.label.clone(),
                section_kind: kind as i32,
                section_color: Some(color),
                section_hotkey: section.hotkey.clone(),
                part_index: (chunk_idx + 1) as u32,
                part_count,
                lines,
                translation_lines,
                footer_copyright: String::new(),
                is_title: false,
                is_blank: false,
                applied_size: 0.0,
            });
        }
    }

    let copyright_line = format_copyright_line(song);
    if !copyright_line.is_empty() && !slides.is_empty() {
        let content_len = slides.len();
        match options.copyright_placement {
            CopyrightPlacement::Unspecified | CopyrightPlacement::None => {}
            CopyrightPlacement::First => {
                slides[0].footer_copyright = copyright_line.clone();
            }
            CopyrightPlacement::Last => {
                slides[content_len - 1].footer_copyright = copyright_line.clone();
            }
            CopyrightPlacement::FirstAndLast => {
                slides[0].footer_copyright = copyright_line.clone();
                if content_len > 1 {
                    slides[content_len - 1].footer_copyright = copyright_line.clone();
                }
            }
            CopyrightPlacement::All => {
                for slide in &mut slides {
                    slide.footer_copyright = copyright_line.clone();
                }
            }
        }
    }

    if options.blank_slide_at_end {
        slides.push(GeneratedSlide {
            section_id: None,
            section_label: "Blank".into(),
            section_kind: SectionKind::Other as i32,
            section_color: Some(section_color(SectionKind::Other)),
            section_hotkey: String::new(),
            part_index: 1,
            part_count: 1,
            lines: Vec::new(),
            translation_lines: Vec::new(),
            footer_copyright: String::new(),
            is_title: false,
            is_blank: true,
            applied_size: 0.0,
        });
    }

    Ok(GeneratedSongSlides {
        slides,
        copyright_line,
        fit_reports: Vec::new(),
        per_output_slides: Vec::new(),
    })
}

/// Skia metric-fit slide generator (M3.3 / M3-A2): starts from rule splitting, shrinks font sizes
/// down to each template variant's `min_size`, cleanly re-splits across extra slides when needed,
/// and returns per-output fit reports + per-output slide decks.
pub fn generate_slides_with_fit(
    song: &Song,
    options: &SplitOptions,
    targets: &[FitTarget],
) -> anyhow::Result<GeneratedSongSlides> {
    let max_lines = if options.max_lines == 0 {
        DEFAULT_MAX_LINES
    } else {
        options.max_lines
    };
    ensure!(
        (1..=MAX_SLIDE_LINES).contains(&max_lines),
        "max_lines must be between 1 and {MAX_SLIDE_LINES}"
    );

    let trans_lang = options
        .translation_language
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let resolved_sections = resolve_arrangement_sections(song, options.arrangement_id.as_ref())?;
    let mut fit_sections = Vec::with_capacity(resolved_sections.len());
    for section in resolved_sections {
        let kind = SectionKind::try_from(section.kind).unwrap_or(SectionKind::Other);
        let color = section.color.unwrap_or_else(|| section_color(kind));
        let trans_lines = trans_lang
            .and_then(|lang| section.translations.get(lang))
            .map(|t| t.lines.clone())
            .unwrap_or_default();
        fit_sections.push(SongSectionInput {
            section_id: section.id.clone(),
            section_label: section.label.clone(),
            section_kind: kind as i32,
            section_color: Some(color),
            section_hotkey: section.hotkey.clone(),
            lines: section.lines.clone(),
            translation_lines: trans_lines,
        });
    }

    let title_lines = options.title_slide.then(|| {
        let mut lines = vec![song.title.trim().to_owned()];
        let authors = song
            .authors
            .iter()
            .map(String::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
        if !authors.is_empty() {
            lines.push(authors);
        }
        lines
    });

    let copyright_line = format_copyright_line(song);
    let fitted = fit_song_slides(
        &SongFitInput {
            sections: fit_sections,
            title_lines,
            copyright_line: copyright_line.clone(),
            copyright_placement: options.copyright_placement,
            blank_slide_at_end: options.blank_slide_at_end,
            max_lines: options.max_lines,
        },
        targets,
    )?;

    let fit_reports = fitted.per_output.iter().map(|(r, _)| r.clone()).collect();
    let per_output_slides = fitted
        .per_output
        .into_iter()
        .map(|(r, s)| OutputFittedSongSlides {
            output_index: r.output_index,
            slides: s,
        })
        .collect();

    Ok(GeneratedSongSlides {
        slides: fitted.primary_slides,
        copyright_line,
        fit_reports,
        per_output_slides,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::library::new_id;
    use selah_proto::v1::{Arrangement, Chord, Section, TranslationLines};

    #[test]
    fn split_breaks_by_max_lines_expands_arrangements_and_aligns_translations() {
        let v1_id = new_id();
        let c_id = new_id();
        let arr_id = new_id();
        let mut trans = HashMap::new();
        trans.insert(
            "yo".to_owned(),
            TranslationLines {
                lines: vec![
                    "Ọore-ọfẹ ti o pọ̀!".into(),
                    "Ti o gba oloshi bi emi!".into(),
                    "Mo ti sọnu tẹlẹ, ṣugbọn a ri mi,".into(),
                    "Mo fọju, ṣugbọn mo riran.".into(),
                ],
            },
        );
        let song = Song {
            id: Some(new_id()),
            title: "Amazing Grace".into(),
            authors: vec!["John Newton".into()],
            ccli_number: "22025".into(),
            public_domain: true,
            sections: vec![
                Section {
                    id: Some(v1_id.clone()),
                    label: "Verse 1".into(),
                    kind: SectionKind::Verse as i32,
                    color: Some(section_color(SectionKind::Verse)),
                    hotkey: "1".into(),
                    lines: vec![
                        LyricLine {
                            text: "Amazing grace! How sweet the sound".into(),
                            chords: vec![Chord {
                                position: 0,
                                text: "G".into(),
                            }],
                        },
                        LyricLine {
                            text: "That saved a wretch like me!".into(),
                            chords: vec![],
                        },
                        LyricLine {
                            text: "I once was lost, but now am found,".into(),
                            chords: vec![],
                        },
                        LyricLine {
                            text: "Was blind, but now I see.".into(),
                            chords: vec![],
                        },
                    ],
                    translations: trans,
                },
                Section {
                    id: Some(c_id.clone()),
                    label: "Tag".into(),
                    kind: SectionKind::Tag as i32,
                    color: Some(section_color(SectionKind::Tag)),
                    hotkey: "T".into(),
                    lines: vec![LyricLine {
                        text: "Was blind, but now I see.".into(),
                        chords: vec![],
                    }],
                    ..Default::default()
                },
            ],
            arrangements: vec![Arrangement {
                id: Some(arr_id.clone()),
                name: "Sunday".into(),
                section_ids: vec![v1_id, c_id.clone(), c_id],
            }],
            ..Default::default()
        };

        let result = generate_slides(
            &song,
            &SplitOptions {
                arrangement_id: Some(arr_id),
                max_lines: 2,
                copyright_placement: CopyrightPlacement::FirstAndLast,
                title_slide: true,
                blank_slide_at_end: true,
                translation_language: Some("yo".into()),
            },
        )
        .unwrap();

        // Title + 2 Verse 1 slides + 2 Tag slides + trailing Blank = 6 slides
        assert_eq!(result.slides.len(), 6);
        assert!(result.slides[0].is_title);
        assert!(!result.slides[0].footer_copyright.is_empty());
        assert_eq!(result.slides[1].section_label, "Verse 1");
        assert_eq!(result.slides[1].part_index, 1);
        assert_eq!(result.slides[1].part_count, 2);
        assert_eq!(result.slides[1].lines[0].chords[0].text, "G");
        assert_eq!(result.slides[1].translation_lines.len(), 2);
        assert_eq!(
            result.slides[2].translation_lines[1],
            "Mo fọju, ṣugbọn mo riran."
        );
        assert_eq!(result.slides[3].section_label, "Tag");
        assert_eq!(result.slides[4].section_label, "Tag");
        // Copyright is on last non-blank content slide, never on the blank slide
        assert!(!result.slides[4].footer_copyright.is_empty());
        assert!(result.slides[5].is_blank);
        assert!(result.slides[5].footer_copyright.is_empty());
    }
}
