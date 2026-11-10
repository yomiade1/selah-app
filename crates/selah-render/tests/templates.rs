//! M3.2 template & text-styling tests: all 7 bundled variable fonts, weights 100..=900,
//! outline, shadow, background box, spacing, alignments, safe area, role slots, variants,
//! and CPU/GPU parity.
use selah_proto::v1::{
    LayerKind, LayerSpec, OutputScene, PreloadHint, Resolution, Scene, SlideLayerSpec, layer_spec,
};
use selah_render::Compositor;
use selah_render::cpu::CpuCompositor;
use selah_render::gpu::{AdapterChoice, GpuCompositor};
use selah_render::template::{
    AspectVariantStyle, BackgroundBoxStyle, OutlineStyle, RoleSlotsPatch, SafeArea, ShadowStyle,
    SlotTextStyle, SlotTextStylePatch, TemplateStyle, TextAlignH, TextAlignV,
};
use selah_render::text::{ALL_FONTS, TextRenderer};

fn alpha_bounding_box(frame: &selah_render::Frame) -> Option<(u32, u32, u32, u32)> {
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;
    for y in 0..frame.height {
        for x in 0..frame.width {
            let at = (y as usize * frame.width as usize + x as usize) * 4;
            if frame.pixels[at + 3] > 8 {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    any.then_some((min_x, min_y, max_x, max_y))
}

fn alpha_sum(frame: &selah_render::Frame) -> u64 {
    frame
        .pixels
        .chunks_exact(4)
        .map(|px| u64::from(px[3]))
        .sum()
}

fn has_pixel_matching(frame: &selah_render::Frame, pred: impl Fn(&[u8]) -> bool) -> bool {
    frame.pixels.chunks_exact(4).any(pred)
}

#[test]
fn all_seven_bundled_fonts_shape_and_render_distinctly() {
    let mut renderer = TextRenderer::bundled().expect("bundled fonts");
    let latin_samples = [
        "Inter",
        "Manrope",
        "JetBrains Mono",
        "Noto Serif",
        "Noto Sans",
    ];
    let mut hashes = Vec::new();
    for family in latin_samples {
        assert_eq!(
            renderer.unresolved_glyphs_for("Grace & Peace 123", family, 600),
            0,
            "{family} should shape ASCII cleanly"
        );
        let style = TemplateStyle {
            roles: selah_render::template::RoleSlots {
                body: SlotTextStyle {
                    font_family: family.into(),
                    size: 56.0,
                    weight: 600,
                    ..SlotTextStyle::default()
                },
                ..Default::default()
            },
            ..TemplateStyle::default()
        };
        let resolved = style.resolve_for_output(640, 360, None);
        let slide = SlideLayerSpec {
            text: "Grace & Peace 123".into(),
            ..Default::default()
        };
        let frame = renderer.render_slide(&slide, &resolved, 640, 360).unwrap();
        assert!(alpha_sum(&frame) > 5_000, "{family} produced blank pixels");
        hashes.push((family, frame.checksum()));
    }
    for i in 0..hashes.len() {
        for j in (i + 1)..hashes.len() {
            assert_ne!(
                hashes[i].1, hashes[j].1,
                "fonts {} and {} rendered identical pixels",
                hashes[i].0, hashes[j].0
            );
        }
    }
    assert_eq!(
        renderer.unresolved_glyphs_for("주님은 나의 목자", "Noto Sans KR", 700),
        0
    );
    assert_eq!(
        renderer.unresolved_glyphs_for("الرب راعيّ", "Noto Naskh Arabic", 700),
        0
    );
    assert_eq!(ALL_FONTS.len(), 7);
}

#[test]
fn variable_weights_produce_monotonically_heavier_strokes() {
    let mut renderer = TextRenderer::bundled().unwrap();
    for family in [
        "Inter",
        "Manrope",
        "JetBrains Mono",
        "Noto Sans",
        "Noto Serif",
    ] {
        let mut sums = Vec::new();
        for weight in [100u16, 400, 550, 700, 900] {
            let style = TemplateStyle {
                roles: selah_render::template::RoleSlots {
                    body: SlotTextStyle {
                        font_family: family.into(),
                        size: 72.0,
                        weight,
                        ..SlotTextStyle::default()
                    },
                    ..Default::default()
                },
                ..TemplateStyle::default()
            };
            let resolved = style.resolve_for_output(640, 240, None);
            let slide = SlideLayerSpec {
                text: "Hallelujah".into(),
                ..Default::default()
            };
            let frame = renderer.render_slide(&slide, &resolved, 640, 240).unwrap();
            sums.push((weight, alpha_sum(&frame)));
        }
        assert!(
            sums[0].1 < sums[1].1 && sums[1].1 < sums[3].1 && sums[3].1 < sums[4].1,
            "{family} weights did not thicken strokes monotonically: {sums:?}"
        );
        assert!(
            sums[1].1 < sums[2].1 && sums[2].1 < sums[3].1,
            "{family} intermediate weight 550 did not sit between 400 and 700: {sums:?}"
        );
    }
}

#[test]
fn outline_shadow_background_box_spacing_and_alignment_all_take_effect() {
    let mut renderer = TextRenderer::bundled().unwrap();

    // 1. Red outline + cyan glyph fill + green-tinted shadow on one word.
    let decorated = TemplateStyle {
        roles: selah_render::template::RoleSlots {
            body: SlotTextStyle {
                font_family: "Inter".into(),
                size: 80.0,
                weight: 700,
                color: [0, 255, 255, 255],
                outline: Some(OutlineStyle {
                    color: [255, 0, 0, 255],
                    width: 6.0,
                }),
                shadow: Some(ShadowStyle {
                    color: [0, 220, 0, 220],
                    offset_x: 18.0,
                    offset_y: 18.0,
                    blur: 4.0,
                }),
                background_box: Some(BackgroundBoxStyle {
                    color: [16, 24, 96, 240],
                    padding_x: 24.0,
                    padding_y: 16.0,
                    radius: 12.0,
                    full_width: false,
                    accent_bar_color: Some([255, 190, 46, 255]),
                    accent_bar_width: 10.0,
                }),
                ..SlotTextStyle::default()
            },
            ..Default::default()
        },
        ..TemplateStyle::default()
    };
    let resolved = decorated.resolve_for_output(800, 400, None);
    let frame = renderer
        .render_slide(
            &SlideLayerSpec {
                text: "Shine".into(),
                ..Default::default()
            },
            &resolved,
            800,
            400,
        )
        .unwrap();
    // Red outline pixels present
    assert!(has_pixel_matching(&frame, |px| px[0] > 200
        && px[1] < 40
        && px[2] < 40));
    // Cyan fill pixels present
    assert!(has_pixel_matching(&frame, |px| px[0] < 40
        && px[1] > 200
        && px[2] > 200));
    // Background navy card pixels present
    assert!(has_pixel_matching(&frame, |px| px[0] < 35
        && px[2] > 70
        && px[3] > 200));
    // Amber USWDS accent bar pixels present
    assert!(has_pixel_matching(&frame, |px| px[0] > 220
        && px[1] > 160
        && px[2] < 80));

    // 2. Line height & letter spacing widen measurements predictably.
    let tight = SlotTextStyle {
        font_family: "Inter".into(),
        size: 48.0,
        line_height: 1.0,
        letter_spacing: 0.0,
        ..SlotTextStyle::default()
    };
    let wide = SlotTextStyle {
        font_family: "Inter".into(),
        size: 48.0,
        line_height: 2.0,
        letter_spacing: 12.0,
        ..SlotTextStyle::default()
    };
    let m_tight = renderer.measure_slot("Line one\nLine two", &tight, 1200.0);
    let m_wide = renderer.measure_slot("Line one\nLine two", &wide, 1200.0);
    assert!(m_wide.height > m_tight.height * 1.5);
    assert!(m_wide.longest_line > m_tight.longest_line + 40.0);

    // 3. Horizontal & Vertical align position ink strictly within safe area corners.
    let top_left = TemplateStyle {
        safe_area: SafeArea {
            left: 0.10,
            top: 0.12,
            right: 0.10,
            bottom: 0.12,
        },
        roles: selah_render::template::RoleSlots {
            body: SlotTextStyle {
                size: 42.0,
                align_h: TextAlignH::Left,
                align_v: TextAlignV::Top,
                ..SlotTextStyle::default()
            },
            ..Default::default()
        },
        ..TemplateStyle::default()
    };
    let bottom_right = TemplateStyle {
        roles: selah_render::template::RoleSlots {
            body: SlotTextStyle {
                size: 42.0,
                align_h: TextAlignH::Right,
                align_v: TextAlignV::Bottom,
                ..SlotTextStyle::default()
            },
            ..Default::default()
        },
        ..top_left.clone()
    };
    let f_tl = renderer
        .render_slide(
            &SlideLayerSpec {
                text: "Corner".into(),
                ..Default::default()
            },
            &top_left.resolve_for_output(640, 360, None),
            640,
            360,
        )
        .unwrap();
    let f_br = renderer
        .render_slide(
            &SlideLayerSpec {
                text: "Corner".into(),
                ..Default::default()
            },
            &bottom_right.resolve_for_output(640, 360, None),
            640,
            360,
        )
        .unwrap();
    let bb_tl = alpha_bounding_box(&f_tl).unwrap();
    let bb_br = alpha_bounding_box(&f_br).unwrap();
    // Safe left/top on 640x360 with 10%/12% is (64, 43); safe right/bottom is (576, 317).
    assert!(bb_tl.0 >= 63 && bb_tl.1 >= 42 && bb_tl.2 < 320 && bb_tl.3 < 180);
    assert!(bb_br.0 > 320 && bb_br.1 > 180 && bb_br.2 <= 576 && bb_br.3 <= 318);
}

#[test]
fn role_slots_variants_and_gpu_cpu_parity_work_together() {
    let mut cpu = CpuCompositor::default();
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");

    let mut template = TemplateStyle::preset_sanctuary();
    // Enable chords and add a custom variant to verify variant resolution and chords slot.
    template.variants.push(AspectVariantStyle {
        aspect_ratio: "21:9".into(),
        reference_width: 2520,
        reference_height: 1080,
        max_lines: Some(3),
        safe_area: Some(SafeArea::led_wide()),
        roles: Some(RoleSlotsPatch {
            body: Some(SlotTextStylePatch {
                font_family: Some("Noto Serif".into()),
                size: Some(64.0),
                ..Default::default()
            }),
            ..Default::default()
        }),
    });
    let led_resolved = template.resolve_for_output(3840, 1080, None);
    assert_eq!(led_resolved.matched_variant.as_deref(), Some("32:9"));
    assert_eq!(led_resolved.max_lines, Some(2));
    let ultra_wide = template.resolve_for_output(2520, 1080, None);
    assert_eq!(ultra_wide.matched_variant.as_deref(), Some("21:9"));
    assert_eq!(ultra_wide.roles.body.font_family, "Noto Serif");

    let stage_json = TemplateStyle::preset_stage_monitor().to_json().unwrap();
    let slide_spec = SlideLayerSpec {
        title_text: "Amazing Grace · Verse 1".into(),
        reference_text: "John Newton (1779)".into(),
        text: "Amazing grace! How sweet the sound\nThat saved a wretch like me".into(),
        translation_text: "Oore-ọ̀fẹ́ àrà ọ̀tọ̀!".into(),
        footer_text: "Public Domain · CCLI 22025".into(),
        chords_json: r#"[{"line":0,"position":0,"text":"G"},{"line":0,"position":19,"text":"C"},{"line":1,"position":18,"text":"D"}]"#.into(),
        style_json: stage_json.clone(),
        ..Default::default()
    };

    let scene = Scene {
        revision: 42,
        outputs: vec![OutputScene {
            output_index: 0,
            resolution: Some(Resolution {
                width: 960,
                height: 540,
            }),
            layers: vec![LayerSpec {
                kind: LayerKind::Slide as i32,
                enabled: true,
                content: Some(layer_spec::Content::Slide(slide_spec.clone())),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    let cpu_frame = cpu.render_output(&scene, 0, 960, 540).unwrap();
    let gpu_frame = gpu.render_output(&scene, 0, 960, 540).unwrap();
    assert_eq!(cpu_frame.width, 960);
    assert!(alpha_sum(&cpu_frame) > 50_000);
    for (pa, pb) in gpu_frame
        .pixels
        .chunks_exact(4)
        .zip(cpu_frame.pixels.chunks_exact(4))
    {
        for c in 0..4 {
            assert!(
                pa[c].abs_diff(pb[c]) <= 1,
                "GPU/CPU template mismatch: gpu={pa:?} cpu={pb:?}"
            );
        }
    }

    // Verify PreloadHint warms the exact same templated cache key.
    let mut cpu_warm = CpuCompositor::default();
    let warm_slide = SlideLayerSpec {
        text: "Warm templated lyric".into(),
        style_json: TemplateStyle::preset_lower_third().to_json().unwrap(),
        ..Default::default()
    };
    cpu_warm
        .prerasterise_hints(&[PreloadHint {
            key: "tmpl".into(),
            output_index: 0,
            resolution: Some(Resolution {
                width: 640,
                height: 360,
            }),
            text: warm_slide.text.clone(),
            style_json: warm_slide.style_json.clone(),
            ..Default::default()
        }])
        .unwrap();
    let (h0, m0) = cpu_warm.cache_stats();
    cpu_warm
        .render_output(
            &Scene {
                revision: 2,
                outputs: vec![OutputScene {
                    output_index: 0,
                    layers: vec![LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Slide(warm_slide)),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
            0,
            640,
            360,
        )
        .unwrap();
    let (h1, m1) = cpu_warm.cache_stats();
    assert!(h1 > h0 && m1 == m0);
}
