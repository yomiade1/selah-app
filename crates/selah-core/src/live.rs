//! Live state and its snapshot (`live_state.bin` + `live_machine.bin`, written on every change; ARCHITECTURE §2, M4.2).
//!
//! If core crashes, the restarted core reloads the snapshots and re-sends the scene and live
//! cursor/layers/undo stacks, so the engine keeps showing the same thing and the operator resumes
//! where they left off.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use prost::Message;
use selah_proto::v1::{
    AdvanceLiveClock, AudioLayerStatus, ClearGroup, CopyrightPlacement, EffectKind, EffectSpec, Id,
    LayerKind, LayerSpec, LiveAutomationState, LiveItemSlides, LiveLayerStatus,
    LiveMachineSnapshot, LiveMessage, LiveSlide, LiveSnapshotEntry, LiveStatus, Look, Macro,
    MacroAction, MacroActionKind, MediaFitMode, MediaItem, MediaKind, MediaLayerSpec,
    MessagesLayerSpec, OutputConfig, OutputConfigList, OutputRole, OutputScene, PreloadHint,
    Presentation, Prop, PropsLayerSpec, ReferencePlacement, Resolution, Scene, SectionKind,
    Service, ServiceItem, ServiceItemKind, ShowMessage, SlideLayerSpec, Song, SplitScriptureSlides,
    StageLayout, Template, TestPattern, Timer, TimerKind, TimerOverrunMode, TimerStatus,
    layer_spec,
};
use selah_render::template::{SlideChordAnchor, TemplateStyle};

use crate::library::{CommandBusError, Library, validate_id};
use crate::songs::sections::{
    assign_default_hotkeys_and_colors, color_to_hex, preferred_hotkey, section_color_hex,
    section_kind_key,
};
use crate::songs::split::{SplitOptions, generate_slides};
use crate::songs::{parse_key, transpose_song};

const MAX_LIVE_UNDO: usize = 64;

pub struct LiveState {
    path: PathBuf,
    machine_path: PathBuf,
    configs_path: PathBuf,
    scene: Scene,
    output_configs: Vec<OutputConfig>,
    explicit_configs: bool,
    snapshot: LiveMachineSnapshot,
}

fn default_output_config() -> OutputConfig {
    OutputConfig {
        output_index: 0,
        name: "Main Output".into(),
        role: OutputRole::Main as i32,
        resolution: Some(Resolution {
            width: 1920,
            height: 1080,
        }),
        enabled: true,
        display_index: 0,
        fullscreen: false,
        refresh_rate_mhz: 60_000,
        template_id: None,
    }
}

/// Something other than nothing (or the logo) is on the screens (SAFE-06).
pub fn is_live(scene: &Scene) -> bool {
    selah_render::scene::scene_is_live(scene)
}

fn default_layer_statuses() -> Vec<LiveLayerStatus> {
    [
        LayerKind::VideoInput,
        LayerKind::Media,
        LayerKind::Slide,
        LayerKind::Props,
        LayerKind::Messages,
    ]
    .into_iter()
    .map(|kind| LiveLayerStatus {
        kind: kind as i32,
        active: false,
        title: String::new(),
        subtitle: String::new(),
        spec: None,
    })
    .collect()
}

fn default_live_status(scene: &Scene) -> LiveStatus {
    LiveStatus {
        cleared: scene.cleared,
        show_logo: scene.outputs.iter().any(|o| o.show_logo),
        scene_revision: scene.revision,
        layers: default_layer_statuses(),
        audio: Some(AudioLayerStatus::default()),
        automation: vec![LiveAutomationState::default()],
        ..Default::default()
    }
}

pub fn format_timer_ms(remaining_ms: i64, show_hours: bool) -> String {
    let negative = remaining_ms < 0;
    let abs_ms = remaining_ms.unsigned_abs();
    let total_secs = if negative {
        abs_ms.div_ceil(1000)
    } else {
        abs_ms / 1000
    };
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    let sign = if negative { "-" } else { "" };
    if show_hours || hours > 0 {
        format!("{sign}{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{sign}{mins:02}:{secs:02}")
    }
}

pub fn interpolate_message_tokens(
    template_text: &str,
    default_tokens: &HashMap<String, String>,
    token_values: &HashMap<String, String>,
    countdown_text: &str,
) -> Result<String, CommandBusError> {
    if template_text.contains('\0') {
        return Err(CommandBusError::Invalid(
            "message text must not contain NUL bytes".into(),
        ));
    }
    if token_values.len() > 32 {
        return Err(CommandBusError::Invalid("too many message tokens".into()));
    }
    for (k, v) in token_values {
        if k.trim().is_empty() || k.len() > 64 || v.chars().count() > 256 || v.contains('\0') {
            return Err(CommandBusError::Invalid(
                "invalid message token key or value".into(),
            ));
        }
    }
    let mut out = String::with_capacity(template_text.len());
    let mut chars = template_text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut key = String::new();
            let mut closed = false;
            for next_ch in chars.by_ref() {
                if next_ch == '}' {
                    closed = true;
                    break;
                }
                key.push(next_ch);
            }
            if closed {
                let k = key.trim();
                if k.eq_ignore_ascii_case("countdown") || k.eq_ignore_ascii_case("timer") {
                    out.push_str(if countdown_text.is_empty() {
                        "00:00"
                    } else {
                        countdown_text
                    });
                } else if let Some(val) = token_values
                    .iter()
                    .find(|(tk, _)| tk.eq_ignore_ascii_case(k))
                    .map(|(_, v)| v.as_str())
                    .or_else(|| {
                        default_tokens
                            .iter()
                            .find(|(tk, _)| tk.eq_ignore_ascii_case(k))
                            .map(|(_, v)| v.as_str())
                    })
                {
                    out.push_str(val);
                } else {
                    out.push('{');
                    out.push_str(&key);
                    out.push('}');
                }
            } else {
                out.push('{');
                out.push_str(&key);
            }
        } else {
            out.push(ch);
        }
    }
    let trimmed = out.trim().to_owned();
    if trimmed.is_empty() || trimmed.chars().count() > 512 {
        return Err(CommandBusError::Invalid(
            "interpolated message text must be 1 to 512 characters".into(),
        ));
    }
    Ok(trimmed)
}

fn media_extension_from_item(ext: &str, kind: MediaKind) -> String {
    let trimmed = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    if !trimmed.is_empty() {
        trimmed
    } else {
        match kind {
            MediaKind::Video => "mp4".into(),
            MediaKind::Audio => "wav".into(),
            _ => "png".into(),
        }
    }
}

fn encode_slide_chords_json(lines: &[selah_proto::v1::LyricLine]) -> String {
    let mut anchors = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        for chord in &line.chords {
            let text = chord.text.trim();
            if !text.is_empty() {
                anchors.push(SlideChordAnchor {
                    line: line_idx,
                    position: chord.position as usize,
                    text: text.to_owned(),
                });
            }
        }
    }
    if anchors.is_empty() {
        String::new()
    } else {
        serde_json::to_string(&anchors).unwrap_or_default()
    }
}

fn resolve_template_style_json(library: &Library, template_id: Option<&Id>) -> String {
    let Some(tid) = template_id.filter(|id| !id.value.is_empty()) else {
        return String::new();
    };
    let Ok(Some(doc)) = library.get::<Template>(tid) else {
        return String::new();
    };
    TemplateStyle::from_proto_template(&doc)
        .and_then(|style| style.to_json())
        .unwrap_or_default()
}

fn resolve_item_slides(
    library: &Library,
    service: &Service,
    item_index: u32,
    item: &ServiceItem,
) -> LiveItemSlides {
    let kind = ServiceItemKind::try_from(item.kind).unwrap_or(ServiceItemKind::Unspecified);
    let style_json = resolve_template_style_json(
        library,
        item.template_id
            .as_ref()
            .filter(|id| !id.value.is_empty())
            .or(service
                .default_template_id
                .as_ref()
                .filter(|id| !id.value.is_empty())),
    );
    let mut song_key = item.song_key.trim().to_owned();
    let mut slides: Vec<LiveSlide> = match kind {
        ServiceItemKind::Header => Vec::new(),
        ServiceItemKind::Song => {
            let loaded_song = item
                .content_id
                .as_ref()
                .filter(|id| !id.value.is_empty())
                .and_then(|id| library.get::<Song>(id).ok().flatten());
            if let Some(mut song) = loaded_song {
                assign_default_hotkeys_and_colors(&mut song.sections);
                if song_key.is_empty() {
                    song_key = song.key.trim().to_owned();
                } else if !song.key.trim().is_empty()
                    && !song_key.eq_ignore_ascii_case(song.key.trim())
                    && let (Some((from_semi, _, _)), Some((to_semi, _, prefer_flats))) =
                        (parse_key(&song.key), parse_key(&song_key))
                {
                    let semitones = (i32::from(to_semi) - i32::from(from_semi)).rem_euclid(12);
                    let _ = transpose_song(&mut song, semitones, Some(prefer_flats));
                    song.key = song_key.clone();
                }
                let opts = SplitOptions {
                    arrangement_id: item
                        .arrangement_id
                        .clone()
                        .filter(|id| !id.value.is_empty()),
                    max_lines: 4,
                    copyright_placement: CopyrightPlacement::FirstAndLast,
                    title_slide: false,
                    blank_slide_at_end: false,
                    translation_language: (!item.translation_language.trim().is_empty())
                        .then(|| item.translation_language.clone()),
                };
                match generate_slides(&song, &opts) {
                    Ok(generated) if !generated.slides.is_empty() => generated
                        .slides
                        .into_iter()
                        .enumerate()
                        .map(|(idx, gs)| {
                            let sec_kind = SectionKind::try_from(gs.section_kind)
                                .unwrap_or(SectionKind::Other);
                            let sec_color = gs
                                .section_color
                                .as_ref()
                                .map(color_to_hex)
                                .unwrap_or_else(|| section_color_hex(sec_kind).to_owned());
                            let label = if gs.part_count > 1 {
                                format!(
                                    "{} ({}/{})",
                                    gs.section_label, gs.part_index, gs.part_count
                                )
                            } else {
                                gs.section_label.clone()
                            };
                            let hotkey = if gs.part_index <= 1 {
                                if !gs.section_hotkey.trim().is_empty() {
                                    gs.section_hotkey.trim().to_owned()
                                } else {
                                    preferred_hotkey(sec_kind, &gs.section_label)
                                        .unwrap_or_default()
                                }
                            } else {
                                String::new()
                            };
                            let text = gs
                                .lines
                                .iter()
                                .map(|l| l.text.as_str())
                                .collect::<Vec<_>>()
                                .join("\n");
                            let translation_text = gs.translation_lines.join("\n");
                            LiveSlide {
                                index: idx as u32,
                                label,
                                section_kind: section_kind_key(sec_kind).to_owned(),
                                section_color: sec_color,
                                hotkey,
                                text,
                                chords_json: encode_slide_chords_json(&gs.lines),
                                notes: item.notes.clone(),
                                title_text: song.title.clone(),
                                reference_text: String::new(),
                                footer_text: gs.footer_copyright,
                                translation_text,
                                elements_json: String::new(),
                                style_json: style_json.clone(),
                                is_blank: gs.is_blank,
                                ..Default::default()
                            }
                        })
                        .collect(),
                    _ => vec![LiveSlide {
                        index: 0,
                        label: "Verse 1".into(),
                        section_kind: "verse".into(),
                        section_color: "#2378C3".into(),
                        hotkey: "1".into(),
                        text: if item.notes.trim().is_empty() {
                            song.title.clone()
                        } else {
                            item.notes.clone()
                        },
                        title_text: song.title,
                        style_json,
                        ..Default::default()
                    }],
                }
            } else {
                vec![LiveSlide {
                    index: 0,
                    label: "Verse 1".into(),
                    section_kind: "verse".into(),
                    section_color: "#2378C3".into(),
                    hotkey: "1".into(),
                    text: if item.notes.trim().is_empty() {
                        item.title.clone()
                    } else {
                        item.notes.clone()
                    },
                    title_text: item.title.clone(),
                    style_json,
                    ..Default::default()
                }]
            }
        }
        ServiceItemKind::Scripture => {
            let reference = if !item.scripture_reference.trim().is_empty() {
                item.scripture_reference.trim()
            } else {
                item.title.trim()
            };
            let translation = if !item.bible_translation.trim().is_empty() {
                item.bible_translation.trim()
            } else {
                "KJV"
            };
            let _ = crate::scripture::ensure_bundled_bibles(&library.paths().bibles);
            let split_slides =
                crate::scripture::get_passages(library, translation, reference, &[], "")
                    .ok()
                    .and_then(|passages| {
                        if passages.verses.is_empty() {
                            return None;
                        }
                        crate::scripture::split_scripture_slides(&SplitScriptureSlides {
                            verses: passages.verses,
                            translation: translation.to_owned(),
                            max_lines: 4,
                            max_chars_per_line: 48,
                            show_verse_numbers: true,
                            reference_placement: ReferencePlacement::SlideFooter as i32,
                            ..Default::default()
                        })
                        .ok()
                    });
            match split_slides {
                Some(gen_slides) if !gen_slides.slides.is_empty() => gen_slides
                    .slides
                    .into_iter()
                    .enumerate()
                    .map(|(idx, s)| LiveSlide {
                        index: idx as u32,
                        label: s.reference.clone(),
                        section_kind: "scripture".into(),
                        section_color: "#009EC1".into(),
                        hotkey: if idx < 9 {
                            (idx + 1).to_string()
                        } else {
                            String::new()
                        },
                        text: s.lines.join("\n"),
                        notes: item.notes.clone(),
                        title_text: item.title.clone(),
                        reference_text: s.reference,
                        style_json: style_json.clone(),
                        ..Default::default()
                    })
                    .collect(),
                _ => vec![LiveSlide {
                    index: 0,
                    label: reference.to_owned(),
                    section_kind: "scripture".into(),
                    section_color: "#009EC1".into(),
                    hotkey: "1".into(),
                    text: if !item.notes.trim().is_empty() {
                        item.notes.clone()
                    } else {
                        item.title.clone()
                    },
                    notes: item.notes.clone(),
                    title_text: item.title.clone(),
                    reference_text: format!("{reference} ({translation})"),
                    style_json,
                    ..Default::default()
                }],
            }
        }
        ServiceItemKind::Presentation => {
            let loaded_pres = item
                .content_id
                .as_ref()
                .filter(|id| !id.value.is_empty())
                .and_then(|id| library.get::<Presentation>(id).ok().flatten());
            if let Some(pres) = loaded_pres
                && !pres.slides.is_empty()
            {
                pres.slides
                    .into_iter()
                    .enumerate()
                    .map(|(idx, slide)| {
                        let slide_text = if !slide.text.trim().is_empty() {
                            slide.text.clone()
                        } else {
                            format!("{} — Slide {}", pres.name, idx + 1)
                        };
                        LiveSlide {
                            index: idx as u32,
                            label: format!("Slide {}", idx + 1),
                            section_kind: "presentation".into(),
                            section_color: "#2378C3".into(),
                            hotkey: if idx < 9 {
                                (idx + 1).to_string()
                            } else {
                                String::new()
                            },
                            text: slide_text,
                            notes: item.notes.clone(),
                            title_text: pres.name.clone(),
                            elements_json: slide.elements_json,
                            style_json: style_json.clone(),
                            ..Default::default()
                        }
                    })
                    .collect()
            } else {
                vec![LiveSlide {
                    index: 0,
                    label: "Slide 1".into(),
                    section_kind: "presentation".into(),
                    section_color: "#2378C3".into(),
                    hotkey: "1".into(),
                    text: item.title.clone(),
                    notes: item.notes.clone(),
                    title_text: item.title.clone(),
                    style_json,
                    ..Default::default()
                }]
            }
        }
        ServiceItemKind::Countdown => {
            let text = if item.countdown_message.trim().is_empty() {
                "{timer}".into()
            } else {
                format!("{}\n{{timer}}", item.countdown_message.trim())
            };
            vec![LiveSlide {
                index: 0,
                label: "Countdown".into(),
                section_kind: "countdown".into(),
                section_color: "#E66F0E".into(),
                hotkey: "1".into(),
                text,
                notes: item.notes.clone(),
                title_text: item.title.clone(),
                style_json,
                ..Default::default()
            }]
        }
        ServiceItemKind::Media => vec![LiveSlide {
            index: 0,
            label: "Media".into(),
            section_kind: "media".into(),
            section_color: "#009EC1".into(),
            hotkey: "1".into(),
            text: item.title.clone(),
            notes: item.notes.clone(),
            title_text: item.title.clone(),
            style_json,
            is_blank: true,
            ..Default::default()
        }],
        ServiceItemKind::Note => vec![LiveSlide {
            index: 0,
            label: "Note".into(),
            section_kind: "note".into(),
            section_color: "#71767A".into(),
            hotkey: "1".into(),
            text: String::new(),
            notes: if item.notes.trim().is_empty() {
                item.title.clone()
            } else {
                item.notes.clone()
            },
            title_text: item.title.clone(),
            style_json,
            is_blank: true,
            ..Default::default()
        }],
        ServiceItemKind::Audio | ServiceItemKind::Unspecified => vec![LiveSlide {
            index: 0,
            label: "Audio".into(),
            section_kind: "audio".into(),
            section_color: "#71767A".into(),
            hotkey: "1".into(),
            text: item.title.clone(),
            notes: item.notes.clone(),
            title_text: item.title.clone(),
            style_json,
            is_blank: true,
            ..Default::default()
        }],
    };

    if !slides.is_empty() {
        let action_idx = (item.slide_action_index as usize).min(slides.len() - 1);
        let has_slide_look = item
            .slide_look_id
            .as_ref()
            .is_some_and(|id| !id.value.is_empty());
        let has_slide_macro = item
            .slide_macro_id
            .as_ref()
            .is_some_and(|id| !id.value.is_empty());
        if has_slide_look || has_slide_macro {
            let target = &mut slides[action_idx];
            if has_slide_look {
                target.look_id = item.slide_look_id.clone();
                if let Some(lid) = item.slide_look_id.as_ref()
                    && let Ok(Some(look)) = library.get::<Look>(lid)
                {
                    target.action_badge = format!("Look: {}", look.name);
                }
            }
            if has_slide_macro {
                target.macro_id = item.slide_macro_id.clone();
                if target.action_badge.is_empty()
                    && let Some(mid) = item.slide_macro_id.as_ref()
                    && let Ok(Some(mac)) = library.get::<Macro>(mid)
                {
                    target.action_badge = format!("Macro: {}", mac.name);
                }
            }
        }
        if let Some(item_look) = item.look_id.as_ref().filter(|id| !id.value.is_empty())
            && slides[0].look_id.is_none()
        {
            slides[0].look_id = Some(item_look.clone());
            if slides[0].action_badge.is_empty() {
                let badge_name = if !item.look_name.trim().is_empty() {
                    item.look_name.trim().to_owned()
                } else {
                    library
                        .get::<Look>(item_look)
                        .ok()
                        .flatten()
                        .map(|l| l.name)
                        .unwrap_or_else(|| "Look".into())
                };
                slides[0].action_badge = format!("Look: {badge_name}");
            }
        }
        if let Some(item_macro) = item.macro_id.as_ref().filter(|id| !id.value.is_empty())
            && slides[0].macro_id.is_none()
        {
            slides[0].macro_id = Some(item_macro.clone());
        }
    }

    LiveItemSlides {
        item_id: item.id.clone(),
        item_index,
        kind: kind as i32,
        title: item.title.clone(),
        subtitle: item.subtitle.clone(),
        song_key,
        slides,
    }
}

fn is_triggerable_item(item: &ServiceItem) -> bool {
    ServiceItemKind::try_from(item.kind).unwrap_or(ServiceItemKind::Unspecified)
        != ServiceItemKind::Header
}

impl LiveState {
    pub fn open(config_dir: &Path) -> Self {
        let path = config_dir.join("live_state.bin");
        let machine_path = config_dir.join("live_machine.bin");
        let configs_path = config_dir.join("output_configs.bin");
        let scene = std::fs::read(&path)
            .ok()
            .and_then(|bytes| Scene::decode(bytes.as_slice()).ok())
            .unwrap_or_else(|| Scene {
                revision: 0,
                test_pattern: TestPattern::Unspecified as i32,
                text: String::new(),
                cleared: true,
                resolution: Some(Resolution {
                    width: 1920,
                    height: 1080,
                }),
                ..Default::default()
            });
        let stored_configs = std::fs::read(&configs_path)
            .ok()
            .and_then(|bytes| OutputConfigList::decode(bytes.as_slice()).ok())
            .filter(|list| !list.outputs.is_empty());
        let (output_configs, explicit_configs) = match stored_configs {
            Some(list) => (list.outputs, true),
            None => (vec![default_output_config()], false),
        };
        let mut snapshot = std::fs::read(&machine_path)
            .ok()
            .and_then(|bytes| LiveMachineSnapshot::decode(bytes.as_slice()).ok())
            .unwrap_or_else(|| LiveMachineSnapshot {
                status: Some(default_live_status(&scene)),
                ..Default::default()
            });
        let status = snapshot
            .status
            .get_or_insert_with(|| default_live_status(&scene));
        if status.layers.len() != 5 {
            status.layers = default_layer_statuses();
        }
        if status.audio.is_none() {
            status.audio = Some(AudioLayerStatus::default());
        }
        if status.automation.is_empty() {
            status.automation.push(LiveAutomationState::default());
        }
        status.scene_revision = scene.revision;
        status.cleared = scene.cleared;
        status.can_live_undo = !snapshot.undo_stack.is_empty();
        status.can_live_redo = !snapshot.redo_stack.is_empty();

        Self {
            path,
            machine_path,
            configs_path,
            scene,
            output_configs,
            explicit_configs,
            snapshot,
        }
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn status(&self) -> LiveStatus {
        self.snapshot
            .status
            .clone()
            .unwrap_or_else(|| default_live_status(&self.scene))
    }

    pub fn output_configs(&self) -> &[OutputConfig] {
        &self.output_configs
    }

    pub fn has_explicit_output_configs(&self) -> bool {
        self.explicit_configs
    }

    pub fn set_output_configs(&mut self, outputs: Vec<OutputConfig>) {
        self.output_configs = outputs;
        self.explicit_configs = true;
        let encoded = OutputConfigList {
            outputs: self.output_configs.clone(),
        }
        .encode_to_vec();
        if let Err(error) = selah_runtime::paths::write_atomic(&self.configs_path, &encoded) {
            tracing::error!(%error, "could not save output configs");
        }
    }

    fn push_undo(&mut self) {
        let current_status = self.status();
        self.snapshot.undo_stack.push(LiveSnapshotEntry {
            scene: Some(self.scene.clone()),
            status: Some(current_status),
        });
        if self.snapshot.undo_stack.len() > MAX_LIVE_UNDO {
            let excess = self.snapshot.undo_stack.len() - MAX_LIVE_UNDO;
            self.snapshot.undo_stack.drain(0..excess);
        }
        self.snapshot.redo_stack.clear();
    }

    fn sync_status_flags(&mut self) {
        let can_undo = !self.snapshot.undo_stack.is_empty();
        let can_redo = !self.snapshot.redo_stack.is_empty();
        let rev = self.scene.revision;
        let cleared = self.scene.cleared;
        let show_logo = self.scene.outputs.iter().any(|o| o.show_logo);
        let status = self
            .snapshot
            .status
            .get_or_insert_with(|| default_live_status(&self.scene));
        status.scene_revision = rev;
        status.cleared = cleared;
        status.show_logo = show_logo;
        status.can_live_undo = can_undo;
        status.can_live_redo = can_redo;
    }

    /// Apply a change, bump the revision and save the snapshot before anything is sent.
    pub fn change(&mut self, edit: impl FnOnce(&mut Scene)) -> &Scene {
        self.push_undo();
        let mut next = self.scene.clone();
        edit(&mut next);
        next.revision = self.scene.revision + 1;
        self.scene = next;
        if self.scene.cleared {
            if let Some(status) = self.snapshot.status.as_mut() {
                for l in &mut status.layers {
                    l.active = false;
                    l.title.clear();
                    l.subtitle.clear();
                    l.spec = None;
                }
                if let Some(audio) = status.audio.as_mut() {
                    audio.playing = false;
                }
            }
            self.scene.audio_layers.clear();
        }
        self.sync_status_flags();
        self.save();
        &self.scene
    }

    /// Make sure our next revision is above `floor` (after the emergency lane acted).
    pub fn raise_revision_above(&mut self, floor: u64) {
        if self.scene.revision <= floor {
            self.scene.revision = floor;
        }
        self.sync_status_flags();
    }

    pub fn load_service(
        &mut self,
        library: &Library,
        service_id: &Id,
        trigger_first_item: bool,
    ) -> Result<LiveStatus, CommandBusError> {
        validate_id(service_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let service = library
            .get::<Service>(service_id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("service not found".into()))?;
        let item_slides: Vec<LiveItemSlides> = service
            .items
            .iter()
            .enumerate()
            .map(|(idx, item)| resolve_item_slides(library, &service, idx as u32, item))
            .collect();

        let first_idx = service
            .items
            .iter()
            .position(is_triggerable_item)
            .unwrap_or(0) as u32;

        self.snapshot.service = Some(service.clone());
        self.snapshot.item_slides = item_slides;

        {
            let status = self
                .snapshot
                .status
                .get_or_insert_with(|| default_live_status(&self.scene));
            status.service_id = service.id.clone();
            status.service_name = service.name.clone();
            status.has_active_item = false;
            status.current_item_index = first_idx;
            status.current_slide_index = 0;
        }
        self.refresh_cursor_metadata();

        if trigger_first_item && service.items.iter().any(is_triggerable_item) {
            return self.trigger_slide(library, first_idx, 0);
        }

        self.sync_status_flags();
        self.save_machine();
        Ok(self.status())
    }

    fn compute_next_cursor(&self, item_index: u32, slide_index: u32) -> Option<(u32, u32)> {
        let service = self.snapshot.service.as_ref()?;
        let cur_slides = self
            .snapshot
            .item_slides
            .get(item_index as usize)
            .map_or(0, |s| s.slides.len() as u32);
        if slide_index + 1 < cur_slides {
            return Some((item_index, slide_index + 1));
        }
        for next_item_idx in (item_index as usize + 1)..service.items.len() {
            if is_triggerable_item(&service.items[next_item_idx]) {
                return Some((next_item_idx as u32, 0));
            }
        }
        None
    }

    fn compute_previous_cursor(&self, item_index: u32, slide_index: u32) -> Option<(u32, u32)> {
        let service = self.snapshot.service.as_ref()?;
        if slide_index > 0 {
            return Some((item_index, slide_index - 1));
        }
        for prev_item_idx in (0..item_index as usize).rev() {
            if is_triggerable_item(&service.items[prev_item_idx]) {
                let count = self
                    .snapshot
                    .item_slides
                    .get(prev_item_idx)
                    .map_or(1, |s| s.slides.len().max(1) as u32);
                return Some((prev_item_idx as u32, count - 1));
            }
        }
        None
    }

    fn refresh_cursor_metadata(&mut self) {
        let Some(service) = self.snapshot.service.as_ref() else {
            return;
        };
        let status = self
            .snapshot
            .status
            .get_or_insert_with(|| default_live_status(&self.scene));
        let cur_item_idx = status.current_item_index;
        let cur_slide_idx = status.current_slide_index;

        if let Some(item_slides) = self.snapshot.item_slides.get(cur_item_idx as usize) {
            status.current_item_title = item_slides.title.clone();
            status.song_key = item_slides.song_key.clone();
            status.current_item_slides = item_slides.slides.clone();
            if let Some(slide) = item_slides.slides.get(cur_slide_idx as usize) {
                status.current_slide_label = slide.label.clone();
                status.current_slide_text = slide.text.clone();
                status.current_chords_json = slide.chords_json.clone();
                status.current_notes = slide.notes.clone();
            }
        }

        let next_pos = if status.has_active_item {
            self.compute_next_cursor(cur_item_idx, cur_slide_idx)
        } else {
            service
                .items
                .iter()
                .position(is_triggerable_item)
                .map(|idx| (idx as u32, 0))
        };

        if let Some((next_i, next_s)) = next_pos {
            let status = self.snapshot.status.as_mut().expect("status initialized");
            status.has_next = true;
            status.next_item_index = next_i;
            status.next_slide_index = next_s;
            if let Some(next_item) = self.snapshot.item_slides.get(next_i as usize) {
                status.next_item_title = next_item.title.clone();
                if let Some(ns) = next_item.slides.get(next_s as usize) {
                    status.next_slide_label = ns.label.clone();
                    status.next_slide_text = ns.text.clone();
                } else {
                    status.next_slide_label.clear();
                    status.next_slide_text.clear();
                }
            }
        } else {
            let status = self.snapshot.status.as_mut().expect("status initialized");
            status.has_next = false;
            status.next_item_title.clear();
            status.next_slide_label.clear();
            status.next_slide_text.clear();
        }
    }

    fn set_layer_slot(
        &mut self,
        kind: LayerKind,
        active: bool,
        title: String,
        subtitle: String,
        spec: Option<LayerSpec>,
    ) {
        let status = self
            .snapshot
            .status
            .get_or_insert_with(|| default_live_status(&self.scene));
        if status.layers.len() != 5 {
            status.layers = default_layer_statuses();
        }
        if let Some(slot) = status.layers.iter_mut().find(|l| l.kind == kind as i32) {
            slot.active = active;
            slot.title = title;
            slot.subtitle = subtitle;
            slot.spec = spec;
        }
    }

    fn build_preload_hints(&self) -> Vec<PreloadHint> {
        let mut hints = Vec::new();
        let Some(status) = self.snapshot.status.as_ref() else {
            return hints;
        };
        let res = self
            .output_configs
            .first()
            .and_then(|c| c.resolution)
            .or(Some(Resolution {
                width: 1920,
                height: 1080,
            }));
        if let Some(cur_item) = self
            .snapshot
            .item_slides
            .get(status.current_item_index as usize)
            && let Some(slide) = cur_item.slides.get(status.current_slide_index as usize)
            && !slide.text.is_empty()
        {
            hints.push(PreloadHint {
                key: format!(
                    "item-{}-slide-{}",
                    status.current_item_index, status.current_slide_index
                ),
                output_index: 0,
                resolution: res,
                text: slide.text.clone(),
                style_json: slide.style_json.clone(),
                elements_json: slide.elements_json.clone(),
                media_hash: String::new(),
            });
        }
        if status.has_next
            && let Some(next_item) = self
                .snapshot
                .item_slides
                .get(status.next_item_index as usize)
            && let Some(slide) = next_item.slides.get(status.next_slide_index as usize)
            && !slide.text.is_empty()
        {
            hints.push(PreloadHint {
                key: format!(
                    "item-{}-slide-{}",
                    status.next_item_index, status.next_slide_index
                ),
                output_index: 0,
                resolution: res,
                text: slide.text.clone(),
                style_json: slide.style_json.clone(),
                elements_json: slide.elements_json.clone(),
                media_hash: String::new(),
            });
        }
        hints
    }

    fn rebuild_scene_from_status(&mut self, cleared: bool, show_logo: bool) {
        let status = self
            .snapshot
            .status
            .get_or_insert_with(|| default_live_status(&self.scene))
            .clone();
        let active_layers: Vec<LayerSpec> = status
            .layers
            .iter()
            .filter(|l| l.active)
            .filter_map(|l| l.spec.clone())
            .collect();
        let stage_style_json = TemplateStyle::preset_stage_monitor()
            .to_json()
            .unwrap_or_default();
        let active_look = self.snapshot.active_look.clone();

        let mut outputs = Vec::new();
        for cfg in self.output_configs.iter().filter(|c| c.enabled) {
            let role = OutputRole::try_from(cfg.role).unwrap_or(OutputRole::Main);
            let res = cfg.resolution.or(Some(Resolution {
                width: 1920,
                height: 1080,
            }));
            let out_show_logo = if role == OutputRole::Stage {
                false
            } else {
                show_logo
            };
            let look_rule = active_look.as_ref().and_then(|look| {
                look.rules
                    .iter()
                    .find(|r| r.output_index == cfg.output_index)
            });
            let out_layers: Vec<LayerSpec> = active_layers
                .iter()
                .filter(|spec| {
                    if let Some(rule) = look_rule {
                        rule.enabled_layers.contains(&spec.kind)
                    } else {
                        true
                    }
                })
                .map(|spec| {
                    let mut cloned = spec.clone();
                    if role == OutputRole::Stage
                        && let Some(layer_spec::Content::Slide(ref mut s)) = cloned.content
                    {
                        s.style_json = stage_style_json.clone();
                    } else if let Some(rule) = look_rule
                        && !rule.template_style_json.trim().is_empty()
                        && let Some(layer_spec::Content::Slide(ref mut s)) = cloned.content
                    {
                        s.style_json = rule.template_style_json.clone();
                    }
                    cloned
                })
                .collect();
            let mut effects = Vec::new();
            if look_rule.is_some_and(|r| r.mask_enabled) {
                effects.push(EffectSpec {
                    kind: EffectKind::Mask as i32,
                    enabled: true,
                    intensity: 1.0,
                    params_json: String::new(),
                });
            }
            outputs.push(OutputScene {
                output_index: cfg.output_index,
                role: role as i32,
                resolution: res,
                cleared,
                show_logo: out_show_logo,
                test_pattern: TestPattern::Unspecified as i32,
                layers: out_layers,
                transition: None,
                effects,
            });
        }

        let audio_layers = status
            .audio
            .as_ref()
            .filter(|a| a.playing && !a.content_hash.trim().is_empty())
            .map(|a| {
                vec![MediaLayerSpec {
                    content_hash: a.content_hash.clone(),
                    media_kind: "audio".into(),
                    media_extension: a.media_extension.clone(),
                    loop_media: a.loop_audio,
                    volume: if a.volume > 0.0 { a.volume } else { 0.8 },
                    paused: false,
                    ..Default::default()
                }]
            })
            .unwrap_or_default();

        let preload_items = self.build_preload_hints();
        let next_revision = self.scene.revision + 1;
        self.scene = Scene {
            revision: next_revision,
            test_pattern: TestPattern::Unspecified as i32,
            text: if cleared || show_logo {
                String::new()
            } else {
                status.current_slide_text.clone()
            },
            cleared,
            resolution: Some(Resolution {
                width: 1920,
                height: 1080,
            }),
            outputs,
            is_preview: false,
            preload_items,
            audio_layers,
        };
        self.sync_status_flags();
        self.save();
    }

    pub fn trigger_slide(
        &mut self,
        library: &Library,
        item_index: u32,
        slide_index: u32,
    ) -> Result<LiveStatus, CommandBusError> {
        let Some(service) = self.snapshot.service.clone() else {
            return Err(CommandBusError::Invalid("no live service is loaded".into()));
        };
        let Some(item) = service.items.get(item_index as usize).cloned() else {
            return Err(CommandBusError::NotFound(format!(
                "service item index {item_index} out of range"
            )));
        };
        if !is_triggerable_item(&item) {
            return Err(CommandBusError::Invalid(
                "cannot trigger a service header item".into(),
            ));
        }

        // Re-resolve item slides from the library in case the song/presentation was edited.
        let resolved = resolve_item_slides(library, &service, item_index, &item);
        if (slide_index as usize) >= resolved.slides.len() {
            return Err(CommandBusError::NotFound(format!(
                "slide index {slide_index} out of range for item {item_index}"
            )));
        }
        if (item_index as usize) < self.snapshot.item_slides.len() {
            self.snapshot.item_slides[item_index as usize] = resolved.clone();
        }
        let slide = resolved.slides[slide_index as usize].clone();

        self.push_undo();

        {
            let status = self
                .snapshot
                .status
                .get_or_insert_with(|| default_live_status(&self.scene));
            status.has_active_item = true;
            status.current_item_index = item_index;
            status.current_slide_index = slide_index;
        }
        self.refresh_cursor_metadata();

        let kind = ServiceItemKind::try_from(item.kind).unwrap_or(ServiceItemKind::Song);
        match kind {
            ServiceItemKind::Audio => {
                let media_id = item
                    .content_id
                    .as_ref()
                    .filter(|id| !id.value.is_empty())
                    .or(item
                        .background_media_id
                        .as_ref()
                        .filter(|id| !id.value.is_empty()));
                if let Some(mid) = media_id
                    && let Ok(Some(media)) = library.get::<MediaItem>(mid)
                {
                    let ext = media_extension_from_item(&media.extension, MediaKind::Audio);
                    if let Some(status) = self.snapshot.status.as_mut() {
                        status.audio = Some(AudioLayerStatus {
                            playing: true,
                            media_id: Some(mid.clone()),
                            title: media.name,
                            content_hash: media.content_hash,
                            media_extension: ext,
                            volume: 0.8,
                            loop_audio: false,
                        });
                    }
                } else if let Some(status) = self.snapshot.status.as_mut() {
                    status.audio = Some(AudioLayerStatus {
                        playing: true,
                        media_id: item.content_id.clone(),
                        title: item.title.clone(),
                        content_hash: String::new(),
                        media_extension: "wav".into(),
                        volume: 0.8,
                        loop_audio: false,
                    });
                }
                let currently_cleared = self.scene.cleared
                    && !self
                        .snapshot
                        .status
                        .as_ref()
                        .is_some_and(|s| s.layers.iter().any(|l| l.active));
                let currently_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
                self.rebuild_scene_from_status(currently_cleared, currently_logo);
            }
            ServiceItemKind::Media => {
                let media_id = item
                    .content_id
                    .as_ref()
                    .filter(|id| !id.value.is_empty())
                    .or(item
                        .background_media_id
                        .as_ref()
                        .filter(|id| !id.value.is_empty()));
                let media_spec = if let Some(mid) = media_id
                    && let Ok(Some(media)) = library.get::<MediaItem>(mid)
                {
                    let mkind = MediaKind::try_from(media.kind).unwrap_or(MediaKind::Image);
                    MediaLayerSpec {
                        content_hash: media.content_hash,
                        media_kind: if mkind == MediaKind::Video {
                            "video".into()
                        } else {
                            "image".into()
                        },
                        media_extension: media_extension_from_item(&media.extension, mkind),
                        fit: MediaFitMode::Fill as i32,
                        loop_media: mkind == MediaKind::Video,
                        volume: 0.8,
                        ..Default::default()
                    }
                } else {
                    MediaLayerSpec {
                        solid_color: Some(selah_proto::v1::Color {
                            red: 0.09,
                            green: 0.18,
                            blue: 0.32,
                            alpha: 1.0,
                        }),
                        ..Default::default()
                    }
                };
                self.set_layer_slot(
                    LayerKind::Media,
                    true,
                    item.title.clone(),
                    item.subtitle.clone(),
                    Some(LayerSpec {
                        kind: LayerKind::Media as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Media(media_spec)),
                        ..Default::default()
                    }),
                );
                self.rebuild_scene_from_status(false, false);
            }
            ServiceItemKind::Note => {
                self.set_layer_slot(LayerKind::Slide, false, String::new(), String::new(), None);
                let any_active = self
                    .snapshot
                    .status
                    .as_ref()
                    .is_some_and(|s| s.layers.iter().any(|l| l.active));
                self.rebuild_scene_from_status(!any_active, false);
            }
            _ => {
                if let Some(bg_id) = item
                    .background_media_id
                    .as_ref()
                    .filter(|id| !id.value.is_empty())
                    && let Ok(Some(media)) = library.get::<MediaItem>(bg_id)
                {
                    let mkind = MediaKind::try_from(media.kind).unwrap_or(MediaKind::Image);
                    let media_spec = MediaLayerSpec {
                        content_hash: media.content_hash,
                        media_kind: if mkind == MediaKind::Video {
                            "video".into()
                        } else {
                            "image".into()
                        },
                        media_extension: media_extension_from_item(&media.extension, mkind),
                        fit: MediaFitMode::Fill as i32,
                        loop_media: mkind == MediaKind::Video,
                        volume: 0.8,
                        ..Default::default()
                    };
                    self.set_layer_slot(
                        LayerKind::Media,
                        true,
                        media.name,
                        "Background".into(),
                        Some(LayerSpec {
                            kind: LayerKind::Media as i32,
                            enabled: true,
                            content: Some(layer_spec::Content::Media(media_spec)),
                            ..Default::default()
                        }),
                    );
                }

                let timer_target_unix_ms = if kind == ServiceItemKind::Countdown {
                    let secs = if item.countdown_seconds > 0 {
                        item.countdown_seconds
                    } else if item.duration_seconds > 0 {
                        item.duration_seconds
                    } else {
                        300
                    };
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as i64);
                    now_ms + i64::from(secs) * 1000
                } else {
                    0
                };

                let slide_spec = SlideLayerSpec {
                    text: slide.text.clone(),
                    elements_json: slide.elements_json.clone(),
                    style_json: slide.style_json.clone(),
                    test_pattern: TestPattern::Unspecified as i32,
                    title_text: slide.title_text.clone(),
                    reference_text: slide.reference_text.clone(),
                    footer_text: slide.footer_text.clone(),
                    chords_json: slide.chords_json.clone(),
                    translation_text: slide.translation_text.clone(),
                    timer_target_unix_ms,
                    ..Default::default()
                };
                self.set_layer_slot(
                    LayerKind::Slide,
                    !slide.is_blank,
                    item.title.clone(),
                    slide.label.clone(),
                    (!slide.is_blank).then_some(LayerSpec {
                        kind: LayerKind::Slide as i32,
                        enabled: true,
                        content: Some(layer_spec::Content::Slide(slide_spec)),
                        ..Default::default()
                    }),
                );
                let any_active = self
                    .snapshot
                    .status
                    .as_ref()
                    .is_some_and(|s| s.layers.iter().any(|l| l.active));
                self.rebuild_scene_from_status(!any_active, false);
            }
        }

        // Execute slide actions (Look / Macro) if attached to this slide.
        if let Some(ref look_id) = slide.look_id
            && !look_id.value.is_empty()
        {
            let _ = self.apply_look_internal(library, look_id);
        }
        if let Some(ref macro_id) = slide.macro_id
            && !macro_id.value.is_empty()
        {
            let _ = self.execute_macro_actions_internal(library, macro_id, 0);
        }
        if slide
            .look_id
            .as_ref()
            .is_some_and(|id| !id.value.is_empty())
            || slide
                .macro_id
                .as_ref()
                .is_some_and(|id| !id.value.is_empty())
        {
            let any_active = self
                .snapshot
                .status
                .as_ref()
                .is_some_and(|s| s.layers.iter().any(|l| l.active));
            let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
            self.rebuild_scene_from_status(!any_active && !show_logo, show_logo);
        }

        Ok(self.status())
    }

    pub fn trigger_item(
        &mut self,
        library: &Library,
        item_index: u32,
    ) -> Result<LiveStatus, CommandBusError> {
        self.trigger_slide(library, item_index, 0)
    }

    pub fn next_slide(&mut self, library: &Library) -> Result<LiveStatus, CommandBusError> {
        let Some(service) = self.snapshot.service.as_ref() else {
            return Err(CommandBusError::Invalid("no live service is loaded".into()));
        };
        let status = self.status();
        if !status.has_active_item {
            if let Some(first_idx) = service.items.iter().position(is_triggerable_item) {
                return self.trigger_slide(library, first_idx as u32, 0);
            }
            return Err(CommandBusError::Invalid(
                "service has no triggerable items".into(),
            ));
        }
        if let Some((next_item, next_slide)) =
            self.compute_next_cursor(status.current_item_index, status.current_slide_index)
        {
            self.trigger_slide(library, next_item, next_slide)
        } else {
            Ok(status)
        }
    }

    pub fn previous_slide(&mut self, library: &Library) -> Result<LiveStatus, CommandBusError> {
        let status = self.status();
        if !status.has_active_item {
            return Ok(status);
        }
        if let Some((prev_item, prev_slide)) =
            self.compute_previous_cursor(status.current_item_index, status.current_slide_index)
        {
            self.trigger_slide(library, prev_item, prev_slide)
        } else {
            Ok(status)
        }
    }

    pub fn next_item(&mut self, library: &Library) -> Result<LiveStatus, CommandBusError> {
        let Some(service) = self.snapshot.service.as_ref() else {
            return Err(CommandBusError::Invalid("no live service is loaded".into()));
        };
        let status = self.status();
        let start_from = if status.has_active_item {
            status.current_item_index as usize + 1
        } else {
            0
        };
        for idx in start_from..service.items.len() {
            if is_triggerable_item(&service.items[idx]) {
                return self.trigger_slide(library, idx as u32, 0);
            }
        }
        Ok(status)
    }

    pub fn previous_item(&mut self, library: &Library) -> Result<LiveStatus, CommandBusError> {
        let Some(service) = self.snapshot.service.as_ref() else {
            return Err(CommandBusError::Invalid("no live service is loaded".into()));
        };
        let status = self.status();
        if !status.has_active_item {
            return Ok(status);
        }
        for idx in (0..status.current_item_index as usize).rev() {
            if is_triggerable_item(&service.items[idx]) {
                return self.trigger_slide(library, idx as u32, 0);
            }
        }
        Ok(status)
    }

    pub fn trigger_section_hotkey(
        &mut self,
        library: &Library,
        hotkey: &str,
    ) -> Result<LiveStatus, CommandBusError> {
        let target = hotkey.trim();
        if target.is_empty() {
            return Err(CommandBusError::Invalid("hotkey must not be empty".into()));
        }
        let status = self.status();
        let Some(item_slides) = self
            .snapshot
            .item_slides
            .get(status.current_item_index as usize)
        else {
            return Err(CommandBusError::Invalid(
                "no active item for section hotkey".into(),
            ));
        };
        let found_idx = item_slides
            .slides
            .iter()
            .find(|s| s.hotkey.eq_ignore_ascii_case(target))
            .or_else(|| {
                item_slides.slides.iter().find(|s| {
                    s.label
                        .trim()
                        .chars()
                        .next()
                        .is_some_and(|c| c.to_string().eq_ignore_ascii_case(target))
                })
            })
            .map(|s| s.index)
            .ok_or_else(|| {
                CommandBusError::NotFound(format!("no section bound to hotkey '{target}'"))
            })?;
        self.trigger_slide(library, status.current_item_index, found_idx)
    }

    pub fn clear_layer(&mut self, kind: LayerKind) -> Result<LiveStatus, CommandBusError> {
        if kind == LayerKind::Unspecified {
            return Err(CommandBusError::Invalid(
                "layer kind must be specified".into(),
            ));
        }
        self.push_undo();
        self.set_layer_slot(kind, false, String::new(), String::new(), None);
        let any_active = self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.layers.iter().any(|l| l.active));
        let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
        self.rebuild_scene_from_status(!any_active && !show_logo, show_logo);
        Ok(self.status())
    }

    pub fn set_layer_state(&mut self, mut spec: LayerSpec) -> Result<LiveStatus, CommandBusError> {
        let kind = selah_render::scene::layer_kind_of(&spec);
        if kind == LayerKind::Unspecified {
            return Err(CommandBusError::Invalid(
                "layer spec must specify a valid LayerKind".into(),
            ));
        }
        spec.kind = kind as i32;
        let test_scene = Scene {
            outputs: vec![OutputScene {
                output_index: 0,
                layers: vec![spec.clone()],
                ..Default::default()
            }],
            ..Default::default()
        };
        selah_render::effects::validate_scene_shader_chains(&test_scene)
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        selah_render::scene::validate_scene_elements(&test_scene)
            .map_err(|e| CommandBusError::Invalid(e.to_string()))?;

        self.push_undo();
        let active = spec.enabled && selah_render::scene::layer_has_content(&spec);
        let title = match spec.content.as_ref() {
            Some(layer_spec::Content::Slide(s)) => {
                if !s.title_text.is_empty() {
                    s.title_text.clone()
                } else {
                    s.text.lines().next().unwrap_or("Slide").to_owned()
                }
            }
            Some(layer_spec::Content::Props(p)) => p.label.clone(),
            Some(layer_spec::Content::Messages(m)) => m.text.clone(),
            Some(layer_spec::Content::VideoInput(v)) => v.source_id.clone(),
            Some(layer_spec::Content::Media(m)) => m.media_kind.clone(),
            None => String::new(),
        };
        self.set_layer_slot(kind, active, title, String::new(), active.then_some(spec));
        let any_active = self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.layers.iter().any(|l| l.active));
        self.rebuild_scene_from_status(!any_active, false);
        Ok(self.status())
    }

    pub fn set_show_logo(&mut self, show_logo: bool) -> LiveStatus {
        self.push_undo();
        let any_active = self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.layers.iter().any(|l| l.active));
        let cleared = !show_logo && !any_active;
        self.rebuild_scene_from_status(cleared, show_logo);
        self.status()
    }

    pub fn clear_all(&mut self) -> LiveStatus {
        self.push_undo();
        if let Some(status) = self.snapshot.status.as_mut() {
            for l in &mut status.layers {
                l.active = false;
                l.title.clear();
                l.subtitle.clear();
                l.spec = None;
            }
            if let Some(audio) = status.audio.as_mut() {
                audio.playing = false;
            }
            status.show_logo = false;
            status.cleared = true;
        }
        self.rebuild_scene_from_status(true, false);
        self.status()
    }

    pub fn play_audio_layer(
        &mut self,
        library: &Library,
        media_id: &Id,
        volume: f32,
        loop_audio: bool,
    ) -> Result<LiveStatus, CommandBusError> {
        validate_id(media_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let media = library
            .get::<MediaItem>(media_id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("audio media item not found".into()))?;
        let mkind = MediaKind::try_from(media.kind).unwrap_or(MediaKind::Audio);
        if mkind != MediaKind::Audio && mkind != MediaKind::Video {
            return Err(CommandBusError::Invalid(
                "media item is not an audio or video stream".into(),
            ));
        }
        self.push_undo();
        let effective_vol = if volume.is_finite() && volume > 0.0 {
            volume.clamp(0.0, 2.0)
        } else {
            0.8
        };
        let ext = media_extension_from_item(&media.extension, mkind);
        if let Some(status) = self.snapshot.status.as_mut() {
            status.audio = Some(AudioLayerStatus {
                playing: true,
                media_id: Some(media_id.clone()),
                title: media.name,
                content_hash: media.content_hash,
                media_extension: ext,
                volume: effective_vol,
                loop_audio,
            });
        }
        let cleared = self.scene.cleared;
        let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
        self.rebuild_scene_from_status(cleared, show_logo);
        Ok(self.status())
    }

    pub fn stop_audio_layer(&mut self) -> LiveStatus {
        self.push_undo();
        if let Some(status) = self.snapshot.status.as_mut()
            && let Some(audio) = status.audio.as_mut()
        {
            audio.playing = false;
        }
        let cleared = self.scene.cleared;
        let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
        self.rebuild_scene_from_status(cleared, show_logo);
        self.status()
    }

    pub fn live_undo(&mut self) -> Result<LiveStatus, CommandBusError> {
        let Some(prev) = self.snapshot.undo_stack.pop() else {
            return Err(CommandBusError::NothingToUndo);
        };
        let cur_status = self.status();
        self.snapshot.redo_stack.push(LiveSnapshotEntry {
            scene: Some(self.scene.clone()),
            status: Some(cur_status),
        });
        let next_revision = self.scene.revision + 1;
        if let Some(mut restored_scene) = prev.scene {
            restored_scene.revision = next_revision;
            self.scene = restored_scene;
        } else {
            self.scene.revision = next_revision;
        }
        if let Some(restored_status) = prev.status {
            self.snapshot.status = Some(restored_status);
        }
        self.sync_status_flags();
        self.save();
        Ok(self.status())
    }

    fn automation_mut(&mut self) -> &mut LiveAutomationState {
        let status = self
            .snapshot
            .status
            .get_or_insert_with(|| default_live_status(&self.scene));
        if status.automation.is_empty() {
            status.automation.push(LiveAutomationState::default());
        }
        &mut status.automation[0]
    }

    pub fn is_booth_locked(&self) -> bool {
        self.snapshot
            .status
            .as_ref()
            .and_then(|s| s.automation.first())
            .is_some_and(|a| a.booth_locked)
    }

    /// Core's role lock (SEC-14), mirrored into every status so the UI can show Simple mode.
    pub fn set_role_locked(&mut self, locked: bool) -> LiveStatus {
        self.automation_mut().role_locked = locked;
        self.sync_status_flags();
        self.save_machine();
        self.status()
    }

    pub fn set_booth_lock(&mut self, locked: bool) -> LiveStatus {
        self.automation_mut().booth_locked = locked;
        self.sync_status_flags();
        self.save_machine();
        self.status()
    }

    pub fn send_booth_message(&mut self, text: &str) -> Result<LiveStatus, CommandBusError> {
        if text.contains('\0') || text.chars().count() > 512 {
            return Err(CommandBusError::Invalid(
                "booth message must be at most 512 characters and contain no NUL bytes".into(),
            ));
        }
        self.automation_mut().booth_message = text.trim().to_owned();
        self.sync_status_flags();
        self.save_machine();
        Ok(self.status())
    }

    fn apply_look_internal(
        &mut self,
        library: &Library,
        look_id: &Id,
    ) -> Result<(), CommandBusError> {
        if look_id.value.is_empty() {
            self.snapshot.active_look = None;
            let auto = self.automation_mut();
            auto.active_look_id = None;
            auto.active_look_name.clear();
            return Ok(());
        }
        validate_id(look_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let mut look = library
            .get::<Look>(look_id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("look not found".into()))?;
        for rule in &mut look.rules {
            if rule.template_style_json.trim().is_empty() {
                rule.template_style_json =
                    resolve_template_style_json(library, rule.template_override_id.as_ref());
            }
        }
        let look_name = look.name.clone();
        let look_id_val = look.id.clone();
        self.snapshot.active_look = Some(look);
        let auto = self.automation_mut();
        auto.active_look_id = look_id_val;
        auto.active_look_name = look_name;
        Ok(())
    }

    pub fn set_look(
        &mut self,
        library: &Library,
        look_id: &Id,
    ) -> Result<LiveStatus, CommandBusError> {
        self.push_undo();
        self.apply_look_internal(library, look_id)?;
        let any_active = self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.layers.iter().any(|l| l.active));
        let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
        self.rebuild_scene_from_status(!any_active && !show_logo, show_logo);
        Ok(self.status())
    }

    fn show_prop_internal(
        &mut self,
        library: &Library,
        prop_id: &Id,
    ) -> Result<(), CommandBusError> {
        validate_id(prop_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let prop = library
            .get::<Prop>(prop_id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("prop not found".into()))?;
        let label = if !prop.label.trim().is_empty() {
            prop.label.clone()
        } else {
            prop.name.clone()
        };
        let props_spec = PropsLayerSpec {
            label: label.clone(),
            elements_json: prop.elements_json.clone(),
            background_color: prop.background_color,
            text_color: prop.text_color,
            use_opacity: true,
            opacity: 1.0,
            effects: Vec::new(),
        };
        self.set_layer_slot(
            LayerKind::Props,
            true,
            prop.name.clone(),
            label,
            Some(LayerSpec {
                kind: LayerKind::Props as i32,
                enabled: true,
                content: Some(layer_spec::Content::Props(props_spec)),
                ..Default::default()
            }),
        );
        let auto = self.automation_mut();
        auto.active_prop_id = prop.id;
        auto.active_prop_name = prop.name;
        Ok(())
    }

    pub fn show_prop(
        &mut self,
        library: &Library,
        prop_id: &Id,
    ) -> Result<LiveStatus, CommandBusError> {
        self.push_undo();
        self.show_prop_internal(library, prop_id)?;
        let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
        self.rebuild_scene_from_status(false, show_logo);
        Ok(self.status())
    }

    fn hide_prop_internal(&mut self) {
        self.set_layer_slot(LayerKind::Props, false, String::new(), String::new(), None);
        let auto = self.automation_mut();
        auto.active_prop_id = None;
        auto.active_prop_name.clear();
    }

    pub fn hide_prop(&mut self, _prop_id: Option<&Id>) -> Result<LiveStatus, CommandBusError> {
        self.push_undo();
        self.hide_prop_internal();
        let any_active = self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.layers.iter().any(|l| l.active));
        let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
        self.rebuild_scene_from_status(!any_active && !show_logo, show_logo);
        Ok(self.status())
    }

    fn show_message_internal(
        &mut self,
        library: &Library,
        cmd: &ShowMessage,
    ) -> Result<(), CommandBusError> {
        let (
            template_text,
            default_tokens,
            duration_secs,
            stage_only,
            bg_color,
            fg_color,
            msg_id,
            timer_id,
        ) = if let Some(ref mid) = cmd.message_id
            && !mid.value.is_empty()
        {
            validate_id(mid).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
            let msg = library
                .get::<LiveMessage>(mid)
                .map_err(CommandBusError::Internal)?
                .ok_or_else(|| CommandBusError::NotFound("message not found".into()))?;
            let tpl = if !cmd.custom_text.trim().is_empty() {
                cmd.custom_text.clone()
            } else {
                msg.template_text
            };
            let dur = if cmd.duration_seconds > 0 {
                cmd.duration_seconds
            } else {
                msg.duration_seconds
            };
            (
                tpl,
                msg.default_tokens,
                dur,
                cmd.stage_only || msg.stage_only,
                msg.background_color,
                msg.text_color,
                Some(mid.clone()),
                msg.timer_id,
            )
        } else {
            (
                cmd.custom_text.clone(),
                HashMap::new(),
                cmd.duration_seconds,
                cmd.stage_only,
                None,
                None,
                None,
                None,
            )
        };

        let countdown_text = {
            let auto = self.automation_mut();
            if let Some(ref tid) = timer_id
                && !tid.value.is_empty()
            {
                auto.timers
                    .iter()
                    .find(|t| t.timer_id.as_ref() == Some(tid))
                    .map(|t| t.formatted_time.clone())
                    .unwrap_or_else(|| "00:00".into())
            } else {
                auto.timers
                    .iter()
                    .find(|t| t.running)
                    .or_else(|| auto.timers.first())
                    .map(|t| t.formatted_time.clone())
                    .unwrap_or_else(|| "00:00".into())
            }
        };

        let rendered = interpolate_message_tokens(
            &template_text,
            &default_tokens,
            &cmd.token_values,
            &countdown_text,
        )?;
        let remaining_ms = u64::from(duration_secs).saturating_mul(1000);
        {
            let auto = self.automation_mut();
            auto.active_message_id = msg_id;
            auto.active_message_text = rendered.clone();
            auto.active_message_remaining_ms = remaining_ms;
            if stage_only {
                auto.booth_message = rendered.clone();
            }
        }
        if stage_only {
            self.set_layer_slot(
                LayerKind::Messages,
                false,
                String::new(),
                String::new(),
                None,
            );
        } else {
            let msg_spec = MessagesLayerSpec {
                text: rendered.clone(),
                background_color: bg_color,
                text_color: fg_color,
                ticker: false,
                use_opacity: true,
                opacity: 1.0,
                effects: Vec::new(),
            };
            self.set_layer_slot(
                LayerKind::Messages,
                true,
                rendered,
                "Message".into(),
                Some(LayerSpec {
                    kind: LayerKind::Messages as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Messages(msg_spec)),
                    ..Default::default()
                }),
            );
        }
        Ok(())
    }

    pub fn show_message(
        &mut self,
        library: &Library,
        cmd: &ShowMessage,
    ) -> Result<LiveStatus, CommandBusError> {
        self.push_undo();
        self.show_message_internal(library, cmd)?;
        let any_active = self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.layers.iter().any(|l| l.active));
        let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
        self.rebuild_scene_from_status(!any_active && !show_logo, show_logo);
        Ok(self.status())
    }

    fn hide_message_internal(&mut self) {
        self.set_layer_slot(
            LayerKind::Messages,
            false,
            String::new(),
            String::new(),
            None,
        );
        let auto = self.automation_mut();
        auto.active_message_id = None;
        auto.active_message_text.clear();
        auto.active_message_remaining_ms = 0;
    }

    pub fn hide_message(&mut self) -> Result<LiveStatus, CommandBusError> {
        self.push_undo();
        self.hide_message_internal();
        let any_active = self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.layers.iter().any(|l| l.active));
        let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
        self.rebuild_scene_from_status(!any_active && !show_logo, show_logo);
        Ok(self.status())
    }

    fn compute_timer_duration_ms(&self, timer: &Timer) -> u64 {
        let kind = TimerKind::try_from(timer.kind).unwrap_or(TimerKind::Countdown);
        match kind {
            TimerKind::Countdown | TimerKind::CountUp | TimerKind::Unspecified => timer.duration_ms,
            TimerKind::CountToTime => {
                let wall_ms = if self.snapshot.virtual_clock_wall_unix_ms > 0 {
                    self.snapshot.virtual_clock_wall_unix_ms
                } else {
                    1_700_000_000_000
                };
                let current_tod_ms = wall_ms.rem_euclid(86_400_000) as u64;
                let target_tod_ms = u64::from(timer.target_time_of_day_seconds) * 1000;
                if target_tod_ms >= current_tod_ms {
                    target_tod_ms - current_tod_ms
                } else {
                    86_400_000 - (current_tod_ms - target_tod_ms)
                }
            }
        }
    }

    fn start_timer_internal(
        &mut self,
        library: &Library,
        timer_id: &Id,
    ) -> Result<(), CommandBusError> {
        validate_id(timer_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let timer = library
            .get::<Timer>(timer_id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("timer not found".into()))?;
        let now_mono_ns = self.snapshot.virtual_clock_mono_ns;
        let duration_ms = self.compute_timer_duration_ms(&timer);
        let kind = TimerKind::try_from(timer.kind).unwrap_or(TimerKind::Countdown);
        let auto = self.automation_mut();
        if let Some(existing) = auto
            .timers
            .iter_mut()
            .find(|t| t.timer_id.as_ref() == Some(timer_id))
        {
            if !existing.running {
                existing.started_at_mono_ns = now_mono_ns;
                existing.running = true;
            }
        } else {
            let initial_rem = if kind == TimerKind::CountUp {
                0
            } else {
                duration_ms as i64
            };
            auto.timers.push(TimerStatus {
                timer_id: Some(timer_id.clone()),
                name: timer.name.clone(),
                kind: timer.kind,
                overrun_mode: timer.overrun_mode,
                running: true,
                overrun: false,
                remaining_ms: initial_rem,
                elapsed_ms: 0,
                duration_ms,
                formatted_time: format_timer_ms(initial_rem, timer.show_hours),
                progress: 0.0,
                started_at_mono_ns: now_mono_ns,
                accumulated_elapsed_ns: 0,
                overrun_macro_id: timer.overrun_macro_id.clone(),
                overrun_macro_fired: false,
            });
        }
        Ok(())
    }

    pub fn start_timer(
        &mut self,
        library: &Library,
        timer_id: &Id,
    ) -> Result<LiveStatus, CommandBusError> {
        self.start_timer_internal(library, timer_id)?;
        self.sync_status_flags();
        self.save_machine();
        Ok(self.status())
    }

    fn pause_timer_internal(&mut self, timer_id: &Id) -> Result<(), CommandBusError> {
        validate_id(timer_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let now_mono_ns = self.snapshot.virtual_clock_mono_ns;
        let auto = self.automation_mut();
        let ts = auto
            .timers
            .iter_mut()
            .find(|t| t.timer_id.as_ref() == Some(timer_id))
            .ok_or_else(|| CommandBusError::NotFound("timer is not active".into()))?;
        if ts.running {
            let delta_ns = now_mono_ns.saturating_sub(ts.started_at_mono_ns);
            ts.accumulated_elapsed_ns = ts.accumulated_elapsed_ns.saturating_add(delta_ns);
            ts.running = false;
        }
        Ok(())
    }

    pub fn pause_timer(&mut self, timer_id: &Id) -> Result<LiveStatus, CommandBusError> {
        self.pause_timer_internal(timer_id)?;
        self.sync_status_flags();
        self.save_machine();
        Ok(self.status())
    }

    fn reset_timer_internal(
        &mut self,
        library: &Library,
        timer_id: &Id,
    ) -> Result<(), CommandBusError> {
        validate_id(timer_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let timer = library
            .get::<Timer>(timer_id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("timer not found".into()))?;
        let now_mono_ns = self.snapshot.virtual_clock_mono_ns;
        let duration_ms = self.compute_timer_duration_ms(&timer);
        let kind = TimerKind::try_from(timer.kind).unwrap_or(TimerKind::Countdown);
        let initial_rem = if kind == TimerKind::CountUp {
            0
        } else {
            duration_ms as i64
        };
        let status_entry = TimerStatus {
            timer_id: Some(timer_id.clone()),
            name: timer.name.clone(),
            kind: timer.kind,
            overrun_mode: timer.overrun_mode,
            running: false,
            overrun: false,
            remaining_ms: initial_rem,
            elapsed_ms: 0,
            duration_ms,
            formatted_time: format_timer_ms(initial_rem, timer.show_hours),
            progress: 0.0,
            started_at_mono_ns: now_mono_ns,
            accumulated_elapsed_ns: 0,
            overrun_macro_id: timer.overrun_macro_id.clone(),
            overrun_macro_fired: false,
        };
        let auto = self.automation_mut();
        if let Some(existing) = auto
            .timers
            .iter_mut()
            .find(|t| t.timer_id.as_ref() == Some(timer_id))
        {
            *existing = status_entry;
        } else {
            auto.timers.push(status_entry);
        }
        Ok(())
    }

    pub fn reset_timer(
        &mut self,
        library: &Library,
        timer_id: &Id,
    ) -> Result<LiveStatus, CommandBusError> {
        self.reset_timer_internal(library, timer_id)?;
        self.sync_status_flags();
        self.save_machine();
        Ok(self.status())
    }

    pub fn advance_live_clock(
        &mut self,
        library: &Library,
        cmd: &AdvanceLiveClock,
    ) -> Result<LiveStatus, CommandBusError> {
        let delta_ns = cmd.delta_ms.saturating_mul(1_000_000);
        self.snapshot.virtual_clock_mono_ns =
            self.snapshot.virtual_clock_mono_ns.saturating_add(delta_ns);
        if cmd.wall_unix_ms > 0 {
            self.snapshot.virtual_clock_wall_unix_ms = cmd.wall_unix_ms;
        } else if self.snapshot.virtual_clock_wall_unix_ms > 0 {
            self.snapshot.virtual_clock_wall_unix_ms = self
                .snapshot
                .virtual_clock_wall_unix_ms
                .saturating_add(cmd.delta_ms as i64);
        }
        let now_mono_ns = self.snapshot.virtual_clock_mono_ns;

        let mut macros_to_fire: Vec<Id> = Vec::new();
        let mut message_expired = false;

        {
            let auto = self.automation_mut();
            for ts in &mut auto.timers {
                if !ts.running {
                    continue;
                }
                let total_elapsed_ns = ts
                    .accumulated_elapsed_ns
                    .saturating_add(now_mono_ns.saturating_sub(ts.started_at_mono_ns));
                let total_elapsed_ms = total_elapsed_ns / 1_000_000;
                ts.elapsed_ms = total_elapsed_ms;
                let kind = TimerKind::try_from(ts.kind).unwrap_or(TimerKind::Countdown);
                match kind {
                    TimerKind::CountUp => {
                        ts.remaining_ms = total_elapsed_ms as i64;
                        ts.overrun = ts.duration_ms > 0 && total_elapsed_ms > ts.duration_ms;
                        ts.progress = if ts.duration_ms > 0 {
                            (total_elapsed_ms as f32 / ts.duration_ms as f32).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        ts.formatted_time = format_timer_ms(ts.remaining_ms, false);
                    }
                    TimerKind::Countdown | TimerKind::CountToTime | TimerKind::Unspecified => {
                        let raw_rem = (ts.duration_ms as i64) - (total_elapsed_ms as i64);
                        ts.progress = if ts.duration_ms > 0 {
                            (total_elapsed_ms as f32 / ts.duration_ms as f32).clamp(0.0, 1.0)
                        } else {
                            1.0
                        };
                        if raw_rem > 0 {
                            ts.overrun = false;
                            ts.remaining_ms = raw_rem;
                        } else {
                            let mode = TimerOverrunMode::try_from(ts.overrun_mode)
                                .unwrap_or(TimerOverrunMode::StopAtZero);
                            match mode {
                                TimerOverrunMode::StopAtZero | TimerOverrunMode::Unspecified => {
                                    ts.remaining_ms = 0;
                                    ts.running = false;
                                    ts.overrun = true;
                                }
                                TimerOverrunMode::CountUpNegative => {
                                    ts.remaining_ms = raw_rem;
                                    ts.overrun = true;
                                }
                                TimerOverrunMode::TriggerMacro => {
                                    ts.remaining_ms = 0;
                                    ts.running = false;
                                    ts.overrun = true;
                                    if !ts.overrun_macro_fired {
                                        ts.overrun_macro_fired = true;
                                        if let Some(ref mid) = ts.overrun_macro_id
                                            && !mid.value.is_empty()
                                        {
                                            macros_to_fire.push(mid.clone());
                                        }
                                    }
                                }
                            }
                        }
                        ts.formatted_time = format_timer_ms(ts.remaining_ms, false);
                    }
                }
            }

            if auto.active_message_remaining_ms > 0 {
                if cmd.delta_ms >= auto.active_message_remaining_ms {
                    auto.active_message_remaining_ms = 0;
                    message_expired = true;
                } else {
                    auto.active_message_remaining_ms -= cmd.delta_ms;
                }
            }
        }

        if message_expired {
            self.hide_message_internal();
        }

        for mid in &macros_to_fire {
            let _ = self.execute_macro_actions_internal(library, mid, 0);
        }

        if message_expired || !macros_to_fire.is_empty() {
            let any_active = self
                .snapshot
                .status
                .as_ref()
                .is_some_and(|s| s.layers.iter().any(|l| l.active));
            let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
            self.rebuild_scene_from_status(!any_active && !show_logo, show_logo);
        } else {
            self.sync_status_flags();
            self.save_machine();
        }
        Ok(self.status())
    }

    fn apply_clear_group_internal(&mut self, group: &ClearGroup) {
        for &lk in &group.layers_to_clear {
            if let Ok(kind) = LayerKind::try_from(lk)
                && kind != LayerKind::Unspecified
            {
                self.set_layer_slot(kind, false, String::new(), String::new(), None);
                if kind == LayerKind::Props {
                    let auto = self.automation_mut();
                    auto.active_prop_id = None;
                    auto.active_prop_name.clear();
                } else if kind == LayerKind::Messages {
                    let auto = self.automation_mut();
                    auto.active_message_id = None;
                    auto.active_message_text.clear();
                    auto.active_message_remaining_ms = 0;
                }
            }
        }
        if group.stop_audio
            && let Some(status) = self.snapshot.status.as_mut()
            && let Some(audio) = status.audio.as_mut()
        {
            audio.playing = false;
        }
        if group.stop_timers {
            let auto = self.automation_mut();
            for ts in &mut auto.timers {
                ts.running = false;
            }
        }
        if group.clear_show_logo
            && let Some(status) = self.snapshot.status.as_mut()
        {
            status.show_logo = false;
        }
    }

    pub fn trigger_clear_group(
        &mut self,
        library: &Library,
        clear_group_id: &Id,
    ) -> Result<LiveStatus, CommandBusError> {
        validate_id(clear_group_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let group = library
            .get::<ClearGroup>(clear_group_id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("clear group not found".into()))?;
        self.push_undo();
        self.apply_clear_group_internal(&group);
        let any_active = self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.layers.iter().any(|l| l.active));
        let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
        self.rebuild_scene_from_status(!any_active && !show_logo, show_logo);
        Ok(self.status())
    }

    fn execute_macro_actions_internal(
        &mut self,
        library: &Library,
        macro_id: &Id,
        depth: u8,
    ) -> Result<(), CommandBusError> {
        if depth > 4 {
            return Err(CommandBusError::Invalid(
                "macro recursion depth exceeded".into(),
            ));
        }
        validate_id(macro_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let mac = library
            .get::<Macro>(macro_id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("macro not found".into()))?;
        for action in &mac.actions {
            self.execute_single_macro_action(library, action)?;
        }
        Ok(())
    }

    fn execute_single_macro_action(
        &mut self,
        library: &Library,
        action: &MacroAction,
    ) -> Result<(), CommandBusError> {
        let kind = MacroActionKind::try_from(action.kind).unwrap_or(MacroActionKind::Unspecified);
        match kind {
            MacroActionKind::SetLook => {
                let target = action.target_id.clone().unwrap_or_default();
                self.apply_look_internal(library, &target)?;
            }
            MacroActionKind::ShowProp => {
                let target = action
                    .target_id
                    .as_ref()
                    .ok_or_else(|| CommandBusError::Invalid("missing prop target_id".into()))?;
                self.show_prop_internal(library, target)?;
            }
            MacroActionKind::HideProp => {
                self.hide_prop_internal();
            }
            MacroActionKind::ShowMessage => {
                let cmd = ShowMessage {
                    message_id: action.target_id.clone(),
                    token_values: HashMap::new(),
                    custom_text: action.text_param.clone(),
                    duration_seconds: 0,
                    stage_only: action.bool_param,
                };
                self.show_message_internal(library, &cmd)?;
            }
            MacroActionKind::HideMessage => {
                self.hide_message_internal();
            }
            MacroActionKind::StartTimer => {
                let target = action
                    .target_id
                    .as_ref()
                    .ok_or_else(|| CommandBusError::Invalid("missing timer target_id".into()))?;
                self.start_timer_internal(library, target)?;
            }
            MacroActionKind::PauseTimer => {
                let target = action
                    .target_id
                    .as_ref()
                    .ok_or_else(|| CommandBusError::Invalid("missing timer target_id".into()))?;
                self.pause_timer_internal(target)?;
            }
            MacroActionKind::ResetTimer => {
                let target = action
                    .target_id
                    .as_ref()
                    .ok_or_else(|| CommandBusError::Invalid("missing timer target_id".into()))?;
                self.reset_timer_internal(library, target)?;
            }
            MacroActionKind::ClearLayer => {
                let lk = LayerKind::try_from(action.layer_kind).unwrap_or(LayerKind::Unspecified);
                if lk != LayerKind::Unspecified {
                    self.set_layer_slot(lk, false, String::new(), String::new(), None);
                }
            }
            MacroActionKind::TriggerClearGroup => {
                let target = action.target_id.as_ref().ok_or_else(|| {
                    CommandBusError::Invalid("missing clear_group target_id".into())
                })?;
                validate_id(target).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
                let group = library
                    .get::<ClearGroup>(target)
                    .map_err(CommandBusError::Internal)?
                    .ok_or_else(|| CommandBusError::NotFound("clear group not found".into()))?;
                self.apply_clear_group_internal(&group);
            }
            MacroActionKind::ClearAll => {
                if let Some(status) = self.snapshot.status.as_mut() {
                    for l in &mut status.layers {
                        l.active = false;
                        l.title.clear();
                        l.subtitle.clear();
                        l.spec = None;
                    }
                    if let Some(audio) = status.audio.as_mut() {
                        audio.playing = false;
                    }
                    status.show_logo = false;
                    status.cleared = true;
                }
            }
            MacroActionKind::ShowLogo => {
                if let Some(status) = self.snapshot.status.as_mut() {
                    status.show_logo = action.bool_param;
                }
            }
            MacroActionKind::PlayAudio => {
                let target = action
                    .target_id
                    .as_ref()
                    .ok_or_else(|| CommandBusError::Invalid("missing audio target_id".into()))?;
                validate_id(target).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
                let media = library
                    .get::<MediaItem>(target)
                    .map_err(CommandBusError::Internal)?
                    .ok_or_else(|| CommandBusError::NotFound("audio media not found".into()))?;
                let mkind = MediaKind::try_from(media.kind).unwrap_or(MediaKind::Audio);
                let ext = media_extension_from_item(&media.extension, mkind);
                let vol = if action.float_param.is_finite() && action.float_param > 0.0 {
                    action.float_param.clamp(0.0, 2.0)
                } else {
                    0.8
                };
                if let Some(status) = self.snapshot.status.as_mut() {
                    status.audio = Some(AudioLayerStatus {
                        playing: true,
                        media_id: Some(target.clone()),
                        title: media.name,
                        content_hash: media.content_hash,
                        media_extension: ext,
                        volume: vol,
                        loop_audio: action.bool_param,
                    });
                }
            }
            MacroActionKind::StopAudio => {
                if let Some(status) = self.snapshot.status.as_mut()
                    && let Some(audio) = status.audio.as_mut()
                {
                    audio.playing = false;
                }
            }
            MacroActionKind::SetStageLayout => {
                let target = action.target_id.clone().unwrap_or_default();
                self.set_stage_layout_internal(library, &target)?;
            }
            MacroActionKind::Unspecified => {
                return Err(CommandBusError::Invalid(
                    "macro action kind must be specified".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn run_macro(
        &mut self,
        library: &Library,
        macro_id: &Id,
    ) -> Result<LiveStatus, CommandBusError> {
        self.push_undo();
        self.execute_macro_actions_internal(library, macro_id, 0)?;
        let any_active = self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.layers.iter().any(|l| l.active));
        let show_logo = self.snapshot.status.as_ref().is_some_and(|s| s.show_logo);
        self.rebuild_scene_from_status(!any_active && !show_logo, show_logo);
        Ok(self.status())
    }

    fn set_stage_layout_internal(
        &mut self,
        library: &Library,
        stage_layout_id: &Id,
    ) -> Result<(), CommandBusError> {
        if stage_layout_id.value.is_empty() {
            self.snapshot.active_stage_layout = None;
            let auto = self.automation_mut();
            auto.active_stage_layout_id = None;
            auto.active_stage_layout_name.clear();
            return Ok(());
        }
        validate_id(stage_layout_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let layout = library
            .get::<StageLayout>(stage_layout_id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("stage layout not found".into()))?;
        let lid = layout.id.clone();
        let lname = layout.name.clone();
        self.snapshot.active_stage_layout = Some(layout);
        let auto = self.automation_mut();
        auto.active_stage_layout_id = lid;
        auto.active_stage_layout_name = lname;
        Ok(())
    }

    pub fn set_stage_layout(
        &mut self,
        library: &Library,
        stage_layout_id: &Id,
    ) -> Result<LiveStatus, CommandBusError> {
        self.set_stage_layout_internal(library, stage_layout_id)?;
        self.sync_status_flags();
        self.save_machine();
        Ok(self.status())
    }

    pub fn live_redo(&mut self) -> Result<LiveStatus, CommandBusError> {
        let Some(next_entry) = self.snapshot.redo_stack.pop() else {
            return Err(CommandBusError::NothingToUndo);
        };
        let cur_status = self.status();
        self.snapshot.undo_stack.push(LiveSnapshotEntry {
            scene: Some(self.scene.clone()),
            status: Some(cur_status),
        });
        let next_revision = self.scene.revision + 1;
        if let Some(mut restored_scene) = next_entry.scene {
            restored_scene.revision = next_revision;
            self.scene = restored_scene;
        } else {
            self.scene.revision = next_revision;
        }
        if let Some(restored_status) = next_entry.status {
            self.snapshot.status = Some(restored_status);
        }
        self.sync_status_flags();
        self.save();
        Ok(self.status())
    }

    fn save_machine(&self) {
        if let Err(error) =
            selah_runtime::paths::write_atomic(&self.machine_path, &self.snapshot.encode_to_vec())
        {
            tracing::error!(%error, "could not save the live-machine snapshot");
        }
    }

    fn save(&self) {
        if let Err(error) =
            selah_runtime::paths::write_atomic(&self.path, &self.scene.encode_to_vec())
        {
            tracing::error!(%error, "could not save the live-state snapshot");
        }
        self.save_machine();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use selah_proto::v1::{LayerKind, LayerSpec, OutputScene, SlideLayerSpec, layer_spec};

    #[test]
    fn snapshot_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut live = LiveState::open(dir.path());
        assert!(!is_live(live.scene()));
        live.change(|scene| {
            scene.cleared = false;
            scene.test_pattern = TestPattern::SmpteBars as i32;
        });
        live.set_output_configs(vec![OutputConfig {
            output_index: 1,
            name: "Wide LED".into(),
            role: OutputRole::Main as i32,
            resolution: Some(Resolution {
                width: 3840,
                height: 1080,
            }),
            enabled: true,
            ..Default::default()
        }]);
        let reopened = LiveState::open(dir.path());
        assert_eq!(reopened.scene().revision, 1);
        assert!(is_live(reopened.scene()));
        assert!(reopened.has_explicit_output_configs());
        assert_eq!(reopened.output_configs()[0].name, "Wide LED");
        assert!(reopened.status().can_live_undo);
    }

    #[test]
    fn corrupt_snapshot_starts_cleared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("live_state.bin"), [0xff, 0xff, 0xff]).unwrap();
        std::fs::write(dir.path().join("live_machine.bin"), [0xff, 0xff, 0xff]).unwrap();
        assert!(LiveState::open(dir.path()).scene().cleared);
    }

    #[test]
    fn layer_scene_liveness_tracks_enabled_layers() {
        let mut scene = Scene {
            cleared: false,
            outputs: vec![OutputScene {
                output_index: 0,
                layers: vec![LayerSpec {
                    kind: LayerKind::Slide as i32,
                    enabled: true,
                    content: Some(layer_spec::Content::Slide(SlideLayerSpec {
                        text: "Blessed Assurance".into(),
                        ..Default::default()
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(is_live(&scene));
        scene.outputs[0].show_logo = true;
        assert!(!is_live(&scene));
    }
}
