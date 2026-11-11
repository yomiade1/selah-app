//! M3-A1: golden-frame suite covering every template style feature, all 8 transitions at
//! t = 0, 0.25, 0.5, 0.75, 1.0, and all 6 built-in effects (M3.8).
//! Run with `cargo test -p selah-testkit --features golden --test golden_m3`.
#![cfg(feature = "golden")]

use selah_proto::v1::{
    Color, ColorStop, CopyrightPlacement, EffectKind, EffectSpec, GradientKind, GradientSpec,
    LayerKind, LayerSpec, LyricLine, MediaLayerSpec, MessagesLayerSpec, OutputScene,
    PropsLayerSpec, Resolution, Scene, SectionKind, SlideLayerSpec, TestPattern,
    TransitionDirection, TransitionKind, layer_spec,
};
use selah_render::Compositor;
use selah_render::cpu::CpuCompositor;
use selah_render::fit::{
    FitTarget, SongFitInput, SongSectionInput, fit_song_slides, generated_song_slide_to_layer,
};
use selah_render::gpu::{AdapterChoice, GpuCompositor};
use selah_render::slide_elements::{
    ShapeKind, SlideElement, SlideElementKind, SlideElementsDocument,
};
use selah_render::template::{
    BackgroundBoxStyle, OutlineStyle, RoleSlots, SafeArea, ShadowStyle, SlotTextStyle,
    TemplateStyle, TextAlignH, TextAlignV,
};
use selah_render::transitions::RenderTransitionPlan;
use selah_testkit::golden::{self, Tolerance};

/// Skia text rasterisation uses DirectWrite on Windows, FreeType on Linux, and CoreText on macOS.
/// Style golden frames allow up to 2% subpixel edge anti-aliasing variance across OS rasterisers;
/// any missing glyph, dropped outline/shadow/card box, broken alignment, or wrong aspect layout
/// changes 5%–40% of pixels and fails immediately.
const TOLERANCE_STYLE: Tolerance = Tolerance {
    channel: 10,
    max_fraction: 0.02,
};

/// Transitions and effects use pure vector shapes and gradients (zero OS text rasteriser paths),
/// so cross-platform rendering stays deterministic within 0.2%.
const TOLERANCE_SHADER: Tolerance = Tolerance {
    channel: 3,
    max_fraction: 0.002,
};

const FIXED_CLOCK_MS: i64 = 1_700_000_000_000;

fn c(red: f32, green: f32, blue: f32, alpha: f32) -> Option<Color> {
    Some(Color {
        red,
        green,
        blue,
        alpha,
    })
}

fn gradient_media_layer(kind: GradientKind, angle: f32, stops: [([f32; 4], f32); 3]) -> LayerSpec {
    LayerSpec {
        kind: LayerKind::Media as i32,
        enabled: true,
        content: Some(layer_spec::Content::Media(MediaLayerSpec {
            gradient: Some(GradientSpec {
                kind: kind as i32,
                angle_degrees: angle,
                stops: stops
                    .into_iter()
                    .map(|(rgba, pos)| ColorStop {
                        color: c(rgba[0], rgba[1], rgba[2], rgba[3]),
                        position: pos,
                    })
                    .collect(),
            }),
            ..Default::default()
        })),
        ..Default::default()
    }
}

fn single_output_scene(revision: u64, w: u32, h: u32, layers: Vec<LayerSpec>) -> Scene {
    Scene {
        revision,
        outputs: vec![OutputScene {
            output_index: 0,
            resolution: Some(Resolution {
                width: w,
                height: h,
            }),
            layers,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn style_roles_decorations_scene(w: u32, h: u32) -> Scene {
    let style = TemplateStyle {
        schema_version: 1,
        name: "M3 Golden Full Features".into(),
        reference_height: 1080,
        safe_area: SafeArea {
            left: 0.08,
            top: 0.07,
            right: 0.08,
            bottom: 0.07,
        },
        roles: RoleSlots {
            title: SlotTextStyle {
                font_family: "Manrope".into(),
                size: 54.0,
                min_size: 28.0,
                weight: 800,
                color: [255, 224, 130, 255],
                outline: Some(OutlineStyle {
                    color: [10, 14, 24, 230],
                    width: 3.0,
                }),
                align_h: TextAlignH::Center,
                align_v: TextAlignV::Top,
                ..SlotTextStyle::default()
            },
            reference: SlotTextStyle {
                font_family: "Noto Serif".into(),
                size: 30.0,
                min_size: 18.0,
                weight: 600,
                color: [196, 224, 255, 245],
                align_h: TextAlignH::Center,
                align_v: TextAlignV::Top,
                ..SlotTextStyle::default()
            },
            body: SlotTextStyle {
                font_family: "Noto Serif".into(),
                size: 62.0,
                min_size: 26.0,
                weight: 700,
                color: [255, 255, 255, 255],
                outline: Some(OutlineStyle {
                    color: [24, 8, 16, 235],
                    width: 4.0,
                }),
                shadow: Some(ShadowStyle {
                    color: [0, 0, 0, 210],
                    offset_x: 4.0,
                    offset_y: 6.0,
                    blur: 8.0,
                }),
                background_box: Some(BackgroundBoxStyle {
                    color: [18, 34, 68, 225],
                    padding_x: 28.0,
                    padding_y: 18.0,
                    radius: 16.0,
                    full_width: false,
                    accent_bar_color: Some([255, 190, 46, 255]),
                    accent_bar_width: 10.0,
                }),
                line_height: 1.35,
                letter_spacing: 2.0,
                align_h: TextAlignH::Center,
                align_v: TextAlignV::Middle,
                ..SlotTextStyle::default()
            },
            chords: SlotTextStyle {
                font_family: "JetBrains Mono".into(),
                size: 32.0,
                min_size: 18.0,
                weight: 700,
                color: [255, 200, 64, 255],
                visible: true,
                ..RoleSlots::default_chords()
            },
            translation: SlotTextStyle {
                font_family: "Noto Sans".into(),
                size: 38.0,
                min_size: 20.0,
                weight: 500,
                color: [204, 236, 224, 245],
                visible: true,
                ..RoleSlots::default_translation()
            },
            footer: SlotTextStyle {
                font_family: "Inter".into(),
                size: 24.0,
                min_size: 14.0,
                weight: 500,
                color: [216, 225, 236, 230],
                align_h: TextAlignH::Center,
                align_v: TextAlignV::Bottom,
                ..RoleSlots::default_footer()
            },
        },
        variants: Vec::new(),
    };

    let bg = gradient_media_layer(
        GradientKind::Linear,
        135.0,
        [
            ([0.06, 0.11, 0.24, 1.0], 0.0),
            ([0.11, 0.22, 0.38, 1.0], 0.55),
            ([0.18, 0.12, 0.28, 1.0], 1.0),
        ],
    );
    let slide = LayerSpec {
        kind: LayerKind::Slide as i32,
        enabled: true,
        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
            title_text: "Amazing Grace · Verse 1".into(),
            reference_text: "Psalm 23 · John Newton".into(),
            text: "How sweet the sound\nThat saved a wretch like me".into(),
            translation_text: "Wondrous mercy, endless song".into(),
            footer_text: "Public Domain · CCLI 22025".into(),
            chords_json: r#"[{"line":0,"position":0,"text":"G"},{"line":0,"position":14,"text":"C"},{"line":1,"position":11,"text":"D"}]"#.into(),
            style_json: style.to_json().unwrap(),
            ..Default::default()
        })),
        ..Default::default()
    };
    single_output_scene(1, w, h, vec![bg, slide])
}

fn style_alignments_safe_area_scene(w: u32, h: u32) -> Scene {
    let safe_area = SafeArea {
        left: 0.12,
        top: 0.10,
        right: 0.08,
        bottom: 0.12,
    };
    let style = TemplateStyle {
        schema_version: 1,
        name: "Alignments & Safe Area".into(),
        reference_height: 720,
        safe_area,
        roles: RoleSlots {
            title: SlotTextStyle {
                font_family: "Manrope".into(),
                size: 40.0,
                weight: 700,
                color: [255, 224, 130, 255],
                align_h: TextAlignH::Left,
                align_v: TextAlignV::Top,
                ..SlotTextStyle::default()
            },
            body: SlotTextStyle {
                font_family: "Noto Sans".into(),
                size: 38.0,
                weight: 600,
                color: [255, 255, 255, 255],
                background_box: Some(BackgroundBoxStyle {
                    color: [16, 44, 72, 235],
                    padding_x: 20.0,
                    padding_y: 16.0,
                    radius: 8.0,
                    full_width: true,
                    accent_bar_color: Some([46, 204, 113, 255]),
                    accent_bar_width: 8.0,
                }),
                align_h: TextAlignH::Justify,
                align_v: TextAlignV::Middle,
                ..SlotTextStyle::default()
            },
            footer: SlotTextStyle {
                font_family: "Inter".into(),
                size: 24.0,
                weight: 500,
                color: [186, 226, 255, 255],
                align_h: TextAlignH::Right,
                align_v: TextAlignV::Bottom,
                ..SlotTextStyle::default()
            },
            ..Default::default()
        },
        variants: Vec::new(),
    };

    // Draw safe-area outline border on Props layer so visual review confirms safe margins.
    let safe_guide = SlideElementsDocument {
        elements: vec![SlideElement {
            id: "safe-border".into(),
            kind: SlideElementKind::Shape,
            x: safe_area.left,
            y: safe_area.top,
            width: 1.0 - safe_area.left - safe_area.right,
            height: 1.0 - safe_area.top - safe_area.bottom,
            z_index: 1,
            shape_kind: ShapeKind::Rect,
            fill_color: [0, 0, 0, 0],
            stroke_color: Some([64, 196, 255, 190]),
            stroke_width: 2.0,
            ..Default::default()
        }],
        ..Default::default()
    };

    let bg = gradient_media_layer(
        GradientKind::Linear,
        90.0,
        [
            ([0.07, 0.09, 0.14, 1.0], 0.0),
            ([0.09, 0.14, 0.22, 1.0], 0.5),
            ([0.06, 0.09, 0.14, 1.0], 1.0),
        ],
    );
    let slide = LayerSpec {
        kind: LayerKind::Slide as i32,
        enabled: true,
        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
            title_text: "Top-Left Title".into(),
            text: "Full-width justified body block honoring custom left and right safe margins across words."
                .into(),
            footer_text: "Bottom-Right Footer".into(),
            style_json: style.to_json().unwrap(),
            ..Default::default()
        })),
        ..Default::default()
    };
    let props = LayerSpec {
        kind: LayerKind::Props as i32,
        enabled: true,
        content: Some(layer_spec::Content::Props(PropsLayerSpec {
            elements_json: safe_guide.to_json().unwrap(),
            ..Default::default()
        })),
        ..Default::default()
    };
    single_output_scene(2, w, h, vec![bg, slide, props])
}

fn style_min_size_shrink_scene(w: u32, h: u32) -> Scene {
    let mut style = TemplateStyle {
        schema_version: 1,
        name: "Metric Shrink To Min Size".into(),
        reference_height: 720,
        safe_area: SafeArea {
            left: 0.10,
            top: 0.12,
            right: 0.10,
            bottom: 0.12,
        },
        roles: RoleSlots {
            body: SlotTextStyle {
                font_family: "Noto Sans".into(),
                size: 96.0,
                min_size: 28.0,
                weight: 600,
                color: [255, 255, 255, 255],
                outline: Some(OutlineStyle {
                    color: [0, 0, 0, 210],
                    width: 2.0,
                }),
                align_h: TextAlignH::Center,
                align_v: TextAlignV::Middle,
                max_lines: Some(6),
                ..SlotTextStyle::default()
            },
            ..Default::default()
        },
        variants: Vec::new(),
    };
    let fitted = fit_song_slides(
        &SongFitInput {
            sections: vec![SongSectionInput {
                section_id: None,
                section_label: "Verse".into(),
                section_kind: SectionKind::Verse as i32,
                section_color: None,
                section_hotkey: "V".into(),
                lines: vec![
                    LyricLine {
                        text: "When we have been there ten thousand years,".into(),
                        chords: Vec::new(),
                    },
                    LyricLine {
                        text: "Bright shining as the glorious morning sun,".into(),
                        chords: Vec::new(),
                    },
                    LyricLine {
                        text: "We have no less days to sing His endless praise".into(),
                        chords: Vec::new(),
                    },
                    LyricLine {
                        text: "Than when our grateful song had first begun.".into(),
                        chords: Vec::new(),
                    },
                ],
                translation_lines: Vec::new(),
            }],
            title_lines: None,
            copyright_line: String::new(),
            copyright_placement: CopyrightPlacement::None,
            blank_slide_at_end: false,
            max_lines: 6,
        },
        &[FitTarget {
            output_index: 0,
            output_name: "Main".into(),
            width: w,
            height: h,
            style: style.clone(),
            max_lines_override: Some(6),
        }],
    )
    .expect("fit_song_slides");

    let (report, slides) = &fitted.per_output[0];
    let baseline_out_size = 96.0 * (h as f32 / 720.0);
    assert!(
        report.fits && !report.overflows_safe_area && report.applied_size + 0.5 < baseline_out_size,
        "expected shrunk fitting report: {report:?}"
    );
    style.reference_height = h;
    style.roles.body.size = report.applied_size;
    let fitted_style_json = style.to_json().unwrap();
    let slide_spec = generated_song_slide_to_layer(&slides[0], Some(&fitted_style_json));

    let bg = gradient_media_layer(
        GradientKind::Radial,
        0.0,
        [
            ([0.12, 0.24, 0.34, 1.0], 0.0),
            ([0.08, 0.14, 0.24, 1.0], 0.6),
            ([0.04, 0.07, 0.12, 1.0], 1.0),
        ],
    );
    let slide = LayerSpec {
        kind: LayerKind::Slide as i32,
        enabled: true,
        content: Some(layer_spec::Content::Slide(slide_spec)),
        ..Default::default()
    };
    single_output_scene(3, w, h, vec![bg, slide])
}

fn style_all_fonts_and_weights_scene(w: u32, h: u32) -> Scene {
    let doc = SlideElementsDocument {
        reference_height: 720,
        elements: vec![
            SlideElement {
                id: "inter-100".into(),
                kind: SlideElementKind::Text,
                x: 0.06,
                y: 0.07,
                width: 0.42,
                height: 0.18,
                z_index: 1,
                text: "Inter 100 Thin".into(),
                font_family: "Inter".into(),
                font_size: 40.0,
                weight: 100,
                background_box: Some(BackgroundBoxStyle {
                    color: [22, 32, 54, 220],
                    padding_x: 12.0,
                    padding_y: 8.0,
                    radius: 8.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
            SlideElement {
                id: "manrope-400".into(),
                kind: SlideElementKind::Text,
                x: 0.52,
                y: 0.07,
                width: 0.42,
                height: 0.18,
                z_index: 2,
                text: "Manrope 400".into(),
                font_family: "Manrope".into(),
                font_size: 40.0,
                weight: 400,
                color: [255, 224, 130, 255],
                background_box: Some(BackgroundBoxStyle {
                    color: [22, 32, 54, 220],
                    padding_x: 12.0,
                    padding_y: 8.0,
                    radius: 8.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
            SlideElement {
                id: "mono-700".into(),
                kind: SlideElementKind::Text,
                x: 0.06,
                y: 0.29,
                width: 0.42,
                height: 0.18,
                z_index: 3,
                text: "JetBrains 700".into(),
                font_family: "JetBrains Mono".into(),
                font_size: 36.0,
                weight: 700,
                color: [164, 240, 196, 255],
                background_box: Some(BackgroundBoxStyle {
                    color: [22, 32, 54, 220],
                    padding_x: 12.0,
                    padding_y: 8.0,
                    radius: 8.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
            SlideElement {
                id: "serif-900".into(),
                kind: SlideElementKind::Text,
                x: 0.52,
                y: 0.29,
                width: 0.42,
                height: 0.18,
                z_index: 4,
                text: "Noto Serif 900".into(),
                font_family: "Noto Serif".into(),
                font_size: 38.0,
                weight: 900,
                color: [255, 192, 168, 255],
                background_box: Some(BackgroundBoxStyle {
                    color: [22, 32, 54, 220],
                    padding_x: 12.0,
                    padding_y: 8.0,
                    radius: 8.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
            SlideElement {
                id: "notosans-500".into(),
                kind: SlideElementKind::Text,
                x: 0.06,
                y: 0.52,
                width: 0.42,
                height: 0.18,
                z_index: 5,
                text: "Noto Sans · Yorùbá Ọlọ́run".into(),
                font_family: "Noto Sans".into(),
                font_size: 32.0,
                weight: 500,
                background_box: Some(BackgroundBoxStyle {
                    color: [22, 32, 54, 220],
                    padding_x: 12.0,
                    padding_y: 8.0,
                    radius: 8.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
            SlideElement {
                id: "kr-700".into(),
                kind: SlideElementKind::Text,
                x: 0.52,
                y: 0.52,
                width: 0.42,
                height: 0.18,
                z_index: 6,
                text: "주님은 나의 목자".into(),
                font_family: "Noto Sans KR".into(),
                font_size: 38.0,
                weight: 700,
                color: [190, 224, 255, 255],
                background_box: Some(BackgroundBoxStyle {
                    color: [22, 32, 54, 220],
                    padding_x: 12.0,
                    padding_y: 8.0,
                    radius: 8.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
            SlideElement {
                id: "ar-700".into(),
                kind: SlideElementKind::Text,
                x: 0.20,
                y: 0.74,
                width: 0.60,
                height: 0.18,
                z_index: 7,
                text: "الرب راعيّ فلا يعوزني شيء".into(),
                font_family: "Noto Naskh Arabic".into(),
                font_size: 40.0,
                weight: 700,
                color: [255, 238, 180, 255],
                background_box: Some(BackgroundBoxStyle {
                    color: [22, 32, 54, 220],
                    padding_x: 16.0,
                    padding_y: 8.0,
                    radius: 8.0,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let bg = gradient_media_layer(
        GradientKind::Linear,
        0.0,
        [
            ([0.05, 0.08, 0.14, 1.0], 0.0),
            ([0.08, 0.12, 0.20, 1.0], 0.5),
            ([0.05, 0.08, 0.14, 1.0], 1.0),
        ],
    );
    let slide = LayerSpec {
        kind: LayerKind::Slide as i32,
        enabled: true,
        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
            elements_json: doc.to_json().unwrap(),
            ..Default::default()
        })),
        ..Default::default()
    };
    single_output_scene(4, w, h, vec![bg, slide])
}

fn style_custom_elements_scene(w: u32, h: u32) -> Scene {
    let doc = SlideElementsDocument {
        reference_height: 720,
        elements: vec![
            SlideElement {
                id: "panel".into(),
                kind: SlideElementKind::Shape,
                x: 0.08,
                y: 0.12,
                width: 0.84,
                height: 0.64,
                z_index: 1,
                shape_kind: ShapeKind::RoundedRect,
                fill_color: [16, 36, 66, 235],
                stroke_color: Some([255, 190, 46, 255]),
                stroke_width: 4.0,
                corner_radius: 18.0,
                ..Default::default()
            },
            SlideElement {
                id: "badge-circle".into(),
                kind: SlideElementKind::Shape,
                x: 0.12,
                y: 0.18,
                width: 0.11,
                height: 0.20,
                z_index: 2,
                shape_kind: ShapeKind::Ellipse,
                fill_color: [232, 92, 64, 255],
                stroke_color: Some([255, 255, 255, 230]),
                stroke_width: 3.0,
                ..Default::default()
            },
            SlideElement {
                id: "divider".into(),
                kind: SlideElementKind::Shape,
                x: 0.12,
                y: 0.44,
                width: 0.76,
                height: 0.01,
                z_index: 3,
                shape_kind: ShapeKind::Line,
                stroke_color: Some([120, 184, 255, 220]),
                stroke_width: 3.0,
                ..Default::default()
            },
            SlideElement {
                id: "corner-rect".into(),
                kind: SlideElementKind::Shape,
                x: 0.76,
                y: 0.20,
                width: 0.12,
                height: 0.16,
                z_index: 4,
                opacity: 0.85,
                shape_kind: ShapeKind::Rect,
                fill_color: [46, 196, 182, 255],
                ..Default::default()
            },
            SlideElement {
                id: "headline".into(),
                kind: SlideElementKind::Text,
                x: 0.26,
                y: 0.18,
                width: 0.48,
                height: 0.20,
                z_index: 5,
                text: "Sunday Worship".into(),
                font_family: "Manrope".into(),
                font_size: 46.0,
                weight: 800,
                color: [255, 255, 255, 255],
                align_h: TextAlignH::Left,
                align_v: TextAlignV::Middle,
                ..Default::default()
            },
            SlideElement {
                id: "clock-timer".into(),
                kind: SlideElementKind::Text,
                x: 0.12,
                y: 0.50,
                width: 0.76,
                height: 0.20,
                z_index: 6,
                text: "Clock {clock} · Countdown {timer}".into(),
                font_family: "JetBrains Mono".into(),
                font_size: 38.0,
                weight: 700,
                color: [255, 214, 102, 255],
                clock_show_seconds: true,
                clock_24h: true,
                timer_target_unix_ms: Some(FIXED_CLOCK_MS + 325_000),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let bg = gradient_media_layer(
        GradientKind::Linear,
        45.0,
        [
            ([0.08, 0.12, 0.20, 1.0], 0.0),
            ([0.12, 0.18, 0.30, 1.0], 0.5),
            ([0.06, 0.10, 0.18, 1.0], 1.0),
        ],
    );
    let slide = LayerSpec {
        kind: LayerKind::Slide as i32,
        enabled: true,
        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
            elements_json: doc.to_json().unwrap(),
            ..Default::default()
        })),
        ..Default::default()
    };
    let props = LayerSpec {
        kind: LayerKind::Props as i32,
        enabled: true,
        content: Some(layer_spec::Content::Props(PropsLayerSpec {
            label: "LIVE".into(),
            background_color: c(0.82, 0.18, 0.20, 1.0),
            text_color: c(1.0, 1.0, 1.0, 1.0),
            ..Default::default()
        })),
        ..Default::default()
    };
    let messages = LayerSpec {
        kind: LayerKind::Messages as i32,
        enabled: true,
        content: Some(layer_spec::Content::Messages(MessagesLayerSpec {
            text: "Nursery Check-In · Room 2B".into(),
            background_color: c(0.08, 0.18, 0.34, 0.94),
            text_color: c(1.0, 1.0, 1.0, 1.0),
            ..Default::default()
        })),
        ..Default::default()
    };
    single_output_scene(5, w, h, vec![bg, slide, props, messages])
}

/// Pure geometric/gradient "From" & "To" scenes (no glyph text) for cross-platform transition goldens.
fn transition_pair_scenes(w: u32, h: u32) -> (Scene, Scene) {
    let from_doc = SlideElementsDocument {
        elements: vec![
            SlideElement {
                id: "from-card".into(),
                kind: SlideElementKind::Shape,
                x: 0.08,
                y: 0.15,
                width: 0.42,
                height: 0.70,
                z_index: 1,
                shape_kind: ShapeKind::RoundedRect,
                fill_color: [236, 154, 41, 255],
                stroke_color: Some([255, 245, 220, 255]),
                stroke_width: 4.0,
                corner_radius: 16.0,
                ..Default::default()
            },
            SlideElement {
                id: "from-circle".into(),
                kind: SlideElementKind::Shape,
                x: 0.58,
                y: 0.22,
                width: 0.28,
                height: 0.50,
                z_index: 2,
                shape_kind: ShapeKind::Ellipse,
                fill_color: [214, 58, 72, 255],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let to_doc = SlideElementsDocument {
        elements: vec![
            SlideElement {
                id: "to-band".into(),
                kind: SlideElementKind::Shape,
                x: 0.06,
                y: 0.22,
                width: 0.88,
                height: 0.22,
                z_index: 1,
                shape_kind: ShapeKind::Rect,
                fill_color: [248, 250, 252, 245],
                ..Default::default()
            },
            SlideElement {
                id: "to-badge".into(),
                kind: SlideElementKind::Shape,
                x: 0.44,
                y: 0.52,
                width: 0.46,
                height: 0.34,
                z_index: 2,
                shape_kind: ShapeKind::RoundedRect,
                fill_color: [34, 152, 232, 255],
                stroke_color: Some([255, 214, 70, 255]),
                stroke_width: 4.0,
                corner_radius: 14.0,
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let from_scene = single_output_scene(
        10,
        w,
        h,
        vec![
            gradient_media_layer(
                GradientKind::Linear,
                135.0,
                [
                    ([0.08, 0.16, 0.48, 1.0], 0.0),
                    ([0.12, 0.34, 0.62, 1.0], 0.5),
                    ([0.05, 0.10, 0.28, 1.0], 1.0),
                ],
            ),
            LayerSpec {
                kind: LayerKind::Slide as i32,
                enabled: true,
                content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                    elements_json: from_doc.to_json().unwrap(),
                    ..Default::default()
                })),
                ..Default::default()
            },
        ],
    );
    let to_scene = single_output_scene(
        11,
        w,
        h,
        vec![
            gradient_media_layer(
                GradientKind::Linear,
                45.0,
                [
                    ([0.06, 0.30, 0.22, 1.0], 0.0),
                    ([0.14, 0.48, 0.34, 1.0], 0.5),
                    ([0.04, 0.18, 0.14, 1.0], 1.0),
                ],
            ),
            LayerSpec {
                kind: LayerKind::Slide as i32,
                enabled: true,
                content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                    elements_json: to_doc.to_json().unwrap(),
                    ..Default::default()
                })),
                ..Default::default()
            },
        ],
    );
    (from_scene, to_scene)
}

fn effect_scenes(w: u32, h: u32) -> Vec<(&'static str, Scene)> {
    let key_subject_doc = SlideElementsDocument {
        elements: vec![
            // Chroma green full card canvas
            SlideElement {
                id: "chroma-bg".into(),
                kind: SlideElementKind::Shape,
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                z_index: 1,
                shape_kind: ShapeKind::Rect,
                fill_color: [25, 229, 51, 255],
                ..Default::default()
            },
            //Foreground emblem to retain after green key
            SlideElement {
                id: "emblem-card".into(),
                kind: SlideElementKind::Shape,
                x: 0.25,
                y: 0.20,
                width: 0.50,
                height: 0.60,
                z_index: 2,
                shape_kind: ShapeKind::RoundedRect,
                fill_color: [24, 92, 212, 255],
                stroke_color: Some([255, 210, 64, 255]),
                stroke_width: 5.0,
                corner_radius: 16.0,
                ..Default::default()
            },
            SlideElement {
                id: "emblem-dot".into(),
                kind: SlideElementKind::Shape,
                x: 0.41,
                y: 0.34,
                width: 0.18,
                height: 0.32,
                z_index: 3,
                shape_kind: ShapeKind::Ellipse,
                fill_color: [255, 210, 64, 255],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    let mask_subject_doc = SlideElementsDocument {
        elements: vec![
            SlideElement {
                id: "coral-top".into(),
                kind: SlideElementKind::Shape,
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 0.50,
                z_index: 1,
                shape_kind: ShapeKind::Rect,
                fill_color: [238, 102, 68, 255],
                ..Default::default()
            },
            SlideElement {
                id: "gold-bottom".into(),
                kind: SlideElementKind::Shape,
                x: 0.0,
                y: 0.50,
                width: 1.0,
                height: 0.50,
                z_index: 2,
                shape_kind: ShapeKind::Rect,
                fill_color: [244, 198, 54, 255],
                ..Default::default()
            },
            SlideElement {
                id: "center-badge".into(),
                kind: SlideElementKind::Shape,
                x: 0.35,
                y: 0.32,
                width: 0.30,
                height: 0.36,
                z_index: 3,
                shape_kind: ShapeKind::Ellipse,
                fill_color: [38, 168, 152, 255],
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    vec![
        (
            "m3-fx-blur",
            Scene {
                revision: 21,
                outputs: vec![OutputScene {
                    output_index: 0,
                    resolution: Some(Resolution {
                        width: w,
                        height: h,
                    }),
                    test_pattern: TestPattern::SmpteBars as i32,
                    effects: vec![EffectSpec {
                        kind: EffectKind::Blur as i32,
                        enabled: true,
                        intensity: 1.0,
                        params_json: r#"{"radius":5.0}"#.into(),
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
        ),
        (
            "m3-fx-colour-adjust",
            Scene {
                revision: 22,
                outputs: vec![OutputScene {
                    output_index: 0,
                    resolution: Some(Resolution {
                        width: w,
                        height: h,
                    }),
                    test_pattern: TestPattern::SmpteBars as i32,
                    effects: vec![EffectSpec {
                        kind: EffectKind::ColourAdjust as i32,
                        enabled: true,
                        intensity: 1.0,
                        params_json: r#"{"brightness":0.12,"contrast":1.35,"saturation":0.35,"hue":55}"#
                            .into(),
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
        ),
        (
            "m3-fx-vignette",
            Scene {
                revision: 23,
                outputs: vec![OutputScene {
                    output_index: 0,
                    resolution: Some(Resolution {
                        width: w,
                        height: h,
                    }),
                    layers: vec![gradient_media_layer(
                        GradientKind::Linear,
                        45.0,
                        [
                            ([0.95, 0.72, 0.28, 1.0], 0.0),
                            ([0.85, 0.36, 0.24, 1.0], 0.5),
                            ([0.28, 0.42, 0.88, 1.0], 1.0),
                        ],
                    )],
                    effects: vec![EffectSpec {
                        kind: EffectKind::Vignette as i32,
                        enabled: true,
                        intensity: 1.0,
                        params_json:
                            r##"{"radius":0.30,"softness":0.55,"strength":0.92,"color":"#060812"}"##
                                .into(),
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
        ),
        (
            "m3-fx-background-blur",
            Scene {
                revision: 24,
                outputs: vec![OutputScene {
                    output_index: 0,
                    resolution: Some(Resolution {
                        width: w,
                        height: h,
                    }),
                    test_pattern: TestPattern::SmpteBars as i32,
                    layers: vec![LayerSpec {
                        kind: LayerKind::Props as i32,
                        enabled: true,
                        effects: vec![EffectSpec {
                            kind: EffectKind::BackgroundBlur as i32,
                            enabled: true,
                            intensity: 1.0,
                            params_json:
                                r##"{"radius":8.0,"region":[0.15,0.20,0.70,0.60],"tint_color":"#ffffff38"}"##
                                    .into(),
                        }],
                        content: Some(layer_spec::Content::Props(PropsLayerSpec {
                            background_color: c(1.0, 1.0, 1.0, 0.14),
                            ..Default::default()
                        })),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
        ),
        (
            "m3-fx-colour-key",
            single_output_scene(
                25,
                w,
                h,
                vec![
                    gradient_media_layer(
                        GradientKind::Linear,
                        135.0,
                        [
                            ([0.92, 0.36, 0.22, 1.0], 0.0),
                            ([0.64, 0.20, 0.54, 1.0], 0.5),
                            ([0.16, 0.14, 0.38, 1.0], 1.0),
                        ],
                    ),
                    LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        effects: vec![EffectSpec {
                            kind: EffectKind::ColourKey as i32,
                            enabled: true,
                            intensity: 1.0,
                            params_json:
                                r##"{"color":"#19e533","tolerance":0.22,"softness":0.06,"spill_suppression":0.4}"##
                                    .into(),
                        }],
                        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                            elements_json: key_subject_doc.to_json().unwrap(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                ],
            ),
        ),
        (
            "m3-fx-mask",
            single_output_scene(
                26,
                w,
                h,
                vec![
                    LayerSpec {
                        kind: LayerKind::Media as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Media(MediaLayerSpec {
                            solid_color: c(0.06, 0.10, 0.18, 1.0),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                    LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        effects: vec![EffectSpec {
                            kind: EffectKind::Mask as i32,
                            enabled: true,
                            intensity: 1.0,
                            params_json:
                                r#"{"shape":"rounded_rect","bounds":[0.18,0.16,0.64,0.68],"corner_radius":0.10,"feather":0.035}"#
                                    .into(),
                        }],
                        content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                            elements_json: mask_subject_doc.to_json().unwrap(),
                            ..Default::default()
                        })),
                        ..Default::default()
                    },
                ],
            ),
        ),
    ]
}

#[test]
fn style_roles_decorations_alignments_and_safe_area_match_golden() {
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");
    gpu.set_clock_unix_ms(Some(FIXED_CLOCK_MS));

    let frames = [
        (
            "m3-style-roles-decorations",
            style_roles_decorations_scene(960, 540),
            960_u32,
            540_u32,
        ),
        (
            "m3-style-alignments-safe-area",
            style_alignments_safe_area_scene(640, 360),
            640,
            360,
        ),
        (
            "m3-style-min-size-shrink",
            style_min_size_shrink_scene(640, 360),
            640,
            360,
        ),
        (
            "m3-style-custom-elements",
            style_custom_elements_scene(640, 360),
            640,
            360,
        ),
    ];

    let mut failures = Vec::new();
    for (name, scene, w, h) in frames {
        let frame = gpu.render_output(&scene, 0, w, h).unwrap();
        if let Err(err) = golden::check(name, &frame, TOLERANCE_STYLE) {
            failures.push(err.to_string());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn style_all_seven_fonts_and_weights_match_golden() {
    let mut renderer = selah_render::text::TextRenderer::bundled().unwrap();
    for (family, sample, weight) in [
        ("Inter", "Inter 100 Thin", 100),
        ("Manrope", "Manrope 400", 400),
        ("JetBrains Mono", "JetBrains 700", 700),
        ("Noto Serif", "Noto Serif 900", 900),
        ("Noto Sans", "Yorùbá Ọlọ́run", 500),
        ("Noto Sans KR", "주님은 나의 목자", 700),
        ("Noto Naskh Arabic", "الرب راعيّ فلا يعوزني شيء", 700),
    ] {
        assert_eq!(
            renderer.unresolved_glyphs_for(sample, family, weight),
            0,
            "{family} weight {weight} has unresolved glyphs for '{sample}'"
        );
    }

    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");
    let frame = gpu
        .render_output(&style_all_fonts_and_weights_scene(640, 360), 0, 640, 360)
        .unwrap();
    golden::check("m3-style-fonts-weights", &frame, TOLERANCE_STYLE).unwrap();
}

#[test]
fn style_aspect_ratio_variants_match_golden() {
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");
    let sanctuary = TemplateStyle::preset_sanctuary();
    let style_json = sanctuary.to_json().unwrap();
    let warm_serif = TemplateStyle::preset_warm_serif().to_json().unwrap();

    let aspect_cases = [
        (
            "m3-style-aspect-32x9",
            640_u32,
            180_u32,
            Some("32:9"),
            style_json.clone(),
        ),
        (
            "m3-style-aspect-4x3",
            480,
            360,
            Some("4:3"),
            style_json.clone(),
        ),
        ("m3-style-aspect-9x16", 270, 480, Some("9:16"), style_json),
        ("m3-style-preset-warm-serif", 640, 360, None, warm_serif),
    ];

    let mut failures = Vec::new();
    for (name, w, h, expected_variant, sjson) in aspect_cases {
        if let Some(variant) = expected_variant {
            let resolved = sanctuary.resolve_for_output(w, h, None);
            assert_eq!(
                resolved.matched_variant.as_deref(),
                Some(variant),
                "{name} did not select aspect variant {variant}"
            );
        }
        let scene = single_output_scene(
            30,
            w,
            h,
            vec![
                gradient_media_layer(
                    GradientKind::Linear,
                    120.0,
                    [
                        ([0.08, 0.14, 0.28, 1.0], 0.0),
                        ([0.14, 0.24, 0.42, 1.0], 0.5),
                        ([0.06, 0.10, 0.18, 1.0], 1.0),
                    ],
                ),
                LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        title_text: "Psalm 23".into(),
                        text: "The Lord is my shepherd,\nI shall not want.".into(),
                        footer_text: "Public Domain".into(),
                        style_json: sjson,
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            ],
        );
        let frame = gpu.render_output(&scene, 0, w, h).unwrap();
        if let Err(err) = golden::check(name, &frame, TOLERANCE_STYLE) {
            failures.push(err.to_string());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn all_eight_transitions_at_five_steps_match_golden() {
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");
    let (w, h) = (320_u32, 180_u32);
    let (from_scene, to_scene) = transition_pair_scenes(w, h);

    let transitions = [
        ("cut", TransitionKind::Cut),
        ("fade", TransitionKind::Fade),
        ("dissolve", TransitionKind::Dissolve),
        ("fade-through-black", TransitionKind::FadeThroughBlack),
        ("push", TransitionKind::Push),
        ("wipe", TransitionKind::Wipe),
        ("zoom", TransitionKind::Zoom),
        ("slide", TransitionKind::Slide),
    ];
    let steps = [
        ("t0.00", 0.0_f32),
        ("t0.25", 0.25),
        ("t0.50", 0.50),
        ("t0.75", 0.75),
        ("t1.00", 1.0),
    ];

    let mut failures = Vec::new();
    for (tr_name, kind) in transitions {
        for (step_name, progress) in steps {
            let plan = RenderTransitionPlan {
                output_transition: Some((
                    from_scene.clone(),
                    kind,
                    TransitionDirection::Left,
                    progress,
                )),
                layer_transitions: Vec::new(),
            };
            let frame = gpu
                .render_output_with_plan(&to_scene, 0, w, h, &plan)
                .unwrap();
            let golden_name = format!("m3-tr-{tr_name}-{step_name}");
            if let Err(err) = golden::check(&golden_name, &frame, TOLERANCE_SHADER) {
                failures.push(err.to_string());
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn all_six_built_in_effects_match_golden() {
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");
    let (w, h) = (320_u32, 180_u32);
    let mut failures = Vec::new();
    for (name, scene) in effect_scenes(w, h) {
        let frame = gpu.render_output(&scene, 0, w, h).unwrap();
        if let Err(err) = golden::check(name, &frame, TOLERANCE_SHADER) {
            failures.push(err.to_string());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn m3_golden_gpu_and_cpu_parity_agree() {
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");
    let mut cpu = CpuCompositor::default();
    gpu.set_clock_unix_ms(Some(FIXED_CLOCK_MS));
    cpu.set_clock_unix_ms(Some(FIXED_CLOCK_MS));

    let effect_tol = Tolerance {
        channel: 6,
        max_fraction: 0.002,
    };

    for (name, scene, w, h) in [
        (
            "roles",
            style_roles_decorations_scene(640, 360),
            640_u32,
            360_u32,
        ),
        ("elements", style_custom_elements_scene(640, 360), 640, 360),
    ] {
        let gf = gpu.render_output(&scene, 0, w, h).unwrap();
        let cf = cpu.render_output(&scene, 0, w, h).unwrap();
        let diff = golden::compare(&gf, &cf, effect_tol).unwrap();
        assert!(
            diff.fraction() <= effect_tol.max_fraction,
            "{name} GPU/CPU mismatch: {diff:?}"
        );
    }

    let (from_scene, to_scene) = transition_pair_scenes(320, 180);
    for kind in [
        TransitionKind::Cut,
        TransitionKind::Fade,
        TransitionKind::Dissolve,
        TransitionKind::FadeThroughBlack,
        TransitionKind::Push,
        TransitionKind::Wipe,
        TransitionKind::Zoom,
        TransitionKind::Slide,
    ] {
        let plan = RenderTransitionPlan {
            output_transition: Some((from_scene.clone(), kind, TransitionDirection::Left, 0.5)),
            layer_transitions: Vec::new(),
        };
        let gf = gpu
            .render_output_with_plan(&to_scene, 0, 320, 180, &plan)
            .unwrap();
        let cf = cpu
            .render_output_with_plan(&to_scene, 0, 320, 180, &plan)
            .unwrap();
        let diff = golden::compare(&gf, &cf, effect_tol).unwrap();
        assert!(
            diff.fraction() <= effect_tol.max_fraction,
            "transition {kind:?} GPU/CPU mismatch: {diff:?}"
        );
    }

    for (name, scene) in effect_scenes(320, 180) {
        let gf = gpu.render_output(&scene, 0, 320, 180).unwrap();
        let cf = cpu.render_output(&scene, 0, 320, 180).unwrap();
        let diff = golden::compare(&gf, &cf, effect_tol).unwrap();
        assert!(
            diff.fraction() <= effect_tol.max_fraction,
            "effect {name} GPU/CPU mismatch: {diff:?}"
        );
    }
}
