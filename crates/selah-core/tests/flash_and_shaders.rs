//! M3.5 / M3-A8 / SAFE-04 / SEC-03 / SEC-04 core service integration tests:
//! closed built-in shader chain enforcement, offline WCAG 2.3.1 media scanning,
//! `.selah` flash metadata round-trip, `AnalyzeFlashRisk`, and `OutputFitReport` propagation.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use selah_core::library::new_id;
use selah_core::{CoreService, Outcome};
use selah_proto::v1::*;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ui_cmd(id: u64, action: command::Action) -> Envelope {
    Envelope {
        id,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "local-owner".into(),
            device_id: "flash-test".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Command(Command {
            action: Some(action),
            undo_group: None,
        })),
    }
}

fn ui_query(id: u64, request: query::Request) -> Envelope {
    Envelope {
        id,
        correlation_id: 0,
        actor: Some(Actor {
            user_id: "local-owner".into(),
            device_id: "flash-test".into(),
            origin: ActorOrigin::Ui as i32,
        }),
        body: Some(envelope::Body::Query(Query {
            request: Some(request),
        })),
    }
}

fn unwrap_reply(outcome: Outcome) -> reply::Result {
    match outcome.reply.and_then(|e| e.body) {
        Some(envelope::Body::Reply(r)) => r.result.expect("reply missing result"),
        Some(envelope::Body::Error(err)) => {
            panic!(
                "unexpected error {} ({}): fix={}",
                err.code, err.message_key, err.fix_article_id
            )
        }
        other => panic!("unexpected outcome: {other:?}"),
    }
}

fn expect_error_code(outcome: Outcome) -> String {
    match outcome.reply.and_then(|e| e.body) {
        Some(envelope::Body::Error(err)) => err.code,
        other => panic!("expected error, got {other:?}"),
    }
}

fn scene_with_effects(
    output_effects: Vec<EffectSpec>,
    layer_effects: Vec<EffectSpec>,
    inner_media_effects: Vec<EffectSpec>,
    inner_slide_effects: Vec<EffectSpec>,
) -> Scene {
    Scene {
        revision: 1,
        outputs: vec![OutputScene {
            output_index: 0,
            role: OutputRole::Main as i32,
            resolution: Some(Resolution {
                width: 640,
                height: 360,
            }),
            effects: output_effects,
            layers: vec![
                LayerSpec {
                    kind: LayerKind::Media as i32,
                    enabled: true,
                    effects: layer_effects,
                    content: Some(layer_spec::Content::Media(MediaLayerSpec {
                        solid_color: Some(Color {
                            red: 0.1,
                            green: 0.2,
                            blue: 0.4,
                            alpha: 1.0,
                        }),
                        effects: inner_media_effects,
                        ..Default::default()
                    })),
                    ..Default::default()
                },
                LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        text: "Grace".into(),
                        effects: inner_slide_effects,
                        ..Default::default()
                    })),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn custom_and_malformed_shader_chains_are_refused_on_present_and_stage() {
    // sec: SEC-03, SEC-04
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);

    let forbidden_cases = [
        EffectSpec {
            kind: EffectKind::Blur as i32,
            enabled: true,
            intensity: 1.0,
            params_json: r#"{"wgsl":"@fragment fn fs_main() {}"}"#.into(),
        },
        EffectSpec {
            kind: EffectKind::ColourAdjust as i32,
            enabled: false, // Even disabled stages are inspected for custom shader code.
            intensity: 1.0,
            params_json: r#"{"nested":[{"entry_point":"fs_main","shader":"void main(){}"}]}"#
                .into(),
        },
        EffectSpec {
            kind: EffectKind::Vignette as i32,
            enabled: true,
            intensity: 1.0,
            params_json: r#"{"script":"return 1;"}"#.into(),
        },
        EffectSpec {
            kind: EffectKind::Unspecified as i32,
            enabled: true,
            intensity: 1.0,
            params_json: String::new(),
        },
        EffectSpec {
            kind: EffectKind::Mask as i32,
            enabled: true,
            intensity: 1.0,
            params_json: format!(r#"{{"note":"{}"}}"#, "a".repeat(17 * 1024)),
        },
    ];

    let mut next_id = 1_u64;
    for bad in forbidden_cases {
        // Test output chain, layer chain, inner media chain, inner slide chain across PresentScene and StagePreviewScene.
        let variants = [
            scene_with_effects(vec![bad.clone()], vec![], vec![], vec![]),
            scene_with_effects(vec![], vec![bad.clone()], vec![], vec![]),
            scene_with_effects(vec![], vec![], vec![bad.clone()], vec![]),
            scene_with_effects(vec![], vec![], vec![], vec![bad]),
        ];
        for scene in variants {
            let p_code = expect_error_code(core.handle(&ui_cmd(
                next_id,
                command::Action::PresentScene(PresentScene {
                    scene: Some(scene.clone()),
                }),
            )));
            next_id += 1;
            assert_eq!(
                p_code, "SEL-IPC-0002",
                "PresentScene must refuse custom/invalid shader"
            );

            let s_code = expect_error_code(core.handle(&ui_cmd(
                next_id,
                command::Action::StagePreviewScene(StagePreviewScene { scene: Some(scene) }),
            )));
            next_id += 1;
            assert_eq!(
                s_code, "SEL-IPC-0002",
                "StagePreviewScene must refuse custom/invalid shader"
            );
        }
    }

    // Over MAX_EFFECTS_PER_CHAIN (8) stages is also rejected (SEC-03).
    let too_many = vec![
        EffectSpec {
            kind: EffectKind::Blur as i32,
            enabled: true,
            intensity: 0.5,
            params_json: r#"{"sigma":2.0}"#.into(),
        };
        9
    ];
    let overflow_scene = scene_with_effects(too_many, vec![], vec![], vec![]);
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            next_id,
            command::Action::PresentScene(PresentScene {
                scene: Some(overflow_scene.clone()),
            }),
        ))),
        "SEL-IPC-0002"
    );
    next_id += 1;
    assert_eq!(
        expect_error_code(core.handle(&ui_cmd(
            next_id,
            command::Action::StagePreviewScene(StagePreviewScene {
                scene: Some(overflow_scene),
            }),
        ))),
        "SEL-IPC-0002"
    );
    next_id += 1;

    // Closed set of all 6 built-in effects passes validation on both PresentScene and StagePreviewScene.
    let valid_scene = scene_with_effects(
        vec![EffectSpec {
            kind: EffectKind::Vignette as i32,
            enabled: true,
            intensity: 0.6,
            params_json: r##"{"inner_radius":0.45,"softness":0.4,"color":"#000000"}"##.into(),
        }],
        vec![EffectSpec {
            kind: EffectKind::ColourAdjust as i32,
            enabled: true,
            intensity: 0.8,
            params_json: r#"{"brightness":0.05,"contrast":1.1,"saturation":1.05}"#.into(),
        }],
        vec![
            EffectSpec {
                kind: EffectKind::Blur as i32,
                enabled: true,
                intensity: 0.5,
                params_json: r#"{"sigma":3.0}"#.into(),
            },
            EffectSpec {
                kind: EffectKind::ColourKey as i32,
                enabled: true,
                intensity: 1.0,
                params_json: r##"{"key":"#00ff00","tolerance":0.25,"softness":0.1}"##.into(),
            },
        ],
        vec![
            EffectSpec {
                kind: EffectKind::BackgroundBlur as i32,
                enabled: true,
                intensity: 0.7,
                params_json: r##"{"sigma":6.0,"region":[0.1,0.7,0.8,0.2],"tint":"#00000080"}"##
                    .into(),
            },
            EffectSpec {
                kind: EffectKind::Mask as i32,
                enabled: true,
                intensity: 1.0,
                params_json:
                    r#"{"shape":"rounded_rect","bounds":[0.05,0.05,0.9,0.9],"radius":0.05}"#.into(),
            },
        ],
    );

    let p_reply = unwrap_reply(core.handle(&ui_cmd(
        next_id,
        command::Action::PresentScene(PresentScene {
            scene: Some(valid_scene.clone()),
        }),
    )));
    next_id += 1;
    assert_eq!(p_reply, reply::Result::Acknowledged(true));

    let s_reply = unwrap_reply(core.handle(&ui_cmd(
        next_id,
        command::Action::StagePreviewScene(StagePreviewScene {
            scene: Some(valid_scene),
        }),
    )));
    assert_eq!(s_reply, reply::Result::Acknowledged(true));
}

#[test]
fn import_media_flags_flashing_clip_and_selah_round_trip_preserves_warning() {
    // safe: SAFE-04
    let dir_a = tempfile::tempdir().unwrap();
    let mut core_a = CoreService::new(dir_a.path(), false);

    let flash_clip = repo_root().join("tests/fixtures/media/committed/wcag-flash-360p60.mp4");
    let safe_clip = repo_root().join("tests/fixtures/media/committed/flash-beep-360p60.mp4");

    let reply::Result::MediaItem(flash_item) = unwrap_reply(core_a.handle(&ui_cmd(
        1,
        command::Action::ImportMedia(ImportMedia {
            source_path: flash_clip.to_string_lossy().into_owned(),
            name: "Strobe Warning Loop".into(),
            tags: vec!["Background".into()],
        }),
    ))) else {
        panic!("expected MediaItem for flashing clip");
    };
    assert!(
        flash_item.flash_warning,
        "wcag-flash-360p60.mp4 must be flagged with flash_warning = true on import"
    );
    assert!(
        flash_item.tags.iter().any(|t| t == "Flashing"),
        "expected 'Flashing' tag on unsafe strobe video, got {:?}",
        flash_item.tags
    );

    let reply::Result::MediaItem(safe_item) = unwrap_reply(core_a.handle(&ui_cmd(
        2,
        command::Action::ImportMedia(ImportMedia {
            source_path: safe_clip.to_string_lossy().into_owned(),
            name: "Safe Calibration Clip".into(),
            tags: vec!["Calibration".into()],
        }),
    ))) else {
        panic!("expected MediaItem for safe clip");
    };
    assert!(
        !safe_item.flash_warning,
        "flash-beep-360p60.mp4 must NOT trigger flash_warning on import"
    );
    assert!(
        !safe_item
            .tags
            .iter()
            .any(|t| t.eq_ignore_ascii_case("flashing")),
        "safe clip must not carry Flashing tag"
    );

    // Export both items into a .selah bundle, import into clean core_b, re-export, verify parity.
    let bundle_1 = dir_a.path().join("flash-export-1.selah");
    unwrap_reply(core_a.handle(&ui_cmd(
        3,
        command::Action::ExportSelahBundle(ExportSelahBundle {
            output_path: bundle_1.to_string_lossy().into_owned(),
            media_ids: vec![
                flash_item.id.clone().unwrap(),
                safe_item.id.clone().unwrap(),
            ],
            include_media: true,
            ..Default::default()
        }),
    )));

    let dir_b = tempfile::tempdir().unwrap();
    let mut core_b = CoreService::new(dir_b.path(), false);
    unwrap_reply(core_b.handle(&ui_cmd(
        4,
        command::Action::ImportSelahBundle(ImportSelahBundle {
            source_path: bundle_1.to_string_lossy().into_owned(),
            raw_bytes: Vec::new(),
            overwrite_existing: false,
        }),
    )));

    let reply::Result::MediaItem(reimported_flash) = unwrap_reply(core_b.handle(&ui_query(
        5,
        query::Request::GetMediaItem(GetMediaItem {
            id: flash_item.id.clone(),
        }),
    ))) else {
        panic!("expected imported MediaItem");
    };
    assert!(reimported_flash.flash_warning);
    assert!(reimported_flash.tags.iter().any(|t| t == "Flashing"));

    let reply::Result::MediaItem(reimported_safe) = unwrap_reply(core_b.handle(&ui_query(
        6,
        query::Request::GetMediaItem(GetMediaItem {
            id: safe_item.id.clone(),
        }),
    ))) else {
        panic!("expected imported safe MediaItem");
    };
    assert!(!reimported_safe.flash_warning);

    let bundle_2 = dir_b.path().join("flash-export-2.selah");
    unwrap_reply(core_b.handle(&ui_cmd(
        7,
        command::Action::ExportSelahBundle(ExportSelahBundle {
            output_path: bundle_2.to_string_lossy().into_owned(),
            media_ids: vec![flash_item.id.unwrap(), safe_item.id.unwrap()],
            include_media: true,
            ..Default::default()
        }),
    )));
    assert_eq!(
        std::fs::read(&bundle_1).unwrap(),
        std::fs::read(&bundle_2).unwrap(),
        ".selah export -> import -> export with Flashing media must be byte-identical"
    );
}

#[test]
fn analyze_flash_risk_and_split_slides_report_wcag_warnings() {
    // safe: SAFE-04
    let dir = tempfile::tempdir().unwrap();
    let mut core = CoreService::new(dir.path(), false);
    core.ensure_bundled_bibles().unwrap();

    let flash_clip = repo_root().join("tests/fixtures/media/committed/wcag-flash-360p60.mp4");
    let reply::Result::MediaItem(flash_item) = unwrap_reply(core.handle(&ui_cmd(
        1,
        command::Action::ImportMedia(ImportMedia {
            source_path: flash_clip.to_string_lossy().into_owned(),
            name: "Strobe".into(),
            tags: Vec::new(),
        }),
    ))) else {
        panic!("expected flashing MediaItem");
    };

    let solid = |r: f32, g: f32, b: f32| Scene {
        revision: 1,
        outputs: vec![OutputScene {
            output_index: 0,
            layers: vec![LayerSpec {
                kind: LayerKind::Media as i32,
                enabled: true,
                content: Some(layer_spec::Content::Media(MediaLayerSpec {
                    solid_color: Some(Color {
                        red: r,
                        green: g,
                        blue: b,
                        alpha: 1.0,
                    }),
                    ..Default::default()
                })),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };

    // 1. Unsafe rapid saturated-red Cut cadence flagged by AnalyzeFlashRisk.
    let reply::Result::FlashRiskReport(unsafe_rep) = unwrap_reply(core.handle(&ui_query(
        2,
        query::Request::AnalyzeFlashRisk(AnalyzeFlashRisk {
            transition: Some(TransitionSpec {
                kind: TransitionKind::Cut as i32,
                duration_ms: 0,
                ..Default::default()
            }),
            repeat_cadence_ms: 100,
            from_scene: Some(solid(0.0, 0.0, 0.0)),
            to_scene: Some(solid(1.0, 0.0, 0.0)),
            media_id: None,
            flashes_per_second_hint: 0.0,
        }),
    ))) else {
        panic!("expected FlashRiskReport");
    };
    assert!(unsafe_rep.flash_warning);
    assert!(unsafe_rep.saturated_red_risk);
    assert!(unsafe_rep.flashes_per_second > 3.0);
    assert!(!unsafe_rep.warning_codes.is_empty());
    assert!(!unsafe_rep.reason.is_empty());

    // 2. Smooth slow single Fade passes without flash_warning.
    let reply::Result::FlashRiskReport(safe_rep) = unwrap_reply(core.handle(&ui_query(
        3,
        query::Request::AnalyzeFlashRisk(AnalyzeFlashRisk {
            transition: Some(TransitionSpec {
                kind: TransitionKind::Fade as i32,
                duration_ms: 600,
                easing: TransitionEasing::EaseInOut as i32,
                ..Default::default()
            }),
            repeat_cadence_ms: 0,
            from_scene: Some(solid(0.0, 0.0, 0.0)),
            to_scene: Some(solid(1.0, 1.0, 1.0)),
            media_id: None,
            flashes_per_second_hint: 0.0,
        }),
    ))) else {
        panic!("expected safe FlashRiskReport");
    };
    assert!(!safe_rep.flash_warning);

    // 3. Flashing media_id triggers AnalyzeFlashRisk & propagates into SplitSongSlides / SplitScriptureSlides fit_reports.
    let reply::Result::FlashRiskReport(media_rep) = unwrap_reply(core.handle(&ui_query(
        4,
        query::Request::AnalyzeFlashRisk(AnalyzeFlashRisk {
            media_id: flash_item.id.clone(),
            ..Default::default()
        }),
    ))) else {
        panic!("expected media FlashRiskReport");
    };
    assert!(media_rep.flash_warning);
    assert!(
        media_rep
            .warning_codes
            .iter()
            .any(|c| c == "MEDIA_FLASH_WARNING")
    );

    let v1 = new_id();
    let song = Song {
        id: Some(new_id()),
        title: "Strobe Song".into(),
        sections: vec![Section {
            id: Some(v1.clone()),
            label: "Verse 1".into(),
            kind: SectionKind::Verse as i32,
            lines: vec![
                LyricLine {
                    text: "Grace upon grace shines forevermore".into(),
                    chords: Vec::new(),
                },
                LyricLine {
                    text: "Bright as the morning star".into(),
                    chords: Vec::new(),
                },
            ],
            translations: HashMap::new(),
            ..Default::default()
        }],
        arrangements: vec![Arrangement {
            id: Some(new_id()),
            name: "Default".into(),
            section_ids: vec![v1],
        }],
        ..Default::default()
    };

    for shrink in [false, true] {
        let reply::Result::GeneratedSongSlides(song_slides) = unwrap_reply(core.handle(&ui_query(
            if shrink { 5 } else { 6 },
            query::Request::SplitSongSlides(SplitSongSlides {
                song: Some(song.clone()),
                max_lines: 4,
                shrink_to_fit: shrink,
                media_id: flash_item.id.clone(),
                target_outputs: if shrink {
                    vec![FitTargetOutput {
                        output_index: 0,
                        name: "Main 16:9".into(),
                        resolution: Some(Resolution {
                            width: 1920,
                            height: 1080,
                        }),
                        ..Default::default()
                    }]
                } else {
                    Vec::new()
                },
                ..Default::default()
            }),
        ))) else {
            panic!("expected GeneratedSongSlides");
        };
        assert!(!song_slides.fit_reports.is_empty());
        assert!(
            song_slides
                .fit_reports
                .iter()
                .all(|r| r.flash_warning && r.warning_codes.iter().any(|c| c == "FLASH_WARNING")),
            "song split fit_reports missing FLASH_WARNING (shrink={shrink}): {:?}",
            song_slides.fit_reports
        );
    }

    let reply::Result::GeneratedScriptureSlides(scripture_slides) =
        unwrap_reply(core.handle(&ui_query(
            7,
            query::Request::SplitScriptureSlides(SplitScriptureSlides {
                translation: "KJV".into(),
                reference: "John 3:16".into(),
                shrink_to_fit: true,
                media_id: flash_item.id,
                target_outputs: vec![FitTargetOutput {
                    output_index: 0,
                    name: "Main".into(),
                    resolution: Some(Resolution {
                        width: 1920,
                        height: 1080,
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            }),
        )))
    else {
        panic!("expected GeneratedScriptureSlides");
    };
    assert!(!scripture_slides.fit_reports.is_empty());
    assert!(
        scripture_slides
            .fit_reports
            .iter()
            .all(|r| r.flash_warning && r.warning_codes.iter().any(|c| c == "FLASH_WARNING"))
    );
}
