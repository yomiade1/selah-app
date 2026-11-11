//! Integration tests for M4.2 Live state machine:
//! - Current and next slide and item resolution and navigation (`LoadLiveService`, `TriggerItem`,
//!   `TriggerSlide`, `NextSlide`, `PreviousSlide`, `NextItem`, `PreviousItem`)
//! - Per-label section hotkeys (`1`–`9`, `C`, `B`, `T`, `P`, `I`, `O`)
//! - Independent layer control (`SetLayerState`, `ClearLayer` F10/F11, `ShowLogoCommand` F9, `ClearOutputs` F12)
//! - Basic audio-layer playback (`PlayAudioLayer`, `StopAudioLayer`, audio service items)
//! - Live undo/redo (`LiveUndo` Ctrl+Z / `LiveRedo`, M4-A2 frame checksum verification)
//! - Crash recovery via `live_state.bin` and `live_machine.bin`

use selah_core::CoreService;
use selah_core::library::new_id;
use selah_proto::v1::{
    Actor, ActorOrigin, Chord, ClearLayer, ClearOutputs, Color, Command, Envelope, GetLiveState,
    LayerKind, LayerSpec, LiveRedo, LiveStatus, LiveUndo, LoadLiveService, LyricLine, MediaItem,
    MediaKind, MediaLayerSpec, NextItem, NextSlide, PlayAudioLayer, PreviousItem, PreviousSlide,
    PropsLayerSpec, Query, SaveMediaItem, SaveService, SaveSong, Section, SectionKind, Service,
    ServiceItem, ServiceItemKind, ShowLogoCommand, Song, StopAudioLayer, TriggerItem,
    TriggerSectionHotkey, TriggerSlide, command, envelope, layer_spec, query, reply,
};
use selah_render::Compositor;
use selah_render::cpu::CpuCompositor;

fn ui_actor() -> Actor {
    Actor {
        user_id: "local-owner".into(),
        device_id: "test-ui".into(),
        origin: ActorOrigin::Ui as i32,
    }
}

fn send_command(core: &mut CoreService, id: u64, action: command::Action) -> Envelope {
    let req = Envelope {
        id,
        correlation_id: 0,
        actor: Some(ui_actor()),
        body: Some(envelope::Body::Command(Command {
            action: Some(action),
            undo_group: None,
        })),
    };
    core.handle(&req).reply.expect("command reply")
}

fn send_live_command(core: &mut CoreService, id: u64, action: command::Action) -> LiveStatus {
    let reply = send_command(core, id, action);
    match reply.body {
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::LiveStatus(status)),
        })) => status,
        other => panic!("expected LiveStatus reply, got {other:?}"),
    }
}

fn get_live_state(core: &mut CoreService, id: u64) -> LiveStatus {
    let req = Envelope {
        id,
        correlation_id: 0,
        actor: Some(ui_actor()),
        body: Some(envelope::Body::Query(Query {
            request: Some(query::Request::GetLiveState(GetLiveState {})),
        })),
    };
    let reply = core.handle(&req).reply.expect("query reply");
    match reply.body {
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::LiveStatus(status)),
        })) => status,
        other => panic!("expected LiveStatus query reply, got {other:?}"),
    }
}

#[test]
fn live_machine_triggers_items_slides_next_previous_and_skips_headers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut core = CoreService::new(dir.path(), false);

    let song_id = new_id();
    let v1_id = new_id();
    let ch_id = new_id();
    let br_id = new_id();
    let song = Song {
        id: Some(song_id.clone()),
        title: "Cornerstone".into(),
        key: "C".into(),
        sections: vec![
            Section {
                id: Some(v1_id.clone()),
                kind: SectionKind::Verse as i32,
                label: "Verse 1".into(),
                lines: vec![
                    LyricLine {
                        text: "My hope is built on nothing less".into(),
                        chords: vec![
                            Chord {
                                position: 0,
                                text: "C".into(),
                            },
                            Chord {
                                position: 20,
                                text: "F".into(),
                            },
                        ],
                    },
                    LyricLine {
                        text: "Than Jesus blood and righteousness".into(),
                        chords: vec![Chord {
                            position: 5,
                            text: "G".into(),
                        }],
                    },
                ],
                ..Default::default()
            },
            Section {
                id: Some(ch_id.clone()),
                kind: SectionKind::Chorus as i32,
                label: "Chorus".into(),
                lines: vec![LyricLine {
                    text: "Christ alone, Cornerstone".into(),
                    chords: vec![
                        Chord {
                            position: 0,
                            text: "F".into(),
                        },
                        Chord {
                            position: 14,
                            text: "G".into(),
                        },
                    ],
                }],
                ..Default::default()
            },
            Section {
                id: Some(br_id.clone()),
                kind: SectionKind::Bridge as i32,
                label: "Bridge".into(),
                lines: vec![LyricLine {
                    text: "When He shall come with trumpet sound".into(),
                    chords: vec![Chord {
                        position: 0,
                        text: "Am".into(),
                    }],
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    send_command(
        &mut core,
        1,
        command::Action::SaveSong(SaveSong { song: Some(song) }),
    );

    let service_id = new_id();
    let service = Service {
        id: Some(service_id.clone()),
        name: "Sunday Morning Live".into(),
        items: vec![
            ServiceItem {
                id: Some(new_id()),
                kind: ServiceItemKind::Header as i32,
                title: "Praise & Worship".into(),
                ..Default::default()
            },
            ServiceItem {
                id: Some(new_id()),
                kind: ServiceItemKind::Song as i32,
                content_id: Some(song_id),
                title: "Cornerstone".into(),
                song_key: "D".into(),
                notes: "Capo 2 for acoustic".into(),
                ..Default::default()
            },
            ServiceItem {
                id: Some(new_id()),
                kind: ServiceItemKind::Scripture as i32,
                title: "Psalm 23:1-2".into(),
                scripture_reference: "Psalm 23:1-2".into(),
                bible_translation: "KJV".into(),
                ..Default::default()
            },
            ServiceItem {
                id: Some(new_id()),
                kind: ServiceItemKind::Note as i32,
                title: "Pastoral Prayer".into(),
                notes: "Invite elders to the front".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    send_command(
        &mut core,
        2,
        command::Action::SaveService(SaveService {
            service: Some(service),
        }),
    );

    // Load service and trigger the first non-header item (index 1, slide 0).
    let status = send_live_command(
        &mut core,
        3,
        command::Action::LoadLiveService(LoadLiveService {
            service_id: Some(service_id),
            trigger_first_item: true,
        }),
    );
    assert!(status.has_active_item);
    assert_eq!(status.current_item_index, 1);
    assert_eq!(status.current_slide_index, 0);
    assert_eq!(status.current_item_title, "Cornerstone");
    assert_eq!(status.song_key, "D");
    assert_eq!(status.current_slide_label, "Verse 1");
    assert!(status.current_slide_text.contains("My hope is built"));
    // Chords should be transposed from C -> D ("D", "G").
    assert!(status.current_chords_json.contains("\"D\""));
    assert!(status.current_chords_json.contains("\"G\""));
    assert!(status.has_next);
    assert_eq!(status.next_item_index, 1);
    assert_eq!(status.next_slide_index, 1);
    assert_eq!(status.next_slide_label, "Chorus");
    assert!(!core.scene().preload_items.is_empty());

    // NextSlide -> Chorus (index 1, slide 1)
    let status = send_live_command(&mut core, 4, command::Action::NextSlide(NextSlide {}));
    assert_eq!(status.current_item_index, 1);
    assert_eq!(status.current_slide_index, 1);
    assert_eq!(status.current_slide_label, "Chorus");
    assert_eq!(status.next_item_index, 1);
    assert_eq!(status.next_slide_index, 2);
    assert_eq!(status.next_slide_label, "Bridge");

    // NextItem -> Scripture (index 2, slide 0)
    let status = send_live_command(&mut core, 5, command::Action::NextItem(NextItem {}));
    assert_eq!(status.current_item_index, 2);
    assert_eq!(status.current_slide_index, 0);
    assert_eq!(status.current_item_title, "Psalm 23:1-2");
    assert!(
        status
            .current_slide_text
            .to_ascii_lowercase()
            .contains("shepherd")
    );

    // PreviousSlide -> jumps back to last slide of Song (index 1, slide 2: Bridge)
    let status = send_live_command(
        &mut core,
        6,
        command::Action::PreviousSlide(PreviousSlide {}),
    );
    assert_eq!(status.current_item_index, 1);
    assert_eq!(status.current_slide_index, 2);
    assert_eq!(status.current_slide_label, "Bridge");

    // NextItem twice -> Note item (index 3), which sets stage notes and blanks audience slide
    let _ = send_live_command(&mut core, 7, command::Action::NextItem(NextItem {}));
    let status = send_live_command(&mut core, 8, command::Action::NextItem(NextItem {}));
    assert_eq!(status.current_item_index, 3);
    assert_eq!(status.current_notes, "Invite elders to the front");
    assert!(!status.has_next);

    // PreviousItem -> back to Scripture (index 2)
    let status = send_live_command(&mut core, 9, command::Action::PreviousItem(PreviousItem {}));
    assert_eq!(status.current_item_index, 2);
}

#[test]
fn live_machine_section_hotkeys_f9_f12_layers_and_audio_playback() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut core = CoreService::new(dir.path(), false);

    let song_id = new_id();
    let song = Song {
        id: Some(song_id.clone()),
        title: "Way Maker".into(),
        key: "E".into(),
        sections: vec![
            Section {
                id: Some(new_id()),
                kind: SectionKind::Verse as i32,
                label: "Verse 1".into(),
                lines: vec![LyricLine {
                    text: "You are here, moving in our midst".into(),
                    chords: Vec::new(),
                }],
                ..Default::default()
            },
            Section {
                id: Some(new_id()),
                kind: SectionKind::Verse as i32,
                label: "Verse 2".into(),
                lines: vec![LyricLine {
                    text: "You are here, touching every heart".into(),
                    chords: Vec::new(),
                }],
                ..Default::default()
            },
            Section {
                id: Some(new_id()),
                kind: SectionKind::Chorus as i32,
                label: "Chorus".into(),
                lines: vec![LyricLine {
                    text: "Way maker, miracle worker".into(),
                    chords: Vec::new(),
                }],
                ..Default::default()
            },
            Section {
                id: Some(new_id()),
                kind: SectionKind::Bridge as i32,
                label: "Bridge".into(),
                lines: vec![LyricLine {
                    text: "Even when I don't see it, You're working".into(),
                    chords: Vec::new(),
                }],
                ..Default::default()
            },
            Section {
                id: Some(new_id()),
                kind: SectionKind::Tag as i32,
                label: "Tag".into(),
                lines: vec![LyricLine {
                    text: "That is who You are".into(),
                    chords: Vec::new(),
                }],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    send_command(
        &mut core,
        1,
        command::Action::SaveSong(SaveSong { song: Some(song) }),
    );

    let media_store = core.library().media_store().expect("open media store");
    let blob = media_store
        .import_reader(b"RIFF....WAVEfmt ".as_slice(), "wav", 4096)
        .expect("import audio blob");
    let audio_id = new_id();
    let audio_item = MediaItem {
        id: Some(audio_id.clone()),
        name: "Warm Pad E.wav".into(),
        kind: MediaKind::Audio as i32,
        content_hash: blob.content_hash,
        extension: blob.extension,
        size_bytes: blob.size_bytes,
        ..Default::default()
    };
    send_command(
        &mut core,
        2,
        command::Action::SaveMediaItem(SaveMediaItem {
            media_item: Some(audio_item),
        }),
    );

    let service_id = new_id();
    let service = Service {
        id: Some(service_id.clone()),
        name: "Worship Night".into(),
        items: vec![ServiceItem {
            id: Some(new_id()),
            kind: ServiceItemKind::Song as i32,
            content_id: Some(song_id),
            title: "Way Maker".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    send_command(
        &mut core,
        3,
        command::Action::SaveService(SaveService {
            service: Some(service),
        }),
    );
    send_live_command(
        &mut core,
        4,
        command::Action::LoadLiveService(LoadLiveService {
            service_id: Some(service_id),
            trigger_first_item: true,
        }),
    );

    // Per-label section hotkeys: 'C' -> Chorus, 'B' -> Bridge, '2' -> Verse 2, 'T' -> Tag, '1' -> Verse 1
    let status = send_live_command(
        &mut core,
        5,
        command::Action::TriggerSectionHotkey(TriggerSectionHotkey { hotkey: "C".into() }),
    );
    assert_eq!(status.current_slide_label, "Chorus");
    assert_eq!(status.current_slide_text, "Way maker, miracle worker");

    let status = send_live_command(
        &mut core,
        6,
        command::Action::TriggerSectionHotkey(TriggerSectionHotkey { hotkey: "B".into() }),
    );
    assert_eq!(status.current_slide_label, "Bridge");

    let status = send_live_command(
        &mut core,
        7,
        command::Action::TriggerSectionHotkey(TriggerSectionHotkey { hotkey: "2".into() }),
    );
    assert_eq!(status.current_slide_label, "Verse 2");

    let status = send_live_command(
        &mut core,
        8,
        command::Action::TriggerSectionHotkey(TriggerSectionHotkey { hotkey: "T".into() }),
    );
    assert_eq!(status.current_slide_label, "Tag");

    // Set Media layer and Props layer so multiple layers are active alongside Slide.
    send_live_command(
        &mut core,
        9,
        command::Action::SetLayerState(selah_proto::v1::SetLayerState {
            layer: Some(LayerSpec {
                kind: LayerKind::Media as i32,
                enabled: true,
                content: Some(layer_spec::Content::Media(MediaLayerSpec {
                    solid_color: Some(Color {
                        red: 0.1,
                        green: 0.2,
                        blue: 0.4,
                        alpha: 1.0,
                    }),
                    ..Default::default()
                })),
                ..Default::default()
            }),
        }),
    );
    let status = send_live_command(
        &mut core,
        10,
        command::Action::SetLayerState(selah_proto::v1::SetLayerState {
            layer: Some(LayerSpec {
                kind: LayerKind::Props as i32,
                enabled: true,
                content: Some(layer_spec::Content::Props(PropsLayerSpec {
                    label: "Church Bug".into(),
                    background_color: Some(Color {
                        red: 0.0,
                        green: 0.5,
                        blue: 0.8,
                        alpha: 0.8,
                    }),
                    ..Default::default()
                })),
                ..Default::default()
            }),
        }),
    );
    assert!(
        status
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Slide as i32 && l.active)
    );
    assert!(
        status
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Media as i32 && l.active)
    );
    assert!(
        status
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Props as i32 && l.active)
    );

    // Play audio layer and verify Scene.audio_layers is populated.
    let status = send_live_command(
        &mut core,
        11,
        command::Action::PlayAudioLayer(PlayAudioLayer {
            media_id: Some(audio_id),
            volume: 0.75,
            loop_audio: true,
        }),
    );
    let audio = status.audio.expect("audio status");
    assert!(audio.playing);
    assert_eq!(audio.title, "Warm Pad E.wav");
    assert!(!core.scene().audio_layers.is_empty());

    // F10: Clear Slide layer only; Media, Props, and Audio stay active.
    let status = send_live_command(
        &mut core,
        12,
        command::Action::ClearLayer(ClearLayer {
            kind: LayerKind::Slide as i32,
        }),
    );
    assert!(
        !status
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Slide as i32 && l.active)
    );
    assert!(
        status
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Media as i32 && l.active)
    );
    assert!(status.audio.as_ref().is_some_and(|a| a.playing));

    // F11: Clear Media layer only; Props and Audio stay active.
    let status = send_live_command(
        &mut core,
        13,
        command::Action::ClearLayer(ClearLayer {
            kind: LayerKind::Media as i32,
        }),
    );
    assert!(
        !status
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Media as i32 && l.active)
    );
    assert!(
        status
            .layers
            .iter()
            .any(|l| l.kind == LayerKind::Props as i32 && l.active)
    );

    // F9: ShowLogoCommand toggles show_logo on outputs.
    let status = send_live_command(
        &mut core,
        14,
        command::Action::ShowLogo(ShowLogoCommand { show_logo: true }),
    );
    assert!(status.show_logo);
    assert!(core.scene().outputs.iter().all(|o| o.show_logo));

    // StopAudioLayer stops audio playback.
    let status = send_live_command(
        &mut core,
        15,
        command::Action::StopAudioLayer(StopAudioLayer {}),
    );
    assert!(!status.audio.as_ref().is_some_and(|a| a.playing));
    assert!(core.scene().audio_layers.is_empty());

    // F12: ClearOutputs clears every layer and resets show_logo.
    send_command(
        &mut core,
        16,
        command::Action::ClearOutputs(ClearOutputs {}),
    );
    let status = get_live_state(&mut core, 17);
    assert!(status.cleared);
    assert!(!status.show_logo);
    assert!(status.layers.iter().all(|l| !l.active));
}

#[test]
fn live_undo_ctrl_z_restores_exact_previous_frame_checksum_and_survives_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut core = CoreService::new(dir.path(), false);
    let mut compositor = CpuCompositor::default();

    let song_id = new_id();
    let song = Song {
        id: Some(song_id.clone()),
        title: "Amazing Grace".into(),
        key: "G".into(),
        sections: vec![
            Section {
                id: Some(new_id()),
                kind: SectionKind::Verse as i32,
                label: "Verse 1".into(),
                lines: vec![
                    LyricLine {
                        text: "Amazing grace how sweet the sound".into(),
                        chords: Vec::new(),
                    },
                    LyricLine {
                        text: "That saved a wretch like me".into(),
                        chords: Vec::new(),
                    },
                ],
                ..Default::default()
            },
            Section {
                id: Some(new_id()),
                kind: SectionKind::Verse as i32,
                label: "Verse 2".into(),
                lines: vec![
                    LyricLine {
                        text: "Twas grace that taught my heart to fear".into(),
                        chords: Vec::new(),
                    },
                    LyricLine {
                        text: "And grace my fears relieved".into(),
                        chords: Vec::new(),
                    },
                ],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    send_command(
        &mut core,
        1,
        command::Action::SaveSong(SaveSong { song: Some(song) }),
    );

    let service_id = new_id();
    let service = Service {
        id: Some(service_id.clone()),
        name: "Sunday Service".into(),
        items: vec![ServiceItem {
            id: Some(new_id()),
            kind: ServiceItemKind::Song as i32,
            content_id: Some(song_id),
            title: "Amazing Grace".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    send_command(
        &mut core,
        2,
        command::Action::SaveService(SaveService {
            service: Some(service),
        }),
    );

    // Load service without triggering yet, then trigger Slide 0 (Verse 1).
    send_live_command(
        &mut core,
        3,
        command::Action::LoadLiveService(LoadLiveService {
            service_id: Some(service_id),
            trigger_first_item: false,
        }),
    );
    let status_v1 = send_live_command(
        &mut core,
        4,
        command::Action::TriggerItem(TriggerItem { item_index: 0 }),
    );
    assert_eq!(status_v1.current_slide_index, 0);
    let checksum_v1 = compositor
        .render(core.scene(), 640, 360)
        .expect("render Verse 1")
        .checksum();

    // Trigger Slide 1 (Verse 2).
    let status_v2 = send_live_command(
        &mut core,
        5,
        command::Action::TriggerSlide(TriggerSlide {
            item_index: 0,
            slide_index: 1,
        }),
    );
    assert_eq!(status_v2.current_slide_index, 1);
    let checksum_v2 = compositor
        .render(core.scene(), 640, 360)
        .expect("render Verse 2")
        .checksum();
    assert_ne!(checksum_v1, checksum_v2);

    // Trigger F9 ShowLogo.
    let status_logo = send_live_command(
        &mut core,
        6,
        command::Action::ShowLogo(ShowLogoCommand { show_logo: true }),
    );
    assert!(status_logo.show_logo);
    let checksum_logo = compositor
        .render(core.scene(), 640, 360)
        .expect("render Logo")
        .checksum();
    assert_ne!(checksum_logo, checksum_v2);

    // LiveUndo (Ctrl+Z in Live) #1 -> restores Verse 2 with exact frame checksum (M4-A2).
    let undo_1 = send_live_command(&mut core, 7, command::Action::LiveUndo(LiveUndo {}));
    assert!(!undo_1.show_logo);
    assert_eq!(undo_1.current_slide_index, 1);
    assert!(undo_1.can_live_undo);
    assert!(undo_1.can_live_redo);
    let checksum_after_undo_1 = compositor
        .render(core.scene(), 640, 360)
        .expect("render after undo 1")
        .checksum();
    assert_eq!(checksum_after_undo_1, checksum_v2);

    // LiveUndo (Ctrl+Z in Live) #2 -> restores Verse 1 with exact frame checksum (M4-A2).
    let undo_2 = send_live_command(&mut core, 8, command::Action::LiveUndo(LiveUndo {}));
    assert_eq!(undo_2.current_slide_index, 0);
    assert_eq!(undo_2.current_slide_label, "Verse 1");
    let checksum_after_undo_2 = compositor
        .render(core.scene(), 640, 360)
        .expect("render after undo 2")
        .checksum();
    assert_eq!(checksum_after_undo_2, checksum_v1);

    // LiveRedo -> restores Verse 2 with exact frame checksum.
    let redo_1 = send_live_command(&mut core, 9, command::Action::LiveRedo(LiveRedo {}));
    assert_eq!(redo_1.current_slide_index, 1);
    let checksum_after_redo_1 = compositor
        .render(core.scene(), 640, 360)
        .expect("render after redo 1")
        .checksum();
    assert_eq!(checksum_after_redo_1, checksum_v2);

    // Simulate crash & restart of CoreService from disk snapshots (`live_state.bin` + `live_machine.bin`).
    drop(core);
    let mut reopened = CoreService::new(dir.path(), false);
    let recovered = get_live_state(&mut reopened, 10);
    assert_eq!(recovered.service_name, "Sunday Service");
    assert_eq!(recovered.current_slide_index, 1);
    assert!(recovered.can_live_undo);
    let checksum_recovered = compositor
        .render(reopened.scene(), 640, 360)
        .expect("render recovered scene")
        .checksum();
    assert_eq!(checksum_recovered, checksum_v2);

    // LiveUndo still works after crash recovery!
    let undo_after_restart =
        send_live_command(&mut reopened, 11, command::Action::LiveUndo(LiveUndo {}));
    assert_eq!(undo_after_restart.current_slide_index, 0);
    let checksum_after_restart_undo = compositor
        .render(reopened.scene(), 640, 360)
        .expect("render after restart undo")
        .checksum();
    assert_eq!(checksum_after_restart_undo, checksum_v1);
}
