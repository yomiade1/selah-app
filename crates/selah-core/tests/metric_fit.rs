//! M3.3 / M3-A2 Metric fit integration & 5,000-set property tests.
use selah_core::CoreService;
use selah_core::library::new_id;
use selah_proto::v1::{
    Actor, ActorOrigin, Arrangement, Chord, Command, CopyrightPlacement, Envelope, FitTargetOutput,
    LyricLine, OutputConfig, OutputRole, Query, ReferencePlacement, Resolution, Section,
    SectionKind, SetOutputConfigs, Song, SplitScriptureSlides, SplitSongSlides, Template,
    TemplateVariant, TranslationLines, command, envelope, query, reply,
};
use selah_render::fit::{
    FitTarget, SongFitInput, SongSectionInput, WARN_EXTRA_SPLIT, fit_song_slides_with_renderer,
    generated_song_slide_to_layer, measure_slide_fits_in_safe_area, scale_resolved_for_slide,
};
use selah_render::template::{AspectVariantStyle, SafeArea, TemplateStyle};
use selah_render::text::TextRenderer;
use std::collections::HashMap;

fn envelope(id: u64, origin: ActorOrigin, body: envelope::Body) -> Envelope {
    Envelope {
        id,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "local-owner".into(),
            device_id: "metric-fit-test".into(),
            origin: origin as i32,
        }),
        body: Some(body),
    }
}

#[test]
fn service_splits_song_and_scripture_with_saved_template_variants_and_validates_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    // Save a multi-variant M3 Template (16:9 + 32:9 LED + 4:3) via CoreService.
    let template_id = new_id();
    let full_style = TemplateStyle::preset_sanctuary();
    let led_style = TemplateStyle {
        safe_area: SafeArea::led_wide(),
        ..TemplateStyle::preset_sanctuary()
    };
    let template = Template {
        id: Some(template_id.clone()),
        name: "Sunday Sanctuary Multi".into(),
        variants: vec![
            TemplateVariant {
                id: Some(new_id()),
                resolution: Some(Resolution {
                    width: 1920,
                    height: 1080,
                }),
                max_lines: 4,
                style_json: full_style.to_json().unwrap(),
            },
            TemplateVariant {
                id: Some(new_id()),
                resolution: Some(Resolution {
                    width: 3840,
                    height: 1080,
                }),
                max_lines: 2,
                style_json: led_style.to_json().unwrap(),
            },
            TemplateVariant {
                id: Some(new_id()),
                resolution: Some(Resolution {
                    width: 1440,
                    height: 1080,
                }),
                max_lines: 4,
                style_json: r#"{"font":"Manrope","size":64,"min_size":28}"#.into(),
            },
        ],
    };
    let save_out = core.handle(&envelope(
        1,
        ActorOrigin::Ui,
        envelope::Body::Command(Command {
            action: Some(command::Action::SaveTemplate(
                selah_proto::v1::SaveTemplate {
                    template: Some(template),
                },
            )),
            ..Default::default()
        }),
    ));
    assert!(matches!(
        save_out.reply.and_then(|r| r.body),
        Some(envelope::Body::Reply(_))
    ));

    // SEC-03: oversized or malformed template style_json is rejected on SaveTemplate.
    let bad_tmpl = Template {
        id: Some(new_id()),
        name: "Too Huge".into(),
        variants: vec![TemplateVariant {
            id: Some(new_id()),
            resolution: Some(Resolution {
                width: 1920,
                height: 1080,
            }),
            max_lines: 4,
            style_json: format!(r#"{{"name":"{}"}}"#, "x".repeat(70_000)),
        }],
    };
    let bad_save = core.handle(&envelope(
        2,
        ActorOrigin::Ui,
        envelope::Body::Command(Command {
            action: Some(command::Action::SaveTemplate(
                selah_proto::v1::SaveTemplate {
                    template: Some(bad_tmpl),
                },
            )),
            ..Default::default()
        }),
    ));
    assert!(matches!(
        bad_save.reply.and_then(|r| r.body),
        Some(envelope::Body::Error(_))
    ));

    // Configure 3 live outputs pointing to the saved template.
    let cfg_out = core.handle(&envelope(
        3,
        ActorOrigin::Ui,
        envelope::Body::Command(Command {
            action: Some(command::Action::SetOutputConfigs(SetOutputConfigs {
                outputs: vec![
                    OutputConfig {
                        output_index: 0,
                        name: "Main 16:9".into(),
                        role: OutputRole::Main as i32,
                        resolution: Some(Resolution {
                            width: 1920,
                            height: 1080,
                        }),
                        enabled: true,
                        template_id: Some(template_id.clone()),
                        ..Default::default()
                    },
                    OutputConfig {
                        output_index: 1,
                        name: "Wide LED".into(),
                        role: OutputRole::Main as i32,
                        resolution: Some(Resolution {
                            width: 3840,
                            height: 1080,
                        }),
                        enabled: true,
                        template_id: Some(template_id.clone()),
                        ..Default::default()
                    },
                    OutputConfig {
                        output_index: 2,
                        name: "Choir 4:3".into(),
                        role: OutputRole::Main as i32,
                        resolution: Some(Resolution {
                            width: 1440,
                            height: 1080,
                        }),
                        enabled: true,
                        template_id: Some(template_id.clone()),
                        ..Default::default()
                    },
                ],
            })),
            ..Default::default()
        }),
    ));
    assert!(matches!(
        cfg_out.reply.and_then(|r| r.body),
        Some(envelope::Body::Reply(_))
    ));

    let v1_id = new_id();
    let tag_id = new_id();
    let mut translations = HashMap::new();
    translations.insert(
        "yo".into(),
        TranslationLines {
            lines: vec![
                "Oore-ọ̀fẹ́ ti o dùn!".into(),
                "Ti o gba oloshi bi emi!".into(),
                "Mo ti sọnu tẹlẹ ṣugbọn a ri mi,".into(),
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
                hotkey: "1".into(),
                lines: vec![
                    LyricLine {
                        text:
                            "Amazing grace! How sweet the sound that saved a wretch like me today"
                                .into(),
                        chords: vec![Chord {
                            position: 0,
                            text: "G".into(),
                        }],
                    },
                    LyricLine {
                        text: "That saved a wretch like me across all the earth and sea forever"
                            .into(),
                        chords: vec![Chord {
                            position: 5,
                            text: "C".into(),
                        }],
                    },
                    LyricLine {
                        text: "I once was lost, but now am found, was blind but now I see.".into(),
                        chords: vec![],
                    },
                    LyricLine {
                        text: "Was blind, but now I see the morning light shining brightly.".into(),
                        chords: vec![],
                    },
                ],
                translations,
                ..Default::default()
            },
            Section {
                id: Some(tag_id.clone()),
                label: "Tag".into(),
                kind: SectionKind::Tag as i32,
                hotkey: "T".into(),
                lines: vec![LyricLine {
                    text: "Forever Amen".into(),
                    chords: vec![],
                }],
                ..Default::default()
            },
        ],
        arrangements: vec![Arrangement {
            id: Some(new_id()),
            name: "Service".into(),
            section_ids: vec![v1_id, tag_id.clone(), tag_id],
        }],
        ..Default::default()
    };

    // Query SplitSongSlides with shrink_to_fit = true using configured live outputs + saved template.
    let song_split = core.handle(&envelope(
        4,
        ActorOrigin::Ui,
        envelope::Body::Query(Query {
            request: Some(query::Request::SplitSongSlides(SplitSongSlides {
                song: Some(song.clone()),
                max_lines: 4,
                copyright_placement: CopyrightPlacement::FirstAndLast as i32,
                title_slide: true,
                blank_slide_at_end: true,
                translation_language: "yo".into(),
                shrink_to_fit: true,
                ..Default::default()
            })),
        }),
    ));
    let song_reply = match song_split.reply.and_then(|r| r.body) {
        Some(envelope::Body::Reply(reply)) => match reply.result {
            Some(reply::Result::GeneratedSongSlides(s)) => s,
            other => panic!("expected GeneratedSongSlides, got {other:?}"),
        },
        other => panic!("unexpected song split reply: {other:?}"),
    };
    assert_eq!(song_reply.fit_reports.len(), 3);
    assert_eq!(song_reply.per_output_slides.len(), 3);
    assert_eq!(song_reply.fit_reports[0].matched_variant, "16:9");
    assert_eq!(song_reply.fit_reports[1].matched_variant, "32:9");
    assert_eq!(song_reply.fit_reports[2].matched_variant, "4:3");
    for rep in &song_reply.fit_reports {
        assert!(
            rep.fits,
            "output {} failed to fit: {:?}",
            rep.output_name, rep
        );
        assert!(rep.applied_size >= rep.min_size);
    }
    // Wide LED (max_lines = 2) forced extra splits relative to 4-line baseline.
    let led_rep = &song_reply.fit_reports[1];
    assert!(led_rep.extra_splits >= 1);
    assert!(led_rep.warning_codes.iter().any(|c| c == WARN_EXTRA_SPLIT));
    // Identical back-to-back arranged sections (Tag, Tag) must stay 1/1 each!
    let led_tags: Vec<_> = song_reply.per_output_slides[1]
        .slides
        .iter()
        .filter(|s| s.section_label == "Tag")
        .collect();
    assert_eq!(led_tags.len(), 2);
    assert_eq!(led_tags[0].part_count, 1);
    assert_eq!(led_tags[1].part_count, 1);

    // Explicit missing template_id returns error cleanly.
    let missing = core.handle(&envelope(
        5,
        ActorOrigin::Ui,
        envelope::Body::Query(Query {
            request: Some(query::Request::SplitSongSlides(SplitSongSlides {
                song: Some(song),
                template_id: Some(new_id()),
                shrink_to_fit: true,
                ..Default::default()
            })),
        }),
    ));
    assert!(matches!(
        missing.reply.and_then(|r| r.body),
        Some(envelope::Body::Error(_))
    ));

    // SplitScriptureSlides with explicit target_outputs returns fitted per-output slides + reports.
    let scr_split = core.handle(&envelope(
        6,
        ActorOrigin::Ui,
        envelope::Body::Query(Query {
            request: Some(query::Request::SplitScriptureSlides(SplitScriptureSlides {
                translation: "KJV".into(),
                translation_name: "King James Version".into(),
                reference: "John 3:16-17".into(),
                verses: vec![
                    selah_proto::v1::ScriptureVerse {
                        translation: "KJV".into(),
                        book_num: 43,
                        book_code: "JHN".into(),
                        book_name: "John".into(),
                        chapter: 3,
                        verse: 16,
                        text: "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.".into(),
                    },
                    selah_proto::v1::ScriptureVerse {
                        translation: "KJV".into(),
                        book_num: 43,
                        book_code: "JHN".into(),
                        book_name: "John".into(),
                        chapter: 3,
                        verse: 17,
                        text: "For God sent not his Son into the world to condemn the world; but that the world through him might be saved.".into(),
                    },
                ],
                max_lines: 4,
                show_verse_numbers: true,
                reference_placement: ReferencePlacement::SlideFooter as i32,
                title_slide: true,
                blank_slide_at_end: true,
                template_id: Some(template_id),
                target_outputs: vec![
                    FitTargetOutput {
                        output_index: 0,
                        name: "Hall 16:9".into(),
                        resolution: Some(Resolution { width: 1920, height: 1080 }),
                        ..Default::default()
                    },
                    FitTargetOutput {
                        output_index: 1,
                        name: "LED 32:9".into(),
                        resolution: Some(Resolution { width: 3840, height: 1080 }),
                        ..Default::default()
                    },
                ],
                shrink_to_fit: true,
                ..Default::default()
            })),
        }),
    ));
    let scr_reply = match scr_split.reply.and_then(|r| r.body) {
        Some(envelope::Body::Reply(reply)) => match reply.result {
            Some(reply::Result::GeneratedScriptureSlides(s)) => s,
            other => panic!("expected GeneratedScriptureSlides, got {other:?}"),
        },
        other => panic!("unexpected scripture fit reply: {other:?}"),
    };
    assert_eq!(scr_reply.fit_reports.len(), 2);
    assert_eq!(scr_reply.per_output_slides.len(), 2);
    assert!(scr_reply.fit_reports.iter().all(|r| r.fits));
}

#[test]
fn m3_a2_property_five_thousand_random_lyric_sets_across_three_outputs() {
    let mut renderer = TextRenderer::bundled().expect("bundled fonts");
    let mut rng = fastrand::Rng::with_seed(0x005E_1AA2_0003);

    let mut stage_tmpl = TemplateStyle::preset_stage_monitor();
    stage_tmpl.variants = vec![
        AspectVariantStyle {
            aspect_ratio: "16:9".into(),
            reference_width: 1920,
            reference_height: 1080,
            max_lines: Some(4),
            safe_area: None,
            roles: None,
        },
        AspectVariantStyle {
            aspect_ratio: "32:9".into(),
            reference_width: 3840,
            reference_height: 1080,
            max_lines: Some(2),
            safe_area: Some(SafeArea::led_wide()),
            roles: None,
        },
        AspectVariantStyle {
            aspect_ratio: "4:3".into(),
            reference_width: 1440,
            reference_height: 1080,
            max_lines: Some(4),
            safe_area: None,
            roles: None,
        },
    ];
    let serif_tmpl = TemplateStyle::preset_warm_serif();
    let sanctuary_tmpl = TemplateStyle::preset_sanctuary();
    let templates = [&sanctuary_tmpl, &serif_tmpl, &stage_tmpl];

    let words_en = [
        "Grace",
        "mercy",
        "everlasting",
        "Hallelujah",
        "congregation",
        "sanctuary",
        "redemption",
        "glory",
        "steadfast",
        "compassion",
        "king",
        "heaven",
        "praise",
        "wonderful",
        "light",
        "promise",
        "righteousness",
        "deliverance",
        "foundation",
        "peace",
    ];
    let words_yo = [
        "Oore-ọ̀fẹ́",
        "Olúwa",
        "àánú",
        "ìmọ́lẹ̀",
        "ìyìn",
        "ọ̀run",
        "àlàáfíà",
        "àrà",
        "ọ̀tọ̀",
        "ẹ̀mí",
    ];
    let words_ko = [
        "주님은",
        "나의",
        "목자시니",
        "할렐루야",
        "은혜와",
        "진리가",
        "영원히",
        "찬양하리",
    ];
    let words_ar = ["الرب", "راعيّ", "سلام", "رحمة", "نور", "حمد", "أبدي"];
    let placements = [
        CopyrightPlacement::None,
        CopyrightPlacement::First,
        CopyrightPlacement::Last,
        CopyrightPlacement::FirstAndLast,
        CopyrightPlacement::All,
    ];

    let mut shrunk_cases = 0usize;
    let mut split_cases = 0usize;

    for set_idx in 0..5_000usize {
        let tmpl = templates[set_idx % templates.len()].clone();
        let targets = [
            FitTarget {
                output_index: 0,
                output_name: "16:9".into(),
                width: 1920,
                height: 1080,
                style: tmpl.clone(),
                max_lines_override: None,
            },
            FitTarget {
                output_index: 1,
                output_name: "32:9 LED".into(),
                width: 3840,
                height: 1080,
                style: tmpl.clone(),
                max_lines_override: None,
            },
            FitTarget {
                output_index: 2,
                output_name: "4:3".into(),
                width: 1440,
                height: 1080,
                style: tmpl.clone(),
                max_lines_override: None,
            },
        ];

        let sec_count = rng.usize(1..=2);
        let include_translation = set_idx % 5 == 0;
        let include_chords = set_idx % 4 == 0;
        let max_lines = [2u32, 3, 4][rng.usize(0..3)];
        let copyright_placement = placements[set_idx % placements.len()];
        let title_slide = set_idx % 7 == 0;
        let blank_slide_at_end = set_idx % 11 == 0;

        let mut sections = Vec::with_capacity(sec_count);
        for s_idx in 0..sec_count {
            let line_count = rng.usize(1..=6);
            let mut lines = Vec::with_capacity(line_count);
            let mut trans_lines = Vec::with_capacity(line_count);
            for _ in 0..line_count {
                let text = match (set_idx + s_idx) % 10 {
                    // Ultra-long unbreakable token edge case
                    0 => format!("Hallelujah{}", "a".repeat(rng.usize(40..=92))),
                    // Yorùbá diacritics
                    1 => (0..rng.usize(3..=12))
                        .map(|_| words_yo[rng.usize(0..words_yo.len())])
                        .collect::<Vec<_>>()
                        .join(" "),
                    // Korean Hangul
                    2 => (0..rng.usize(3..=10))
                        .map(|_| words_ko[rng.usize(0..words_ko.len())])
                        .collect::<Vec<_>>()
                        .join(" "),
                    // Arabic RTL
                    3 => (0..rng.usize(3..=10))
                        .map(|_| words_ar[rng.usize(0..words_ar.len())])
                        .collect::<Vec<_>>()
                        .join(" "),
                    // Long multi-word sentence forcing shrink + multi-slide re-split
                    4 | 5 => (0..rng.usize(12..=28))
                        .map(|_| words_en[rng.usize(0..words_en.len())])
                        .collect::<Vec<_>>()
                        .join(" "),
                    // Typical worship lyric lines
                    _ => (0..rng.usize(2..=10))
                        .map(|_| words_en[rng.usize(0..words_en.len())])
                        .collect::<Vec<_>>()
                        .join(" "),
                };
                let char_len = text.chars().count();
                let chords = if include_chords && char_len >= 4 {
                    vec![
                        Chord {
                            position: 0,
                            text: "G".into(),
                        },
                        Chord {
                            position: (char_len / 2) as u32,
                            text: "D/F#".into(),
                        },
                    ]
                } else {
                    Vec::new()
                };
                lines.push(LyricLine { text, chords });
                if include_translation {
                    let t_words = rng.usize(3..=12);
                    let t = (0..t_words)
                        .map(|_| words_yo[rng.usize(0..words_yo.len())])
                        .collect::<Vec<_>>()
                        .join(" ");
                    trans_lines.push(t);
                }
            }
            sections.push(SongSectionInput {
                section_id: None,
                section_label: if s_idx == 0 {
                    "Verse 1".into()
                } else {
                    "Chorus".into()
                },
                section_kind: SectionKind::Verse as i32,
                section_color: None,
                section_hotkey: if s_idx == 0 { "1".into() } else { "C".into() },
                lines,
                translation_lines: trans_lines,
            });
        }

        let input = SongFitInput {
            sections,
            title_lines: title_slide.then(|| vec![format!("Hymn #{set_idx}"), "Composer".into()]),
            copyright_line: "Public Domain · CCLI #12345".into(),
            copyright_placement,
            blank_slide_at_end,
            max_lines,
        };

        let result = fit_song_slides_with_renderer(&mut renderer, &input, &targets)
            .unwrap_or_else(|e| panic!("set {set_idx} errored: {e}"));
        assert_eq!(result.per_output.len(), 3);

        for (target, (report, slides)) in targets.iter().zip(&result.per_output) {
            assert!(
                report.fits,
                "set {set_idx} output {} did not fit: {:?}",
                target.output_name, report
            );
            assert!(
                !report.overflows_safe_area,
                "set {set_idx} output {} overflowed safe area",
                target.output_name
            );
            assert_eq!(report.final_slide_count as usize, slides.len());
            assert_eq!(
                report.extra_splits,
                report
                    .final_slide_count
                    .saturating_sub(report.original_slide_count)
            );
            assert!(
                report.applied_size + 1e-3 >= report.min_size,
                "set {set_idx} output {} dropped below min_size: {} < {}",
                target.output_name,
                report.applied_size,
                report.min_size
            );
            if report.warning_codes.iter().any(|c| c == "SHRUNK_TO_FIT") {
                shrunk_cases += 1;
            }
            if report.extra_splits > 0 {
                split_cases += 1;
            }

            let resolved = target.style.resolve_for_output(
                target.width,
                target.height,
                target.max_lines_override,
            );
            for (slide_idx, slide) in slides.iter().enumerate() {
                if slide.is_blank {
                    continue;
                }
                let slide_min = if slide.is_title {
                    resolved.roles.title.min_size
                } else {
                    resolved.roles.body.min_size
                };
                assert!(
                    slide.applied_size + 1e-3 >= slide_min,
                    "set {set_idx} output {} slide {slide_idx} fell below min_size: {} < {slide_min}",
                    target.output_name,
                    slide.applied_size
                );
                let scaled =
                    scale_resolved_for_slide(&resolved, slide.applied_size, slide.is_title, None);
                let layer = generated_song_slide_to_layer(slide, None);
                let check = measure_slide_fits_in_safe_area(&mut renderer, &layer, &scaled);
                assert!(
                    check.fits && !check.overflows_safe_area,
                    "set {set_idx} output {} slide {slide_idx} failed safe area check: {check:?} lines={:?} trans={:?}",
                    target.output_name,
                    slide.lines.iter().map(|l| &l.text).collect::<Vec<_>>(),
                    slide.translation_lines
                );
                // Also rasterise sampled complex slides (scripts, chords, translations, footers)
                // and verify rendered Skia pixels stay strictly inside `SafeRect`.
                if (set_idx < 12 || set_idx % 250 == 0) && slide_idx < 2 {
                    let safe = scaled.safe_area.resolved_rect(target.width, target.height);
                    let frame = renderer
                        .render_slide(&layer, &scaled, target.width, target.height)
                        .expect("slide pixel render");
                    for y in 0..frame.height {
                        for x in 0..frame.width {
                            let px = &frame.pixels
                                [(y as usize * frame.width as usize + x as usize) * 4..][..4];
                            if px[3] > 32 {
                                assert!(
                                    x as f32 >= safe.left - 2.0
                                        && (x as f32) <= safe.right + 2.0
                                        && y as f32 >= safe.top - 2.0
                                        && (y as f32) <= safe.bottom + 2.0,
                                    "set {set_idx} output {} slide {slide_idx} pixel ({x},{y}) alpha={} escaped safe area {safe:?}",
                                    target.output_name,
                                    px[3]
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    assert!(
        shrunk_cases > 500,
        "expected many shrunk cases across 5000 sets, got {shrunk_cases}"
    );
    assert!(
        split_cases > 500,
        "expected many extra-split cases across 5000 sets, got {split_cases}"
    );
}
