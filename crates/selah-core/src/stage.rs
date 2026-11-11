//! Stage display (M4.6): what singers, musicians and speakers see on Stage-role outputs.
//!
//! Core composes it from the live state and the active [`StageLayout`] as two ordinary layers the
//! engine already knows how to draw:
//! - a Slide layer: the words on screen now (through the stage template, so chords sit over the
//!   right syllables), the header with song, key and sections, the next slide, notes and the booth
//!   message;
//! - a Props layer holding only the clock and the running timer, so the engine re-renders that
//!   small layer each second and leaves the words cached.
//!
//! The words come from `content/i18n/en/stage.ftl` (ADR-010). **Message the booth** text only ever
//! reaches stage outputs and the producer's UI (SAFE-01).
use std::collections::HashMap;
use std::sync::OnceLock;

use anyhow::{Result, ensure};
use selah_proto::v1::{
    LayerKind, LayerSpec, LiveAutomationState, LiveStatus, PropsLayerSpec, Rect, SlideLayerSpec,
    StageElement, StageElementKind, StageLayout, TimerKind, TimerStatus, layer_spec,
};
use selah_render::slide_elements::{
    ShapeKind, SlideElement, SlideElementKind, SlideElementsDocument,
};
use selah_render::template::{SafeArea, TemplateStyle, TextAlignH, TextAlignV};

/// Layouts are 1920 × 1080 reference pixels; rects are fractions of the output.
const REF_W: f32 = 1920.0;
const REF_H: f32 = 1080.0;
/// At most this many section chips in the header (the engine allows 64 elements in all).
const MAX_CHIPS: usize = 10;
/// Longest layout name we accept (the editor caps it too).
pub const MAX_NAME_CHARS: usize = 80;

const WHITE: [u8; 4] = [245, 245, 247, 255];
const GREY: [u8; 4] = [161, 161, 166, 255];
const BAR: [u8; 4] = [28, 28, 30, 255];
const BAND: [u8; 4] = [17, 17, 19, 255];
const CHIP: [u8; 4] = [44, 44, 46, 255];
const CHIP_ON: [u8; 4] = [36, 145, 255, 255];
const GOLD: [u8; 4] = [255, 190, 46, 255];
const INK: [u8; 4] = [27, 27, 27, 255];
const GREEN: [u8; 4] = [33, 200, 52, 255];
const RED: [u8; 4] = [255, 99, 99, 255];
const CREAM: [u8; 4] = [250, 243, 209, 255];

/// Every element kind a stage layout can place, in drawing order.
pub const KINDS: [StageElementKind; 8] = [
    StageElementKind::HeaderBar,
    StageElementKind::CurrentSlide,
    StageElementKind::NextSlide,
    StageElementKind::SpeakerNotes,
    StageElementKind::Timer,
    StageElementKind::Clock,
    StageElementKind::BoothMessage,
    StageElementKind::ArrangementStrip,
];

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Where each element sits when the layout doesn't say (prototype screen 06).
pub fn default_rect(kind: StageElementKind) -> Rect {
    match kind {
        StageElementKind::HeaderBar | StageElementKind::ArrangementStrip => {
            rect(0.0, 0.0, 1.0, 72.0 / REF_H)
        }
        StageElementKind::CurrentSlide => rect(0.0, 72.0 / REF_H, 1.0, 728.0 / REF_H),
        StageElementKind::NextSlide => rect(0.0, 800.0 / REF_H, 1490.0 / REF_W, 220.0 / REF_H),
        StageElementKind::SpeakerNotes => {
            rect(1100.0 / REF_W, 100.0 / REF_H, 790.0 / REF_W, 680.0 / REF_H)
        }
        StageElementKind::Timer => {
            rect(1490.0 / REF_W, 800.0 / REF_H, 430.0 / REF_W, 220.0 / REF_H)
        }
        StageElementKind::Clock => rect(1500.0 / REF_W, 0.0, 400.0 / REF_W, 72.0 / REF_H),
        StageElementKind::BoothMessage => rect(0.0, 1020.0 / REF_H, 1.0, 60.0 / REF_H),
        StageElementKind::Unspecified => rect(0.0, 0.0, 1.0, 1.0),
    }
}

/// The built-in layout: prototype screen 06 (everything but notes).
pub fn default_layout() -> StageLayout {
    StageLayout {
        id: None,
        name: labels().get("stage-default-layout-name", &[]),
        show_chords: true,
        show_key_badge: true,
        show_arrangement_strip: true,
        show_clock: true,
        show_next_slide: true,
        show_timer: true,
        show_speaker_notes: false,
        show_booth_message: true,
        elements: Vec::new(),
    }
}

/// SEC-03: a stage layout is outside input (it can arrive in an imported library). Every rect must
/// be finite and on the screen, every scale sensible, and the name short.
pub fn validate_layout(layout: &StageLayout) -> Result<()> {
    ensure!(
        layout.name.chars().count() <= MAX_NAME_CHARS,
        "stage layout name is longer than {MAX_NAME_CHARS} characters"
    );
    ensure!(
        layout.elements.len() <= 32,
        "too many stage layout elements"
    );
    for element in &layout.elements {
        ensure!(
            StageElementKind::try_from(element.kind).is_ok() && element.kind != 0,
            "invalid stage element kind"
        );
        if let Some(r) = &element.rect {
            ensure!(
                [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite()),
                "stage element rect must be finite"
            );
            ensure!(
                (0.0..=1.0).contains(&r.x)
                    && (0.0..=1.0).contains(&r.y)
                    && (0.0..=1.0).contains(&r.width)
                    && (0.0..=1.0).contains(&r.height)
                    && r.x + r.width <= 1.0 + 1e-4
                    && r.y + r.height <= 1.0 + 1e-4,
                "stage element rect must be on the screen"
            );
        }
        ensure!(
            element.font_scale == 0.0 || (0.5..=3.0).contains(&element.font_scale),
            "stage element text size must be 50% to 300%"
        );
    }
    Ok(())
}

/// One element as it will be drawn: where, how big, and whether at all.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub kind: StageElementKind,
    pub rect: Rect,
    pub scale: f32,
    pub visible: bool,
    pub show_chords: bool,
}

fn layout_flag(layout: &StageLayout, kind: StageElementKind) -> bool {
    match kind {
        StageElementKind::HeaderBar | StageElementKind::CurrentSlide => true,
        StageElementKind::NextSlide => layout.show_next_slide,
        StageElementKind::SpeakerNotes => layout.show_speaker_notes,
        StageElementKind::Timer => layout.show_timer,
        StageElementKind::Clock => layout.show_clock,
        StageElementKind::BoothMessage => layout.show_booth_message,
        StageElementKind::ArrangementStrip => layout.show_arrangement_strip,
        StageElementKind::Unspecified => false,
    }
}

/// Resolve the layout into one [`Placed`] per kind: the layout's own element when it has one,
/// otherwise the default position. Tick boxes on the layout switch whole kinds off.
pub fn place(layout: &StageLayout) -> Vec<Placed> {
    KINDS
        .iter()
        .map(|&kind| {
            let own: Option<&StageElement> = layout.elements.iter().find(|e| e.kind == kind as i32);
            let usable = |r: &Rect| r.width > 0.0 && r.height > 0.0;
            let rect = own
                .and_then(|e| e.rect)
                .filter(usable)
                .unwrap_or_else(|| default_rect(kind));
            let scale = own
                .map(|e| e.font_scale)
                .filter(|s| (0.5..=3.0).contains(s))
                .unwrap_or(1.0);
            Placed {
                kind,
                rect,
                scale,
                visible: layout_flag(layout, kind) && own.is_none_or(|e| e.visible),
                show_chords: layout.show_chords && own.is_none_or(|e| e.show_chords),
            }
        })
        .collect()
}

/// The stage words from `stage.ftl`, compiled in so core needs no files to draw them.
pub struct Labels(HashMap<String, String>);

impl Labels {
    pub fn parse(source: &str) -> Self {
        let mut map = HashMap::new();
        for line in source.lines() {
            let line = line.trim_end();
            if line.starts_with('#') || line.starts_with(' ') {
                continue;
            }
            if let Some((id, value)) = line.split_once(" = ") {
                map.insert(id.trim().to_owned(), value.trim().to_owned());
            }
        }
        Self(map)
    }

    /// The message with `{ $name }` placeables filled in. A missing id shows the id, which the
    /// tests would catch.
    pub fn get(&self, id: &str, args: &[(&str, &str)]) -> String {
        let mut text = self.0.get(id).cloned().unwrap_or_else(|| id.to_owned());
        for (name, value) in args {
            text = text.replace(&format!("{{ ${name} }}"), value);
        }
        text
    }
}

pub fn labels() -> &'static Labels {
    static LABELS: OnceLock<Labels> = OnceLock::new();
    LABELS.get_or_init(|| Labels::parse(include_str!("../../../content/i18n/en/stage.ftl")))
}

/// What the stage composer reads: the live state, the clock and the layout.
pub struct StageInputs<'a> {
    pub status: &'a LiveStatus,
    pub layout: Option<&'a StageLayout>,
    /// Wall-clock time the timer targets are measured from (the live virtual clock in tests).
    pub wall_now_unix_ms: i64,
}

fn px_x(v: f32) -> f32 {
    v / REF_W
}

fn px_y(v: f32) -> f32 {
    v / REF_H
}

fn shape(id: &str, r: Rect, fill: [u8; 4], radius: f32, kind: ShapeKind) -> SlideElement {
    SlideElement {
        id: id.into(),
        kind: SlideElementKind::Shape,
        shape_kind: kind,
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
        fill_color: fill,
        corner_radius: radius,
        ..SlideElement::default()
    }
}

#[allow(clippy::too_many_arguments)]
fn text(
    id: &str,
    r: Rect,
    body: &str,
    size: f32,
    weight: u16,
    color: [u8; 4],
    align_h: TextAlignH,
    align_v: TextAlignV,
) -> SlideElement {
    SlideElement {
        id: id.into(),
        kind: SlideElementKind::Text,
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
        text: body.into(),
        font_family: "Inter".into(),
        font_size: size.clamp(8.0, 400.0),
        weight,
        color,
        align_h,
        align_v,
        line_height: 1.2,
        padding: 0.0,
        ..SlideElement::default()
    }
}

/// Rough width of `s` in reference pixels at `size` (Inter bold averages about 0.6 em). Core can't
/// measure glyphs, and the header only needs chips that don't overlap.
fn estimate_width(s: &str, size: f32) -> f32 {
    s.chars().count() as f32 * size * 0.6
}

/// "Verse 1" → "V1", "Pre-Chorus 2" → "P2", short labels stay as they are.
pub fn chip_label(label: &str) -> String {
    let label = label.trim();
    let mut words = label.split_whitespace();
    if let (Some(first), Some(number), None) = (words.next(), words.next(), words.next())
        && number.chars().all(|c| c.is_ascii_digit())
        && let Some(initial) = first.chars().next()
    {
        return format!("{}{number}", initial.to_uppercase());
    }
    if label.chars().count() <= 6 {
        return label.to_owned();
    }
    label.chars().take(5).collect()
}

/// Sections of the current item, merging slides that share a label: `(chip, holds current)`.
pub fn arrangement(status: &LiveStatus) -> Vec<(String, bool)> {
    let mut chips: Vec<(String, bool)> = Vec::new();
    let mut last_label: Option<&str> = None;
    for slide in &status.current_item_slides {
        let current = status.has_active_item && slide.index == status.current_slide_index;
        if last_label == Some(slide.label.as_str())
            && let Some(last) = chips.last_mut()
        {
            last.1 |= current;
            continue;
        }
        last_label = Some(slide.label.as_str());
        chips.push((chip_label(&slide.label), current));
    }
    if chips.len() > MAX_CHIPS {
        let at = chips.iter().position(|c| c.1).unwrap_or(0);
        let start = at
            .saturating_sub(MAX_CHIPS / 2)
            .min(chips.len() - MAX_CHIPS);
        chips = chips[start..start + MAX_CHIPS].to_vec();
    }
    chips
}

/// The timer the stage shows: the first running one, else the first that has run over.
pub fn stage_timer(automation: Option<&LiveAutomationState>) -> Option<&TimerStatus> {
    let timers = &automation?.timers;
    timers
        .iter()
        .find(|t| t.running)
        .or_else(|| timers.iter().find(|t| t.overrun))
}

/// The layers for one Stage-role output.
pub fn compose(inputs: &StageInputs<'_>) -> Vec<LayerSpec> {
    let fallback = default_layout();
    let layout = inputs.layout.unwrap_or(&fallback);
    let placed = place(layout);
    let get = |kind: StageElementKind| {
        placed
            .iter()
            .find(|p| p.kind == kind)
            .cloned()
            .expect("every kind is placed")
    };
    let status = inputs.status;
    let automation = status.automation.first();
    let words = labels();
    let mut slide_elements: Vec<SlideElement> = Vec::new();
    let mut live_elements: Vec<SlideElement> = Vec::new();

    // Header: song title, key badge and section chips.
    let header = get(StageElementKind::HeaderBar);
    if header.visible {
        let r = header.rect;
        slide_elements.push(shape("header", r, BAR, 0.0, ShapeKind::Rect));
        let s = header.scale;
        let mut cursor = r.x + px_x(36.0);
        let title = if status.has_active_item {
            status.current_item_title.clone()
        } else {
            status.service_name.clone()
        };
        if !title.is_empty() {
            let width = px_x(estimate_width(&title, 24.0 * s)).min(0.4);
            slide_elements.push(text(
                "title",
                rect(cursor, r.y, width + px_x(8.0), r.height),
                &title,
                24.0 * s,
                800,
                WHITE,
                TextAlignH::Left,
                TextAlignV::Middle,
            ));
            cursor += width + px_x(22.0);
        }
        if layout.show_key_badge && status.has_active_item && !status.song_key.is_empty() {
            let width = px_x(estimate_width(&status.song_key, 18.0 * s) + 20.0);
            let chip = rect(cursor, r.y + r.height * 0.28, width, r.height * 0.44);
            slide_elements.push(shape("key-badge", chip, GOLD, 4.0, ShapeKind::RoundedRect));
            slide_elements.push(text(
                "key",
                chip,
                &status.song_key,
                18.0 * s,
                800,
                INK,
                TextAlignH::Center,
                TextAlignV::Middle,
            ));
            cursor += width + px_x(22.0);
        }
        let strip = get(StageElementKind::ArrangementStrip);
        if strip.visible && status.has_active_item {
            let clock_left = get(StageElementKind::Clock).rect.x;
            for (n, (label, on)) in arrangement(status).into_iter().enumerate() {
                let width = px_x(estimate_width(&label, 15.0 * s) + 22.0);
                if cursor + width > clock_left {
                    break;
                }
                let chip = rect(cursor, r.y + r.height * 0.3, width, r.height * 0.4);
                let (fill, ink) = if on {
                    (CHIP_ON, [255, 255, 255, 255])
                } else {
                    (CHIP, GREY)
                };
                slide_elements.push(shape(
                    &format!("chip-{n}"),
                    chip,
                    fill,
                    8.0,
                    ShapeKind::RoundedRect,
                ));
                slide_elements.push(text(
                    &format!("chip-text-{n}"),
                    chip,
                    &label,
                    15.0 * s,
                    700,
                    ink,
                    TextAlignH::Center,
                    TextAlignV::Middle,
                ));
                cursor += width + px_x(6.0);
            }
        }
    }

    // Next slide (a band across the bottom, as in prototype 06).
    let next = get(StageElementKind::NextSlide);
    if next.visible {
        let r = next.rect;
        slide_elements.push(shape(
            "next-band",
            rect(0.0, r.y, 1.0, r.height),
            BAND,
            0.0,
            ShapeKind::Rect,
        ));
        let s = next.scale;
        let other_item = status.next_item_index != status.current_item_index;
        let (label, body) = if !status.has_next {
            (
                words.get("stage-next", &[]),
                words.get("stage-end-of-service", &[]),
            )
        } else {
            let section = if other_item || status.next_slide_label.is_empty() {
                status.next_item_title.as_str()
            } else {
                status.next_slide_label.as_str()
            };
            let label = if section.is_empty() {
                words.get("stage-next", &[])
            } else {
                words.get("stage-next-section", &[("section", section)])
            };
            (label, status.next_slide_text.clone())
        };
        let inner = rect(
            r.x + px_x(50.0),
            r.y + px_y(20.0),
            r.width - px_x(80.0),
            r.height - px_y(40.0),
        );
        slide_elements.push(text(
            "next-label",
            rect(inner.x, inner.y, inner.width, px_y(26.0 * s)),
            &label.to_uppercase(),
            15.0 * s,
            800,
            GREEN,
            TextAlignH::Left,
            TextAlignV::Top,
        ));
        slide_elements.push(text(
            "next-text",
            rect(
                inner.x,
                inner.y + px_y(30.0 * s),
                inner.width,
                (inner.height - px_y(30.0 * s)).max(px_y(40.0)),
            ),
            &body,
            36.0 * s,
            650,
            GREY,
            TextAlignH::Left,
            TextAlignV::Top,
        ));
    }

    // Slide notes (for speakers).
    let notes = get(StageElementKind::SpeakerNotes);
    if notes.visible && !status.current_notes.trim().is_empty() {
        let r = notes.rect;
        let s = notes.scale;
        slide_elements.push(shape("notes-box", r, BAR, 18.0, ShapeKind::RoundedRect));
        slide_elements.push(text(
            "notes-label",
            rect(
                r.x + px_x(22.0),
                r.y + px_y(16.0),
                r.width - px_x(44.0),
                px_y(24.0 * s),
            ),
            &words.get("stage-notes", &[]).to_uppercase(),
            15.0 * s,
            800,
            GOLD,
            TextAlignH::Left,
            TextAlignV::Top,
        ));
        slide_elements.push(text(
            "notes-text",
            rect(
                r.x + px_x(22.0),
                r.y + px_y(48.0 * s),
                r.width - px_x(44.0),
                (r.height - px_y(64.0 * s)).max(px_y(40.0)),
            ),
            &status.current_notes,
            30.0 * s,
            600,
            WHITE,
            TextAlignH::Left,
            TextAlignV::Top,
        ));
    }

    // Booth message (SAFE-01: only stage screens and the producer see it).
    let booth = get(StageElementKind::BoothMessage);
    let message = automation
        .map(|a| a.booth_message.trim())
        .unwrap_or_default();
    if booth.visible && !message.is_empty() {
        let r = booth.rect;
        let s = booth.scale;
        slide_elements.push(shape("booth", r, CREAM, 0.0, ShapeKind::Rect));
        slide_elements.push(shape(
            "booth-accent",
            rect(r.x, r.y, px_x(12.0), r.height),
            GOLD,
            0.0,
            ShapeKind::Rect,
        ));
        let dot = px_y(30.0 * s).min(r.height * 0.8);
        let dot_rect = rect(
            r.x + px_x(48.0),
            r.y + (r.height - dot) / 2.0,
            dot * REF_H / REF_W,
            dot,
        );
        slide_elements.push(shape("booth-dot", dot_rect, INK, 0.0, ShapeKind::Ellipse));
        slide_elements.push(text(
            "booth-mark",
            dot_rect,
            "!",
            18.0 * s,
            800,
            CREAM,
            TextAlignH::Center,
            TextAlignV::Middle,
        ));
        let left = dot_rect.x + dot_rect.width + px_x(16.0);
        slide_elements.push(text(
            "booth-text",
            rect(
                left,
                r.y,
                (r.x + r.width - left - px_x(36.0)).max(px_x(40.0)),
                r.height,
            ),
            &words.get("stage-from-booth", &[("message", message)]),
            25.0 * s,
            800,
            INK,
            TextAlignH::Left,
            TextAlignV::Middle,
        ));
    }

    // Clock and timer: the Props layer, which the engine re-renders as time passes.
    let clock = get(StageElementKind::Clock);
    if clock.visible {
        let mut el = text(
            "clock",
            rect(
                clock.rect.x,
                clock.rect.y,
                clock.rect.width - px_x(20.0),
                clock.rect.height,
            ),
            "{clock}",
            32.0 * clock.scale,
            800,
            WHITE,
            TextAlignH::Right,
            TextAlignV::Middle,
        );
        el.clock_24h = false;
        el.clock_show_seconds = true;
        live_elements.push(el);
    }
    let timer_place = get(StageElementKind::Timer);
    if timer_place.visible
        && let Some(timer) = stage_timer(automation)
    {
        let r = timer_place.rect;
        let s = timer_place.scale;
        let card = rect(
            r.x,
            r.y + px_y(20.0),
            r.width - px_x(50.0),
            r.height - px_y(40.0),
        );
        live_elements.push(shape("timer-card", card, BAR, 18.0, ShapeKind::RoundedRect));
        let kind = TimerKind::try_from(timer.kind).unwrap_or(TimerKind::Countdown);
        let label_id = if kind == TimerKind::CountUp {
            "stage-time-elapsed"
        } else {
            "stage-time-left"
        };
        live_elements.push(text(
            "timer-label",
            rect(
                card.x + px_x(22.0),
                card.y + px_y(24.0),
                card.width - px_x(44.0),
                px_y(24.0 * s),
            ),
            &words.get(label_id, &[("name", timer.name.as_str())]),
            15.0 * s,
            700,
            GREY,
            TextAlignH::Left,
            TextAlignV::Top,
        ));
        let counting_down = timer.running && kind != TimerKind::CountUp && !timer.overrun;
        let mut value = text(
            "timer-value",
            rect(
                card.x + px_x(22.0),
                card.y + px_y(52.0 * s),
                card.width - px_x(44.0),
                (card.height - px_y(64.0 * s)).max(px_y(40.0)),
            ),
            if counting_down {
                "{timer}"
            } else {
                timer.formatted_time.as_str()
            },
            66.0 * s,
            800,
            if timer.overrun { RED } else { WHITE },
            TextAlignH::Left,
            TextAlignV::Middle,
        );
        if counting_down {
            value.timer_target_unix_ms = Some(
                inputs
                    .wall_now_unix_ms
                    .saturating_add(timer.remaining_ms.max(0)),
            );
        }
        live_elements.push(value);
    }

    // The words on screen now, with chords over them (the stage template, M4-A7).
    let current = get(StageElementKind::CurrentSlide);
    let mut slide = SlideLayerSpec::default();
    if current.visible && status.has_active_item {
        let r = current.rect;
        let s = current.scale;
        let mut template = TemplateStyle::preset_stage_monitor();
        template.safe_area = SafeArea {
            left: r.x + px_x(50.0),
            top: r.y + px_y(30.0),
            right: (1.0 - r.x - r.width) + px_x(50.0),
            bottom: (1.0 - r.y - r.height) + px_y(30.0),
        };
        template.roles.body.size = 64.0 * s;
        template.roles.body.min_size = (28.0 * s).min(template.roles.body.size);
        template.roles.body.weight = 800;
        template.roles.body.line_height = 1.05;
        template.roles.chords.size = 34.0 * s;
        template.roles.chords.min_size = (18.0 * s).min(template.roles.chords.size);
        template.roles.chords.weight = 800;
        template.roles.chords.font_family = "Inter".into();
        template.roles.chords.color = GOLD;
        template.roles.chords.visible = current.show_chords;
        let template = template.validated();
        slide.text = status.current_slide_text.clone();
        if current.show_chords {
            slide.chords_json = status.current_chords_json.clone();
        }
        slide.style_json = template.to_json().unwrap_or_default();
    }
    if !slide_elements.is_empty() {
        let doc = SlideElementsDocument {
            reference_height: REF_H as u32,
            elements: slide_elements,
            ..SlideElementsDocument::default()
        };
        slide.elements_json = serde_json::to_string(&doc).unwrap_or_default();
    }

    let mut layers = vec![LayerSpec {
        kind: LayerKind::Slide as i32,
        enabled: true,
        content: Some(layer_spec::Content::Slide(slide)),
        ..LayerSpec::default()
    }];
    if !live_elements.is_empty() {
        let doc = SlideElementsDocument {
            reference_height: REF_H as u32,
            elements: live_elements,
            ..SlideElementsDocument::default()
        };
        layers.push(LayerSpec {
            kind: LayerKind::Props as i32,
            enabled: true,
            content: Some(layer_spec::Content::Props(PropsLayerSpec {
                elements_json: serde_json::to_string(&doc).unwrap_or_default(),
                ..PropsLayerSpec::default()
            })),
            ..LayerSpec::default()
        });
    }
    layers
}

#[cfg(test)]
mod tests {
    use super::*;
    use selah_proto::v1::LiveSlide;

    fn slide(index: u32, label: &str) -> LiveSlide {
        LiveSlide {
            index,
            label: label.into(),
            ..LiveSlide::default()
        }
    }

    #[test]
    fn every_stage_word_is_in_the_ftl_file() {
        for id in [
            "stage-next",
            "stage-next-section",
            "stage-end-of-service",
            "stage-from-booth",
            "stage-time-left",
            "stage-time-elapsed",
            "stage-notes",
            "stage-default-layout-name",
        ] {
            assert_ne!(labels().get(id, &[]), id, "{id} is missing from stage.ftl");
        }
        assert_eq!(
            labels().get("stage-from-booth", &[("message", "Mic 2 is out")]),
            "From the booth: Mic 2 is out"
        );
    }

    #[test]
    fn section_chips_are_short_and_merge_repeated_slides() {
        assert_eq!(chip_label("Verse 1"), "V1");
        assert_eq!(chip_label("Chorus"), "Chorus");
        assert_eq!(chip_label("Instrumental"), "Instr");
        let status = LiveStatus {
            has_active_item: true,
            current_slide_index: 2,
            current_item_slides: vec![
                slide(0, "Verse 1"),
                slide(1, "Chorus"),
                slide(2, "Chorus"),
                slide(3, "Verse 2"),
            ],
            ..LiveStatus::default()
        };
        assert_eq!(
            arrangement(&status),
            vec![
                ("V1".to_owned(), false),
                ("Chorus".to_owned(), true),
                ("V2".to_owned(), false)
            ]
        );
    }

    #[test]
    fn layouts_off_the_screen_or_with_silly_sizes_are_refused() {
        // sec: SEC-03
        let mut layout = default_layout();
        assert!(validate_layout(&layout).is_ok());
        layout.elements.push(StageElement {
            kind: StageElementKind::Clock as i32,
            rect: Some(rect(0.9, 0.0, 0.5, 0.1)),
            visible: true,
            ..StageElement::default()
        });
        assert!(validate_layout(&layout).is_err());
        layout.elements[0].rect = Some(rect(f32::NAN, 0.0, 0.1, 0.1));
        assert!(validate_layout(&layout).is_err());
        layout.elements[0].rect = Some(rect(0.8, 0.0, 0.2, 0.1));
        layout.elements[0].font_scale = 12.0;
        assert!(validate_layout(&layout).is_err());
        layout.elements[0].font_scale = 1.5;
        assert!(validate_layout(&layout).is_ok());
        layout.name = "x".repeat(MAX_NAME_CHARS + 1);
        assert!(validate_layout(&layout).is_err());
    }

    #[test]
    fn tick_boxes_switch_kinds_off_and_elements_move_them() {
        let mut layout = default_layout();
        layout.show_clock = false;
        layout.elements.push(StageElement {
            kind: StageElementKind::Timer as i32,
            rect: Some(rect(0.1, 0.1, 0.3, 0.3)),
            visible: true,
            font_scale: 2.0,
            ..StageElement::default()
        });
        let placed = place(&layout);
        let find = |k| placed.iter().find(|p| p.kind == k).unwrap();
        assert!(!find(StageElementKind::Clock).visible);
        assert!(!find(StageElementKind::SpeakerNotes).visible);
        assert_eq!(find(StageElementKind::Timer).rect, rect(0.1, 0.1, 0.3, 0.3));
        assert_eq!(find(StageElementKind::Timer).scale, 2.0);
        assert_eq!(
            find(StageElementKind::NextSlide).rect,
            default_rect(StageElementKind::NextSlide)
        );
    }

    #[test]
    fn the_booth_message_and_a_running_timer_reach_the_stage_layers() {
        // safe: SAFE-01 (the stage is one of only two places a booth message goes)
        let status = LiveStatus {
            has_active_item: true,
            current_item_title: "Be Thou My Vision".into(),
            current_slide_text: "Be Thou my Wisdom".into(),
            song_key: "Eb".into(),
            automation: vec![LiveAutomationState {
                booth_message: "Repeat verse 4".into(),
                timers: vec![TimerStatus {
                    name: "Worship set".into(),
                    running: true,
                    remaining_ms: 180_000,
                    formatted_time: "03:00".into(),
                    ..TimerStatus::default()
                }],
                ..LiveAutomationState::default()
            }],
            ..LiveStatus::default()
        };
        let layers = compose(&StageInputs {
            status: &status,
            layout: None,
            wall_now_unix_ms: 1_000_000,
        });
        assert_eq!(layers.len(), 2);
        let Some(layer_spec::Content::Slide(slide)) = &layers[0].content else {
            panic!("slide layer first")
        };
        assert_eq!(slide.text, "Be Thou my Wisdom");
        assert!(
            slide
                .elements_json
                .contains("From the booth: Repeat verse 4")
        );
        assert!(slide.elements_json.contains("Eb"));
        let doc = SlideElementsDocument::from_json_strict(&slide.elements_json).unwrap();
        assert!(doc.elements.len() <= 64);
        let Some(layer_spec::Content::Props(props)) = &layers[1].content else {
            panic!("props layer second")
        };
        let live = SlideElementsDocument::from_json_strict(&props.elements_json).unwrap();
        let value = live
            .elements
            .iter()
            .find(|e| e.id == "timer-value")
            .unwrap();
        assert_eq!(value.text, "{timer}");
        assert_eq!(value.timer_target_unix_ms, Some(1_180_000));
        assert!(live.elements.iter().any(|e| e.text == "{clock}"));
        assert!(TemplateStyle::from_json(&slide.style_json).is_ok());
    }
}
