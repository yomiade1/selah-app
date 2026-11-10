//! Metric fit & multi-output slide splitting using Skia HarfBuzz shaping (M3.3 / M3-A2).
//!
//! Uses the exact measurement & layout rules of [`TextRenderer::render_slide`] so every
//! fitted slide satisfies:
//! 1. Zero text glyphs or strokes leave the template's [`crate::template::SafeRect`].
//! 2. Text never shrinks below `min_size` without first splitting across additional slides.
//! 3. Accurate per-output [`OutputFitReport`] metrics (`fits`, `applied_size`, `min_size`,
//!    `original_slide_count`, `final_slide_count`, `extra_splits`, `overflows_safe_area`,
//!    `warning_codes`, `summary`).
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

use anyhow::Result;
use selah_proto::v1::{
    Chord, Color, CopyrightPlacement, FitTargetOutput, GeneratedScriptureSlide, GeneratedSlide, Id,
    LyricLine, OutputFitReport, ReferencePlacement, Resolution, ScriptureVerse, SectionKind,
    SlideLayerSpec,
};

use crate::template::{
    ResolvedTemplate, SafeRect, SlotTextStyle, TemplateStyle, TextAlignV, parse_chords_json,
};
use crate::text::TextRenderer;

pub const WARN_SHRUNK_TO_FIT: &str = "SHRUNK_TO_FIT";
pub const WARN_EXTRA_SPLIT: &str = "EXTRA_SPLIT";
pub const WARN_OVERFLOWS_SAFE_AREA: &str = "OVERFLOWS_SAFE_AREA";
pub const WARN_AT_MIN_SIZE: &str = "AT_MIN_SIZE";

thread_local! {
    static FIT_RENDERER: RefCell<Option<std::result::Result<TextRenderer, String>>> =
        const { RefCell::new(None) };
}

pub fn with_bundled_fit_renderer<R>(f: impl FnOnce(&mut TextRenderer) -> Result<R>) -> Result<R> {
    FIT_RENDERER.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(TextRenderer::bundled().map_err(|e| e.to_string()));
        }
        match slot.as_mut().expect("initialized") {
            Ok(renderer) => f(renderer),
            Err(msg) => anyhow::bail!("{msg}"),
        }
    })
}

/// Target output specification for multi-output metric fitting.
#[derive(Clone, Debug)]
pub struct FitTarget {
    pub output_index: u32,
    pub output_name: String,
    pub width: u32,
    pub height: u32,
    pub style: TemplateStyle,
    pub max_lines_override: Option<u32>,
}

impl FitTarget {
    pub fn from_proto(target: &FitTargetOutput, style: TemplateStyle) -> Self {
        let res = target.resolution.unwrap_or(Resolution {
            width: 1920,
            height: 1080,
        });
        Self {
            output_index: target.output_index,
            output_name: if target.name.trim().is_empty() {
                format!("Output {}", target.output_index + 1)
            } else {
                target.name.clone()
            },
            width: res.width.max(1),
            height: res.height.max(1),
            style,
            max_lines_override: (target.max_lines_override > 0)
                .then_some(target.max_lines_override),
        }
    }
}

/// Arranged song section payload fed into the metric fitter.
#[derive(Clone, Debug)]
pub struct SongSectionInput {
    pub section_id: Option<Id>,
    pub section_label: String,
    pub section_kind: i32,
    pub section_color: Option<Color>,
    pub section_hotkey: String,
    pub lines: Vec<LyricLine>,
    pub translation_lines: Vec<String>,
}

/// Complete song slide-generation input for multi-output metric fitting.
#[derive(Clone, Debug)]
pub struct SongFitInput {
    pub sections: Vec<SongSectionInput>,
    pub title_lines: Option<Vec<String>>,
    pub copyright_line: String,
    pub copyright_placement: CopyrightPlacement,
    pub blank_slide_at_end: bool,
    pub max_lines: u32,
}

#[derive(Clone, Debug)]
pub struct SongFitResult {
    pub primary_slides: Vec<GeneratedSlide>,
    pub per_output: Vec<(OutputFitReport, Vec<GeneratedSlide>)>,
}

/// Complete scripture slide-generation input for multi-output metric fitting.
#[derive(Clone, Debug)]
pub struct ScriptureFitInput {
    pub translation_code: String,
    pub translation_name: String,
    pub passage_label: String,
    pub verses: Vec<ScriptureVerse>,
    pub show_verse_numbers: bool,
    pub verses_per_slide: Option<usize>,
    pub one_verse_per_slide: bool,
    pub reference_placement: ReferencePlacement,
    pub title_slide: bool,
    pub blank_slide_at_end: bool,
    pub max_lines: u32,
    pub baseline_chars_per_line: usize,
}

#[derive(Clone, Debug)]
pub struct ScriptureFitResult {
    pub primary_slides: Vec<GeneratedScriptureSlide>,
    pub per_output: Vec<(OutputFitReport, Vec<GeneratedScriptureSlide>)>,
}

/// Result of verifying whether a rendered `SlideLayerSpec` stays strictly inside `SafeRect`.
#[derive(Clone, Debug)]
pub struct SlideSafeCheck {
    pub fits: bool,
    pub overflows_safe_area: bool,
    pub visual_line_count: usize,
    pub longest_line: f32,
    pub layout_width: f32,
    pub required_height: f32,
    pub available_height: f32,
}

/// Convert a generated song slide into a renderable [`SlideLayerSpec`].
pub fn generated_song_slide_to_layer(
    slide: &GeneratedSlide,
    style_json: Option<&str>,
) -> SlideLayerSpec {
    if slide.is_blank {
        return SlideLayerSpec {
            style_json: style_json.unwrap_or_default().to_owned(),
            ..Default::default()
        };
    }
    if slide.is_title {
        let title_text = slide
            .lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        return SlideLayerSpec {
            title_text,
            footer_text: slide.footer_copyright.clone(),
            style_json: style_json.unwrap_or_default().to_owned(),
            ..Default::default()
        };
    }
    let text = slide
        .lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let translation_text = if slide.translation_lines.iter().any(|l| !l.trim().is_empty()) {
        slide.translation_lines.join("\n")
    } else {
        String::new()
    };
    let chords_per_line: Vec<Vec<&Chord>> = slide
        .lines
        .iter()
        .map(|l| l.chords.iter().collect())
        .collect();
    let chords_json = if chords_per_line.iter().any(|c| !c.is_empty()) {
        serialize_chord_anchors(&chords_per_line)
    } else {
        String::new()
    };
    SlideLayerSpec {
        text,
        translation_text,
        footer_text: slide.footer_copyright.clone(),
        chords_json,
        style_json: style_json.unwrap_or_default().to_owned(),
        ..Default::default()
    }
}

/// Convert a generated scripture slide into a renderable [`SlideLayerSpec`].
pub fn generated_scripture_slide_to_layer(
    slide: &GeneratedScriptureSlide,
    style_json: Option<&str>,
) -> SlideLayerSpec {
    if slide.is_blank {
        return SlideLayerSpec {
            style_json: style_json.unwrap_or_default().to_owned(),
            ..Default::default()
        };
    }
    if slide.is_title {
        return SlideLayerSpec {
            title_text: slide.lines.join("\n"),
            style_json: style_json.unwrap_or_default().to_owned(),
            ..Default::default()
        };
    }
    let reference_text = if !slide.header_reference.is_empty() {
        slide.header_reference.clone()
    } else {
        slide.footer_reference.clone()
    };
    SlideLayerSpec {
        text: slide.lines.join("\n"),
        reference_text,
        style_json: style_json.unwrap_or_default().to_owned(),
        ..Default::default()
    }
}

/// Return a [`ResolvedTemplate`] scaled for a slide's chosen font size (`applied_size` in output
/// pixels). Scales `title` when `is_title`, otherwise `body` plus proportionally `translation`
/// and `chords`. Also aligns `roles.reference.align_v` when scripture placement specifies header
/// or footer.
pub fn scale_resolved_for_slide(
    resolved: &ResolvedTemplate,
    applied_size: f32,
    is_title: bool,
    ref_placement: Option<ReferencePlacement>,
) -> ResolvedTemplate {
    let mut out = resolved.clone();
    if let Some(rp) = ref_placement {
        match rp {
            ReferencePlacement::SlideHeader => out.roles.reference.align_v = TextAlignV::Top,
            ReferencePlacement::SlideFooter => out.roles.reference.align_v = TextAlignV::Bottom,
            _ => {}
        }
    }
    if applied_size <= 0.0 || !applied_size.is_finite() {
        return out;
    }
    if is_title {
        let base = out.roles.title.size.max(8.0);
        let min_s = out.roles.title.min_size.min(base);
        out.roles.title.size = applied_size.clamp(min_s, base);
    } else {
        let base = out.roles.body.size.max(8.0);
        let min_s = out.roles.body.min_size.min(base);
        let clamped = applied_size.clamp(min_s, base);
        let ratio = clamped / base;
        out.roles.body.size = clamped;
        let t_base = out.roles.translation.size.max(8.0);
        let t_min = out.roles.translation.min_size.min(t_base);
        out.roles.translation.size = (t_base * ratio).clamp(t_min, t_base);
        let c_base = out.roles.chords.size.max(8.0);
        let c_min = out.roles.chords.min_size.min(c_base);
        out.roles.chords.size = (c_base * ratio).clamp(c_min, c_base);
    }
    out
}

fn serialize_chord_anchors(lines: &[Vec<&Chord>]) -> String {
    #[derive(serde::Serialize)]
    struct Item<'a> {
        line: usize,
        position: u32,
        text: &'a str,
    }
    let mut items = Vec::new();
    for (line_idx, chords) in lines.iter().enumerate() {
        for c in chords {
            items.push(Item {
                line: line_idx,
                position: c.position,
                text: &c.text,
            });
        }
    }
    serde_json::to_string(&items).unwrap_or_default()
}

fn shadow_pad_y(style: &SlotTextStyle) -> f32 {
    style
        .shadow
        .as_ref()
        .filter(|s| s.color[3] > 0)
        .map_or(0.0, |s| s.offset_y.abs() + s.blur * 0.5)
}

fn shadow_pad_x(style: &SlotTextStyle) -> f32 {
    style
        .shadow
        .as_ref()
        .filter(|s| s.color[3] > 0)
        .map_or(0.0, |s| s.offset_x.abs() + s.blur * 0.5)
}

fn effective_layout_width(max_width: f32, style: &SlotTextStyle) -> f32 {
    TextRenderer::slot_layout_width(max_width, style)
}

fn reserve_header_footer_space(
    renderer: &mut TextRenderer,
    safe: SafeRect,
    footer_text: &str,
    reference_text: &str,
    resolved: &ResolvedTemplate,
) -> (f32, f32, bool) {
    let gap = (safe.height * 0.015).max(4.0).round();
    let mut avail_top = safe.top;
    let mut avail_bottom = safe.bottom;
    let mut aux_fits = true;

    if resolved.roles.footer.visible && !footer_text.trim().is_empty() {
        let st = &resolved.roles.footer;
        let layout_w = effective_layout_width(safe.width, st);
        let unclipped = SlotTextStyle {
            max_lines: None,
            ..st.clone()
        };
        let meas = renderer.measure_slot(footer_text, &unclipped, safe.width);
        let slot_h = meas.total_height_with_decorations(st) + shadow_pad_y(st) * 2.0;
        if meas.min_intrinsic_width > layout_w || meas.longest_line > layout_w {
            aux_fits = false;
        }
        if slot_h > (avail_bottom - avail_top) {
            aux_fits = false;
        }
        let clamped_h = slot_h.min((avail_bottom - avail_top).max(1.0));
        match st.align_v {
            TextAlignV::Top => avail_top = (avail_top + clamped_h + gap).min(avail_bottom),
            TextAlignV::Middle | TextAlignV::Bottom => {
                avail_bottom = (avail_bottom - clamped_h - gap).max(avail_top);
            }
        }
    }

    if resolved.roles.reference.visible && !reference_text.trim().is_empty() {
        let st = &resolved.roles.reference;
        let layout_w = effective_layout_width(safe.width, st);
        let unclipped = SlotTextStyle {
            max_lines: None,
            ..st.clone()
        };
        let meas = renderer.measure_slot(reference_text, &unclipped, safe.width);
        let slot_h = meas.total_height_with_decorations(st) + shadow_pad_y(st) * 2.0;
        if meas.min_intrinsic_width > layout_w || meas.longest_line > layout_w {
            aux_fits = false;
        }
        if slot_h > (avail_bottom - avail_top) {
            aux_fits = false;
        }
        let clamped_h = slot_h.min((avail_bottom - avail_top).max(1.0));
        match st.align_v {
            TextAlignV::Top => avail_top = (avail_top + clamped_h + gap).min(avail_bottom),
            TextAlignV::Middle | TextAlignV::Bottom => {
                avail_bottom = (avail_bottom - clamped_h - gap).max(avail_top);
            }
        }
    }

    (safe.width, (avail_bottom - avail_top).max(1.0), aux_fits)
}

/// Measure whether `slide` fits inside `resolved`'s [`SafeArea`] without horizontal spill,
/// vertical overflow, or exceeding `max_lines` clipping.
pub fn measure_slide_fits_in_safe_area(
    renderer: &mut TextRenderer,
    slide: &SlideLayerSpec,
    resolved: &ResolvedTemplate,
) -> SlideSafeCheck {
    let safe = resolved
        .safe_area
        .resolved_rect(resolved.width, resolved.height);
    let has_title = resolved.roles.title.visible && !slide.title_text.trim().is_empty();
    let has_body = resolved.roles.body.visible && !slide.text.trim().is_empty();
    let has_trans = resolved.roles.translation.visible && !slide.translation_text.trim().is_empty();
    let has_ref = resolved.roles.reference.visible && !slide.reference_text.trim().is_empty();

    let (mut avail_w, mut avail_h, mut fits) = reserve_header_footer_space(
        renderer,
        safe,
        &slide.footer_text,
        if has_ref { &slide.reference_text } else { "" },
        resolved,
    );

    if has_title && !has_body && !has_trans && !has_ref {
        let title_style = &resolved.roles.title;
        let layout_w = effective_layout_width(avail_w, title_style);
        let unclipped = SlotTextStyle {
            max_lines: None,
            ..title_style.clone()
        };
        let m = renderer.measure_slot(&slide.title_text, &unclipped, avail_w);
        let req_h =
            m.total_height_with_decorations(title_style) + shadow_pad_y(title_style) * 2.0 + 2.0;
        let line_ok = title_style
            .max_lines
            .is_none_or(|c| m.line_count <= c as usize);
        let horiz_ok = m.min_intrinsic_width <= layout_w
            && m.longest_line <= layout_w
            && m.min_line_left >= -0.25
            && m.max_line_right <= layout_w + 0.25;
        let vert_ok = req_h <= avail_h;
        fits &= line_ok && horiz_ok && vert_ok;
        return SlideSafeCheck {
            fits,
            overflows_safe_area: !(horiz_ok && vert_ok),
            visual_line_count: m.line_count,
            longest_line: m.longest_line,
            layout_width: layout_w,
            required_height: req_h,
            available_height: avail_h,
        };
    }

    if has_title {
        let gap = (safe.height * 0.015).max(4.0).round();
        let st = &resolved.roles.title;
        let layout_w = effective_layout_width(avail_w, st);
        let unclipped = SlotTextStyle {
            max_lines: None,
            ..st.clone()
        };
        let m = renderer.measure_slot(&slide.title_text, &unclipped, avail_w);
        let title_h = m.total_height_with_decorations(st) + shadow_pad_y(st) * 2.0;
        if m.min_intrinsic_width > layout_w || m.longest_line > layout_w || title_h + gap > avail_h
        {
            fits = false;
        }
        avail_h = (avail_h - title_h - gap).max(1.0);
    }

    if !has_body && !has_trans {
        return SlideSafeCheck {
            fits,
            overflows_safe_area: !fits,
            visual_line_count: 0,
            longest_line: 0.0,
            layout_width: avail_w,
            required_height: 0.0,
            available_height: avail_h,
        };
    }

    let chords = if resolved.roles.chords.visible {
        parse_chords_json(&slide.chords_json)
    } else {
        HashMap::new()
    };

    if !chords.is_empty() && has_body {
        let body_style = &resolved.roles.body;
        let stroke_pad = body_style.outline.as_ref().map_or(0.0, |o| o.width);
        let layout_w = (avail_w - stroke_pad * 2.0 - shadow_pad_x(body_style) * 2.0 - 2.0).max(1.0);
        let (total_h, max_w, visual_lines, min_intrinsic) = renderer.measure_body_with_chords(
            &slide.text,
            &chords,
            body_style,
            &resolved.roles.chords,
            has_trans.then_some((&slide.translation_text, &resolved.roles.translation)),
            avail_w,
        );
        let req_h = total_h + shadow_pad_y(body_style) * 2.0 + 2.0;
        let line_cap = body_style.max_lines.or(resolved.max_lines);
        let line_ok = line_cap.is_none_or(|c| visual_lines <= c as usize);
        let horiz_ok = min_intrinsic <= layout_w && max_w <= layout_w;
        let vert_ok = req_h <= avail_h;
        fits &= line_ok && horiz_ok && vert_ok;
        return SlideSafeCheck {
            fits,
            overflows_safe_area: !(horiz_ok && vert_ok),
            visual_line_count: visual_lines,
            longest_line: max_w,
            layout_width: layout_w,
            required_height: req_h,
            available_height: avail_h,
        };
    }

    let body_style = &resolved.roles.body;
    let body_layout_w = effective_layout_width(avail_w, body_style);
    let mut visual_lines = 0usize;
    let mut longest_line = 0.0f32;
    let mut req_h = 0.0f32;
    let mut horiz_ok = true;
    let mut line_ok = true;

    if has_body {
        let unclipped = SlotTextStyle {
            max_lines: None,
            ..body_style.clone()
        };
        let bm = renderer.measure_slot(&slide.text, &unclipped, avail_w);
        visual_lines = bm.line_count;
        longest_line = bm.longest_line;
        avail_w = body_layout_w;
        req_h += bm.total_height_with_decorations(body_style) + shadow_pad_y(body_style) * 2.0;
        horiz_ok &= bm.min_intrinsic_width <= body_layout_w
            && bm.longest_line <= body_layout_w
            && bm.min_line_left >= -0.25
            && bm.max_line_right <= body_layout_w + 0.25;
        let line_cap = body_style.max_lines.or(resolved.max_lines);
        line_ok &= line_cap.is_none_or(|c| bm.line_count <= c as usize);
    }

    if has_trans {
        let trans_style = &resolved.roles.translation;
        let trans_layout_w = effective_layout_width(safe.width, trans_style);
        let unclipped = SlotTextStyle {
            max_lines: None,
            ..trans_style.clone()
        };
        let tm = renderer.measure_slot(&slide.translation_text, &unclipped, safe.width);
        let th = tm.total_height_with_decorations(trans_style) + shadow_pad_y(trans_style) * 2.0;
        if has_body {
            req_h += (body_style.size * 0.3).max(4.0).round() + th;
        } else {
            req_h += th;
        }
        horiz_ok &= tm.min_intrinsic_width <= trans_layout_w
            && tm.longest_line <= trans_layout_w
            && tm.min_line_left >= -0.25
            && tm.max_line_right <= trans_layout_w + 0.25;
        line_ok &= trans_style
            .max_lines
            .is_none_or(|c| tm.line_count <= c as usize);
        longest_line = longest_line.max(tm.longest_line);
        visual_lines += tm.line_count;
    }

    req_h += 2.0;
    let vert_ok = req_h <= avail_h;
    fits &= line_ok && horiz_ok && vert_ok;
    SlideSafeCheck {
        fits,
        overflows_safe_area: !(horiz_ok && vert_ok),
        visual_line_count: visual_lines,
        longest_line,
        layout_width: avail_w,
        required_height: req_h,
        available_height: avail_h,
    }
}

struct FitContext<'a> {
    footer_text: &'a str,
    reference_text: &'a str,
    resolved: &'a ResolvedTemplate,
    is_title: bool,
    ref_placement: Option<ReferencePlacement>,
}

fn check_chunk_fits(
    renderer: &mut TextRenderer,
    lines: &[LyricLine],
    translation_lines: &[String],
    ctx: &FitContext<'_>,
    candidate_size: f32,
) -> SlideSafeCheck {
    let scaled = scale_resolved_for_slide(
        ctx.resolved,
        candidate_size,
        ctx.is_title,
        ctx.ref_placement,
    );
    let temp_slide = GeneratedSlide {
        lines: lines.to_vec(),
        translation_lines: translation_lines.to_vec(),
        footer_copyright: ctx.footer_text.to_owned(),
        is_title: ctx.is_title,
        ..Default::default()
    };
    let mut layer = generated_song_slide_to_layer(&temp_slide, None);
    if !ctx.is_title && !ctx.reference_text.is_empty() {
        layer.reference_text = ctx.reference_text.to_owned();
    }
    measure_slide_fits_in_safe_area(renderer, &layer, &scaled)
}

fn break_long_tokens_in_line(
    renderer: &mut TextRenderer,
    line: &LyricLine,
    trans: &str,
    style: &SlotTextStyle,
    max_width: f32,
) -> Vec<(LyricLine, String)> {
    let layout_w = effective_layout_width(max_width, style);
    let unclipped = SlotTextStyle {
        max_lines: None,
        ..style.clone()
    };
    let whole = renderer.measure_slot(&line.text, &unclipped, max_width);
    if whole.min_intrinsic_width <= layout_w {
        return vec![(line.clone(), trans.to_owned())];
    }

    // Walk space-delimited spans in `line.text` and break any individual token that cannot fit in `layout_w`.
    let chars: Vec<char> = line.text.chars().collect();
    if chars.is_empty() {
        return vec![(line.clone(), trans.to_owned())];
    }

    // Build segments of `line.text` where every token in a segment fits `layout_w`.
    let mut out_segments: Vec<(usize, usize)> = Vec::new();
    let mut cur_start = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let tok_start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let tok_end = i;
        let tok_str: String = chars[tok_start..tok_end].iter().collect();
        let m = renderer.measure_slot(&tok_str, &unclipped, max_width);
        if m.min_intrinsic_width <= layout_w {
            continue;
        }

        // Token at [tok_start..tok_end] itself exceeds layout_w; slice this token into chunks
        // that each satisfy `min_intrinsic_width <= layout_w`.
        let tok_chars = &chars[tok_start..tok_end];
        let mut t_pos = 0usize;
        while t_pos < tok_chars.len() {
            let mut lo = 1usize;
            let mut hi = tok_chars.len() - t_pos;
            let mut best = 1usize;
            while lo <= hi {
                let mid = (lo + hi) / 2;
                let probe: String = tok_chars[t_pos..t_pos + mid].iter().collect();
                let pm = renderer.measure_slot(&probe, &unclipped, max_width);
                if pm.longest_line <= layout_w && pm.min_intrinsic_width <= layout_w {
                    best = mid;
                    lo = mid + 1;
                } else if mid == 1 {
                    best = 1;
                    break;
                } else {
                    hi = mid - 1;
                }
            }
            let abs_start = if t_pos == 0 && cur_start < tok_start {
                let with_prefix: String = chars[cur_start..tok_start + best].iter().collect();
                let wm = renderer.measure_slot(&with_prefix, &unclipped, max_width);
                if wm.min_intrinsic_width <= layout_w {
                    let seg_start = cur_start;
                    cur_start = tok_start + best;
                    out_segments.push((seg_start, cur_start));
                    t_pos += best;
                    continue;
                } else {
                    out_segments.push((cur_start, tok_start));
                    tok_start
                }
            } else {
                tok_start + t_pos
            };
            let abs_end = tok_start + t_pos + best;
            out_segments.push((abs_start, abs_end));
            cur_start = abs_end;
            t_pos += best;
        }
    }

    if out_segments.is_empty() {
        return vec![(line.clone(), trans.to_owned())];
    }
    if cur_start < chars.len() {
        let rem: String = chars[cur_start..].iter().collect();
        if !rem.trim().is_empty() {
            out_segments.push((cur_start, chars.len()));
        }
    }

    let mut result = Vec::with_capacity(out_segments.len());
    for (seg_idx, (s, e)) in out_segments.into_iter().enumerate() {
        let raw_seg: String = chars[s..e].iter().collect();
        let trimmed = raw_seg.trim();
        if trimmed.is_empty() {
            continue;
        }
        let leading_ws = raw_seg.chars().take_while(|c| c.is_whitespace()).count();
        let seg_start_trimmed = s + leading_ws;
        let seg_len = trimmed.chars().count();
        let chords = line
            .chords
            .iter()
            .filter_map(|c| {
                let p = c.position as usize;
                if p >= s && p < e {
                    Some(Chord {
                        position: p.saturating_sub(seg_start_trimmed).min(seg_len) as u32,
                        text: c.text.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();
        result.push((
            LyricLine {
                text: trimmed.to_owned(),
                chords,
            },
            if seg_idx == 0 {
                trans.to_owned()
            } else {
                String::new()
            },
        ));
    }

    if result.is_empty() {
        vec![(line.clone(), trans.to_owned())]
    } else {
        result
    }
}

fn preprocess_overlong_tokens(
    renderer: &mut TextRenderer,
    lines: &[LyricLine],
    trans: &[String],
    style: &SlotTextStyle,
    max_width: f32,
) -> (Vec<LyricLine>, Vec<String>) {
    let mut new_lines = Vec::with_capacity(lines.len());
    let mut new_trans = Vec::with_capacity(lines.len());
    let has_any_trans = !trans.is_empty();
    for (idx, line) in lines.iter().enumerate() {
        let t = trans.get(idx).map(String::as_str).unwrap_or("");
        for (sub_line, sub_t) in break_long_tokens_in_line(renderer, line, t, style, max_width) {
            new_lines.push(sub_line);
            if has_any_trans {
                new_trans.push(sub_t);
            }
        }
    }
    (new_lines, new_trans)
}

fn split_single_line_at_word_boundary(
    renderer: &mut TextRenderer,
    line: &LyricLine,
    trans: &str,
    ctx: &FitContext<'_>,
    min_size: f32,
) -> Option<((LyricLine, String), (LyricLine, String))> {
    let chars: Vec<char> = line.text.chars().collect();
    // Collect char end positions after each word separator boundary.
    let mut word_ends: Vec<usize> = Vec::new();
    let mut in_word = false;
    for (idx, &ch) in chars.iter().enumerate() {
        if ch.is_whitespace() {
            if in_word {
                word_ends.push(idx);
                in_word = false;
            }
        } else {
            in_word = true;
        }
    }
    if in_word {
        word_ends.push(chars.len());
    }
    if word_ends.len() < 2 {
        // Fallback: split mid-string at char boundaries if already a single token somehow overflowed.
        if chars.len() < 2 {
            return None;
        }
        let mut lo = 1usize;
        let mut hi = chars.len() - 1;
        let mut best = 0usize;
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let prefix_text: String = chars[..mid].iter().collect();
            let probe = LyricLine {
                text: prefix_text,
                chords: Vec::new(),
            };
            let t_slice = if trans.is_empty() {
                Vec::new()
            } else {
                vec![trans.to_owned()]
            };
            let chk = check_chunk_fits(renderer, &[probe], &t_slice, ctx, min_size);
            if chk.fits {
                best = mid;
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        if best == 0 {
            return None;
        }
        return Some(split_line_at_char_index(line, trans, best));
    }

    let mut lo = 0usize;
    let mut hi = word_ends.len() - 2; // never take all words
    let mut best_word_idx = None;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let end_char = word_ends[mid];
        let prefix_text: String = chars[..end_char].iter().collect();
        let probe = LyricLine {
            text: prefix_text,
            chords: Vec::new(),
        };
        let t_slice = if trans.is_empty() {
            Vec::new()
        } else {
            vec![trans.to_owned()]
        };
        let chk = check_chunk_fits(renderer, &[probe], &t_slice, ctx, min_size);
        if chk.fits {
            best_word_idx = Some(mid);
            lo = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            hi = mid - 1;
        }
    }

    let best_idx = best_word_idx?;
    let split_char = word_ends[best_idx];
    Some(split_line_at_char_index(line, trans, split_char))
}

fn split_line_at_char_index(
    line: &LyricLine,
    trans: &str,
    split_char: usize,
) -> ((LyricLine, String), (LyricLine, String)) {
    let chars: Vec<char> = line.text.chars().collect();
    let split = split_char.clamp(1, chars.len().saturating_sub(1));
    let left_raw: String = chars[..split].iter().collect();
    let right_raw: String = chars[split..].iter().collect();
    let left_text = left_raw.trim_end().to_owned();
    let left_len = left_text.chars().count();
    let right_leading_ws = right_raw.chars().take_while(|c| c.is_whitespace()).count();
    let right_start = split + right_leading_ws;
    let right_text = right_raw.trim_start().to_owned();
    let right_len = right_text.chars().count();

    let mut left_chords = Vec::new();
    let mut right_chords = Vec::new();
    for c in &line.chords {
        let p = c.position as usize;
        if p < split {
            left_chords.push(Chord {
                position: p.min(left_len) as u32,
                text: c.text.clone(),
            });
        } else {
            right_chords.push(Chord {
                position: p.saturating_sub(right_start).min(right_len) as u32,
                text: c.text.clone(),
            });
        }
    }

    (
        (
            LyricLine {
                text: left_text,
                chords: left_chords,
            },
            trans.to_owned(),
        ),
        (
            LyricLine {
                text: right_text,
                chords: right_chords,
            },
            String::new(),
        ),
    )
}

fn split_translation_if_oversized(
    renderer: &mut TextRenderer,
    line: &LyricLine,
    trans: &str,
    ctx: &FitContext<'_>,
    min_size: f32,
) -> Option<(String, String)> {
    let words: Vec<&str> = trans.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let mut lo = 1usize;
    let mut hi = words.len() - 1;
    let mut best = 1usize;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let probe_t = words[..mid].join(" ");
        let chk = check_chunk_fits(
            renderer,
            std::slice::from_ref(line),
            &[probe_t],
            ctx,
            min_size,
        );
        if chk.fits {
            best = mid;
            lo = mid + 1;
        } else if mid == 1 {
            break;
        } else {
            hi = mid - 1;
        }
    }
    Some((words[..best].join(" "), words[best..].join(" ")))
}

fn maximize_size_for_chunk(
    renderer: &mut TextRenderer,
    lines: &[LyricLine],
    trans: &[String],
    ctx: &FitContext<'_>,
    min_size: f32,
    max_size: f32,
) -> f32 {
    if check_chunk_fits(renderer, lines, trans, ctx, max_size).fits {
        return max_size;
    }
    let mut lo = min_size;
    let mut hi = max_size;
    for _ in 0..8 {
        if hi - lo <= 0.5 {
            break;
        }
        let mid = 0.5 * (lo + hi);
        if check_chunk_fits(renderer, lines, trans, ctx, mid).fits {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let mut candidate = ((lo * 2.0).floor() / 2.0).max(min_size);
    while candidate > min_size {
        if check_chunk_fits(renderer, lines, trans, ctx, candidate).fits {
            return candidate;
        }
        candidate = (candidate - 0.5).max(min_size);
    }
    min_size
}

#[derive(Clone, Debug)]
struct FittedUnit {
    lines: Vec<LyricLine>,
    translation_lines: Vec<String>,
    applied_size: f32,
    overflows_safe_area: bool,
}

fn fit_chunk_sequence(
    renderer: &mut TextRenderer,
    initial_chunks: Vec<(Vec<LyricLine>, Vec<String>)>,
    ctx: &FitContext<'_>,
) -> Vec<FittedUnit> {
    let slot_style = if ctx.is_title {
        &ctx.resolved.roles.title
    } else {
        &ctx.resolved.roles.body
    };
    let max_size = slot_style.size.max(8.0);
    let min_size = slot_style.min_size.clamp(8.0, max_size);
    let safe = ctx
        .resolved
        .safe_area
        .resolved_rect(ctx.resolved.width, ctx.resolved.height);
    let min_style =
        scale_resolved_for_slide(ctx.resolved, min_size, ctx.is_title, ctx.ref_placement);
    let min_slot_style = if ctx.is_title {
        &min_style.roles.title
    } else {
        &min_style.roles.body
    };

    let mut queue: VecDeque<(Vec<LyricLine>, Vec<String>)> = initial_chunks.into_iter().collect();
    let mut out = Vec::new();
    let mut safety_iters = 0usize;

    while let Some((raw_lines, raw_trans)) = queue.pop_front() {
        safety_iters += 1;
        if safety_iters > 4096 {
            out.push(FittedUnit {
                lines: raw_lines,
                translation_lines: raw_trans,
                applied_size: min_size,
                overflows_safe_area: true,
            });
            break;
        }

        if raw_lines.is_empty() {
            out.push(FittedUnit {
                lines: Vec::new(),
                translation_lines: Vec::new(),
                applied_size: max_size,
                overflows_safe_area: false,
            });
            continue;
        }

        // Fast path: fits cleanly at base template size.
        let fast = check_chunk_fits(renderer, &raw_lines, &raw_trans, ctx, max_size);
        if fast.fits {
            out.push(FittedUnit {
                lines: raw_lines,
                translation_lines: raw_trans,
                applied_size: max_size,
                overflows_safe_area: false,
            });
            continue;
        }

        // Next: try min_size before preprocessing tokens (avoids chopping a word that fits at smaller size).
        let at_min_raw = check_chunk_fits(renderer, &raw_lines, &raw_trans, ctx, min_size);
        if at_min_raw.fits {
            let best =
                maximize_size_for_chunk(renderer, &raw_lines, &raw_trans, ctx, min_size, max_size);
            out.push(FittedUnit {
                lines: raw_lines,
                translation_lines: raw_trans,
                applied_size: best,
                overflows_safe_area: false,
            });
            continue;
        }

        // Break any unbreakable token that overflows width even at min_size.
        let (lines, mut trans) = preprocess_overlong_tokens(
            renderer,
            &raw_lines,
            &raw_trans,
            min_slot_style,
            safe.width,
        );
        // Break oversized translation tokens too if present.
        if !ctx.is_title && !trans.is_empty() && ctx.resolved.roles.translation.visible {
            let trans_min = &min_style.roles.translation;
            let trans_layout_w = effective_layout_width(safe.width, trans_min);
            let unclipped_t = SlotTextStyle {
                max_lines: None,
                ..trans_min.clone()
            };
            for t_item in &mut trans {
                if !t_item.trim().is_empty() {
                    let tm = renderer.measure_slot(t_item, &unclipped_t, safe.width);
                    if tm.min_intrinsic_width > trans_layout_w {
                        let dummy = LyricLine {
                            text: t_item.clone(),
                            chords: Vec::new(),
                        };
                        let broken =
                            break_long_tokens_in_line(renderer, &dummy, "", trans_min, safe.width);
                        *t_item = broken
                            .into_iter()
                            .map(|(l, _)| l.text)
                            .collect::<Vec<_>>()
                            .join(" ");
                    }
                }
            }
        }

        // Check if token wrapping already made it fit at min_size.
        if check_chunk_fits(renderer, &lines, &trans, ctx, min_size).fits {
            let best = maximize_size_for_chunk(renderer, &lines, &trans, ctx, min_size, max_size);
            out.push(FittedUnit {
                lines,
                translation_lines: trans,
                applied_size: best,
                overflows_safe_area: false,
            });
            continue;
        }

        // Must split across slides without dropping below min_size!
        if lines.len() > 1 {
            // Find largest prefix [0..k] (`k >= 1`) that fits at `min_size`.
            let mut k = lines.len() - 1;
            let mut found = 0usize;
            while k >= 1 {
                let t_prefix: Vec<String> = if trans.is_empty() {
                    Vec::new()
                } else {
                    trans[..k].to_vec()
                };
                if check_chunk_fits(renderer, &lines[..k], &t_prefix, ctx, min_size).fits {
                    found = k;
                    break;
                }
                k -= 1;
            }
            if found >= 1 {
                let first_lines = lines[..found].to_vec();
                let first_trans = if trans.is_empty() {
                    Vec::new()
                } else {
                    trans[..found].to_vec()
                };
                let rem_lines = lines[found..].to_vec();
                let rem_trans = if trans.is_empty() {
                    Vec::new()
                } else {
                    trans[found..].to_vec()
                };
                let best = maximize_size_for_chunk(
                    renderer,
                    &first_lines,
                    &first_trans,
                    ctx,
                    min_size,
                    max_size,
                );
                out.push(FittedUnit {
                    lines: first_lines,
                    translation_lines: first_trans,
                    applied_size: best,
                    overflows_safe_area: false,
                });
                queue.push_front((rem_lines, rem_trans));
                continue;
            }
        }

        // Even lines[0] alone does not fit at min_size: split lines[0] (or its translation) across slides!
        let t0 = trans.first().map(String::as_str).unwrap_or("");
        if let Some(((l_head, t_head), (l_tail, t_tail))) =
            split_single_line_at_word_boundary(renderer, &lines[0], t0, ctx, min_size)
        {
            let first_t = if trans.is_empty() {
                Vec::new()
            } else {
                vec![t_head]
            };
            let best = maximize_size_for_chunk(
                renderer,
                std::slice::from_ref(&l_head),
                &first_t,
                ctx,
                min_size,
                max_size,
            );
            let chk_head =
                check_chunk_fits(renderer, std::slice::from_ref(&l_head), &first_t, ctx, best);
            out.push(FittedUnit {
                lines: vec![l_head],
                translation_lines: first_t,
                applied_size: best,
                overflows_safe_area: chk_head.overflows_safe_area,
            });
            let mut next_lines = Vec::with_capacity(lines.len());
            if !l_tail.text.trim().is_empty() {
                next_lines.push(l_tail);
            }
            next_lines.extend(lines[1..].iter().cloned());
            let mut next_trans = Vec::new();
            if !trans.is_empty() && !next_lines.is_empty() {
                next_trans.push(t_tail);
                next_trans.extend(trans[1..].iter().cloned());
            }
            if !next_lines.is_empty() {
                queue.push_front((next_lines, next_trans));
            }
            continue;
        }

        // Maybe translation on a short single lyric line caused the vertical overflow:
        if !ctx.is_title
            && !t0.trim().is_empty()
            && let Some((t_head, t_tail)) =
                split_translation_if_oversized(renderer, &lines[0], t0, ctx, min_size)
        {
            let best = maximize_size_for_chunk(
                renderer,
                std::slice::from_ref(&lines[0]),
                std::slice::from_ref(&t_head),
                ctx,
                min_size,
                max_size,
            );
            out.push(FittedUnit {
                lines: vec![lines[0].clone()],
                translation_lines: vec![t_head],
                applied_size: best,
                overflows_safe_area: false,
            });
            let mut next_lines = vec![LyricLine {
                text: String::new(),
                chords: Vec::new(),
            }];
            next_lines.extend(lines[1..].iter().cloned());
            let mut next_trans = vec![t_tail];
            next_trans.extend(trans[1..].iter().cloned());
            queue.push_front((next_lines, next_trans));
            continue;
        }

        // Impossible constraint (e.g. min_size larger than safe height): report overflow honestly.
        out.push(FittedUnit {
            lines,
            translation_lines: trans,
            applied_size: min_size,
            overflows_safe_area: true,
        });
    }

    out
}

fn format_summary(report: &OutputFitReport) -> String {
    if !report.fits {
        return format!(
            "{} ({}x{}): overflows safe area at {:.0} px",
            report.output_name,
            report.resolution.map_or(0, |r| r.width),
            report.resolution.map_or(0, |r| r.height),
            report.min_size
        );
    }
    if report.extra_splits > 0 {
        format!(
            "{} ({}x{}): fits in {} slides (+{} split, {:.0} px)",
            report.output_name,
            report.resolution.map_or(0, |r| r.width),
            report.resolution.map_or(0, |r| r.height),
            report.final_slide_count,
            report.extra_splits,
            report.applied_size
        )
    } else if (report.applied_size - report.min_size).abs() < 0.5
        && report.warning_codes.iter().any(|w| w == WARN_SHRUNK_TO_FIT)
    {
        format!(
            "{} ({}x{}): fits at min size {:.0} px ({} slides)",
            report.output_name,
            report.resolution.map_or(0, |r| r.width),
            report.resolution.map_or(0, |r| r.height),
            report.applied_size,
            report.final_slide_count
        )
    } else {
        format!(
            "{} ({}x{}): fits ({} slides, {:.0} px)",
            report.output_name,
            report.resolution.map_or(0, |r| r.width),
            report.resolution.map_or(0, |r| r.height),
            report.final_slide_count,
            report.applied_size
        )
    }
}

fn fit_footer_text_for_output(
    renderer: &mut TextRenderer,
    raw_footer: &str,
    resolved: &ResolvedTemplate,
) -> String {
    let trimmed = raw_footer.trim();
    if trimmed.is_empty() || !resolved.roles.footer.visible {
        return String::new();
    }
    let safe = resolved
        .safe_area
        .resolved_rect(resolved.width, resolved.height);
    let st = &resolved.roles.footer;
    let layout_w = effective_layout_width(safe.width, st);
    let unclipped = SlotTextStyle {
        max_lines: None,
        ..st.clone()
    };
    let meas = renderer.measure_slot(trimmed, &unclipped, safe.width);
    if meas.min_intrinsic_width <= layout_w && meas.longest_line <= layout_w {
        return trimmed.to_owned();
    }
    let line = LyricLine {
        text: trimmed.to_owned(),
        chords: Vec::new(),
    };
    break_long_tokens_in_line(renderer, &line, "", st, safe.width)
        .into_iter()
        .map(|(l, _)| l.text)
        .collect::<Vec<_>>()
        .join(" ")
}

fn apply_copyright_with_fit_guard(
    renderer: &mut TextRenderer,
    slides: &mut Vec<GeneratedSlide>,
    section_group_ids: &mut Vec<Option<usize>>,
    footer_copyright: &str,
    placement: CopyrightPlacement,
    resolved: &ResolvedTemplate,
) -> bool {
    if footer_copyright.trim().is_empty() || slides.is_empty() {
        return false;
    }
    let content_len = slides.len();
    let indices: Vec<usize> = match placement {
        CopyrightPlacement::Unspecified | CopyrightPlacement::None => return false,
        CopyrightPlacement::First => vec![0],
        CopyrightPlacement::Last => vec![content_len - 1],
        CopyrightPlacement::FirstAndLast => {
            if content_len > 1 {
                vec![0, content_len - 1]
            } else {
                vec![0]
            }
        }
        CopyrightPlacement::All => (0..content_len).collect(),
    };
    for &idx in &indices {
        slides[idx].footer_copyright = footer_copyright.to_owned();
    }

    // Re-check any newly footered non-title slide; if adding footer pushed a tight slide past avail_h,
    // shrink or split that specific slide cleanly while preserving arranged section group IDs.
    let mut overflow_any = false;
    if matches!(
        placement,
        CopyrightPlacement::First | CopyrightPlacement::Last | CopyrightPlacement::FirstAndLast
    ) {
        let mut rebuilt: Vec<GeneratedSlide> = Vec::with_capacity(slides.len());
        let mut rebuilt_groups: Vec<Option<usize>> = Vec::with_capacity(slides.len());
        let orig_groups = std::mem::take(section_group_ids);
        let total_orig = slides.len();
        for (i, slide) in slides.drain(..).enumerate() {
            let gid = orig_groups.get(i).copied().flatten();
            if slide.is_blank || slide.footer_copyright.is_empty() {
                rebuilt.push(slide);
                rebuilt_groups.push(gid);
                continue;
            }
            let ctx = FitContext {
                footer_text: &slide.footer_copyright,
                reference_text: "",
                resolved,
                is_title: slide.is_title,
                ref_placement: None,
            };
            let check = check_chunk_fits(
                renderer,
                &slide.lines,
                &slide.translation_lines,
                &ctx,
                slide.applied_size.max(8.0),
            );
            if check.fits {
                rebuilt.push(slide);
                rebuilt_groups.push(gid);
                continue;
            }
            let units = fit_chunk_sequence(
                renderer,
                vec![(slide.lines.clone(), slide.translation_lines.clone())],
                &ctx,
            );
            let keep_first = placement == CopyrightPlacement::First
                || (placement == CopyrightPlacement::FirstAndLast && i == 0);
            let keep_last = placement == CopyrightPlacement::Last
                || (placement == CopyrightPlacement::FirstAndLast && i + 1 == total_orig);
            let u_len = units.len();
            for (u_idx, u) in units.into_iter().enumerate() {
                overflow_any |= u.overflows_safe_area;
                let attach = (keep_first && u_idx == 0) || (keep_last && u_idx + 1 == u_len);
                rebuilt.push(GeneratedSlide {
                    footer_copyright: if attach {
                        footer_copyright.to_owned()
                    } else {
                        String::new()
                    },
                    lines: u.lines,
                    translation_lines: u.translation_lines,
                    applied_size: u.applied_size,
                    ..slide.clone()
                });
                rebuilt_groups.push(gid);
            }
        }
        *slides = rebuilt;
        *section_group_ids = rebuilt_groups;
    }
    overflow_any
}

fn recount_section_parts_by_group(slides: &mut [GeneratedSlide], group_ids: &[Option<usize>]) {
    let mut idx = 0usize;
    while idx < slides.len() {
        let Some(gid) = group_ids.get(idx).copied().flatten() else {
            slides[idx].part_index = 1;
            slides[idx].part_count = 1;
            idx += 1;
            continue;
        };
        let mut end = idx + 1;
        while end < slides.len() && group_ids.get(end).copied().flatten() == Some(gid) {
            end += 1;
        }
        let count = (end - idx) as u32;
        for (p_idx, slide) in slides[idx..end].iter_mut().enumerate() {
            slide.part_index = (p_idx + 1) as u32;
            slide.part_count = count;
        }
        idx = end;
    }
}

/// Fit a complete song across all target outputs using `renderer` (M3.3 / M3-A2).
pub fn fit_song_slides_with_renderer(
    renderer: &mut TextRenderer,
    input: &SongFitInput,
    targets: &[FitTarget],
) -> Result<SongFitResult> {
    let default_target;
    let active_targets = if targets.is_empty() {
        default_target = vec![FitTarget {
            output_index: 0,
            output_name: "Main Output".into(),
            width: 1920,
            height: 1080,
            style: TemplateStyle::default(),
            max_lines_override: None,
        }];
        default_target.as_slice()
    } else {
        targets
    };

    let req_max_lines = if input.max_lines == 0 {
        4
    } else {
        input.max_lines.clamp(1, 100)
    };
    // Compute baseline (rule-split) slide count with `req_max_lines`.
    let baseline_title_count =
        usize::from(input.title_lines.as_ref().is_some_and(|l| !l.is_empty()));
    let baseline_blank_count = usize::from(input.blank_slide_at_end);
    let baseline_body_count: usize = input
        .sections
        .iter()
        .map(|s| {
            if s.lines.is_empty() {
                1
            } else {
                s.lines.len().div_ceil(req_max_lines as usize)
            }
        })
        .sum();
    let original_slide_count =
        (baseline_title_count + baseline_body_count + baseline_blank_count) as u32;

    let mut per_output = Vec::with_capacity(active_targets.len());

    for target in active_targets {
        let resolved =
            target
                .style
                .resolve_for_output(target.width, target.height, target.max_lines_override);
        let eff_max_lines = target
            .max_lines_override
            .or_else(|| {
                resolved.max_lines.map(|vm| {
                    if input.max_lines > 0 {
                        req_max_lines.min(vm)
                    } else {
                        vm
                    }
                })
            })
            .unwrap_or(req_max_lines)
            .clamp(1, 100) as usize;

        let safe_footer = fit_footer_text_for_output(renderer, &input.copyright_line, &resolved);
        let always_reserve_footer = matches!(input.copyright_placement, CopyrightPlacement::All);
        let base_body_size = resolved.roles.body.size.max(8.0);
        let min_body_size = resolved.roles.body.min_size.clamp(8.0, base_body_size);

        let mut slides: Vec<GeneratedSlide> = Vec::new();
        let mut overflow_any = false;
        let mut section_group_ids: Vec<Option<usize>> = Vec::new();

        if let Some(title_lines) = &input.title_lines
            && !title_lines.is_empty()
        {
            let lyric_title_lines: Vec<LyricLine> = title_lines
                .iter()
                .map(|t| LyricLine {
                    text: t.clone(),
                    chords: Vec::new(),
                })
                .collect();
            let title_footer = if matches!(
                input.copyright_placement,
                CopyrightPlacement::First
                    | CopyrightPlacement::FirstAndLast
                    | CopyrightPlacement::All
            ) {
                safe_footer.as_str()
            } else {
                ""
            };
            let title_ctx = FitContext {
                footer_text: title_footer,
                reference_text: "",
                resolved: &resolved,
                is_title: true,
                ref_placement: None,
            };
            let units =
                fit_chunk_sequence(renderer, vec![(lyric_title_lines, Vec::new())], &title_ctx);
            for u in units {
                overflow_any |= u.overflows_safe_area;
                slides.push(GeneratedSlide {
                    section_id: None,
                    section_label: "Title".into(),
                    section_kind: SectionKind::Other as i32,
                    section_color: None,
                    section_hotkey: String::new(),
                    part_index: 1,
                    part_count: 1,
                    lines: u.lines,
                    translation_lines: Vec::new(),
                    footer_copyright: String::new(),
                    is_title: true,
                    is_blank: false,
                    applied_size: u.applied_size,
                });
                section_group_ids.push(None);
            }
        }

        for (sec_idx, section) in input.sections.iter().enumerate() {
            if section.lines.is_empty() {
                slides.push(GeneratedSlide {
                    section_id: section.section_id.clone(),
                    section_label: section.section_label.clone(),
                    section_kind: section.section_kind,
                    section_color: section.section_color,
                    section_hotkey: section.section_hotkey.clone(),
                    part_index: 1,
                    part_count: 1,
                    lines: Vec::new(),
                    translation_lines: Vec::new(),
                    footer_copyright: String::new(),
                    is_title: false,
                    is_blank: false,
                    applied_size: base_body_size,
                });
                section_group_ids.push(Some(sec_idx));
                continue;
            }

            let mut rule_chunks = Vec::new();
            let total_lines = section.lines.len();
            for start in (0..total_lines).step_by(eff_max_lines) {
                let end = (start + eff_max_lines).min(total_lines);
                let l_chunk = section.lines[start..end].to_vec();
                let t_chunk = if section.translation_lines.is_empty() {
                    Vec::new()
                } else {
                    (start..end)
                        .map(|idx| {
                            section
                                .translation_lines
                                .get(idx)
                                .cloned()
                                .unwrap_or_default()
                        })
                        .collect()
                };
                rule_chunks.push((l_chunk, t_chunk));
            }

            let footer_for_fit = if always_reserve_footer {
                safe_footer.as_str()
            } else {
                ""
            };
            let body_ctx = FitContext {
                footer_text: footer_for_fit,
                reference_text: "",
                resolved: &resolved,
                is_title: false,
                ref_placement: None,
            };
            let units = fit_chunk_sequence(renderer, rule_chunks, &body_ctx);
            let unit_count = units.len() as u32;
            for (u_idx, u) in units.into_iter().enumerate() {
                overflow_any |= u.overflows_safe_area;
                slides.push(GeneratedSlide {
                    section_id: section.section_id.clone(),
                    section_label: section.section_label.clone(),
                    section_kind: section.section_kind,
                    section_color: section.section_color,
                    section_hotkey: section.section_hotkey.clone(),
                    part_index: (u_idx + 1) as u32,
                    part_count: unit_count,
                    lines: u.lines,
                    translation_lines: u.translation_lines,
                    footer_copyright: String::new(),
                    is_title: false,
                    is_blank: false,
                    applied_size: u.applied_size,
                });
                section_group_ids.push(Some(sec_idx));
            }
        }

        let post_overflow = apply_copyright_with_fit_guard(
            renderer,
            &mut slides,
            &mut section_group_ids,
            &safe_footer,
            input.copyright_placement,
            &resolved,
        );
        overflow_any |= post_overflow;
        recount_section_parts_by_group(&mut slides, &section_group_ids);

        if input.blank_slide_at_end {
            slides.push(GeneratedSlide {
                section_id: None,
                section_label: "Blank".into(),
                section_kind: SectionKind::Other as i32,
                section_color: None,
                section_hotkey: String::new(),
                part_index: 1,
                part_count: 1,
                lines: Vec::new(),
                translation_lines: Vec::new(),
                footer_copyright: String::new(),
                is_title: false,
                is_blank: true,
                applied_size: base_body_size,
            });
        }

        let min_applied_body = slides
            .iter()
            .filter(|s| {
                !s.is_blank
                    && !s.is_title
                    && (!s.lines.is_empty() || !s.translation_lines.is_empty())
            })
            .map(|s| s.applied_size)
            .reduce(f32::min)
            .unwrap_or_else(|| {
                slides
                    .iter()
                    .filter(|s| !s.is_blank)
                    .map(|s| s.applied_size)
                    .reduce(f32::min)
                    .unwrap_or(base_body_size)
            });

        let final_slide_count = slides.len() as u32;
        let extra_splits = final_slide_count.saturating_sub(original_slide_count);
        let mut warning_codes = Vec::new();
        if overflow_any {
            warning_codes.push(WARN_OVERFLOWS_SAFE_AREA.to_owned());
        }
        if min_applied_body + 0.25 < base_body_size {
            warning_codes.push(WARN_SHRUNK_TO_FIT.to_owned());
        }
        if (min_applied_body - min_body_size).abs() < 0.5
            && min_applied_body + 0.25 < base_body_size
        {
            warning_codes.push(WARN_AT_MIN_SIZE.to_owned());
        }
        if extra_splits > 0 {
            warning_codes.push(WARN_EXTRA_SPLIT.to_owned());
        }

        let mut report = OutputFitReport {
            output_index: target.output_index,
            output_name: target.output_name.clone(),
            resolution: Some(Resolution {
                width: target.width,
                height: target.height,
            }),
            matched_variant: resolved.matched_variant.clone().unwrap_or_default(),
            fits: !overflow_any,
            applied_size: min_applied_body,
            min_size: min_body_size,
            original_slide_count,
            final_slide_count,
            extra_splits,
            overflows_safe_area: overflow_any,
            warning_codes,
            summary: String::new(),
            flash_warning: false,
        };
        report.summary = format_summary(&report);
        per_output.push((report, slides));
    }

    let primary_slides = per_output
        .first()
        .map(|(_, s)| s.clone())
        .unwrap_or_default();

    Ok(SongFitResult {
        primary_slides,
        per_output,
    })
}

pub fn fit_song_slides(input: &SongFitInput, targets: &[FitTarget]) -> Result<SongFitResult> {
    with_bundled_fit_renderer(|r| fit_song_slides_with_renderer(r, input, targets))
}

fn format_verse_span_label(verses: &[&ScriptureVerse], translation: &str) -> String {
    if verses.is_empty() {
        return String::new();
    }
    let mut segments = Vec::new();
    let mut i = 0usize;
    while i < verses.len() {
        let start = verses[i];
        let mut end = i;
        while end + 1 < verses.len()
            && verses[end + 1].book_num == start.book_num
            && verses[end + 1].chapter == verses[end].chapter
            && verses[end + 1].verse <= verses[end].verse + 1
        {
            end += 1;
        }
        let end_v = verses[end];
        let span = if start.chapter == end_v.chapter && start.verse == end_v.verse {
            format!("{} {}:{}", start.book_name, start.chapter, start.verse)
        } else if start.chapter == end_v.chapter {
            format!(
                "{} {}:{}-{}",
                start.book_name, start.chapter, start.verse, end_v.verse
            )
        } else {
            format!(
                "{} {}:{}-{}:{}",
                start.book_name, start.chapter, start.verse, end_v.chapter, end_v.verse
            )
        };
        segments.push(span);
        i = end + 1;
    }
    let joined = segments.join("; ");
    let t = translation.trim();
    if t.is_empty() {
        joined
    } else {
        format!("{joined} ({t})")
    }
}

fn split_reference_tuple(placement: ReferencePlacement, ref_label: &str) -> (String, String) {
    match placement {
        ReferencePlacement::None => (String::new(), String::new()),
        ReferencePlacement::SlideHeader => (ref_label.to_owned(), String::new()),
        ReferencePlacement::SlideFooter | ReferencePlacement::Unspecified => {
            (String::new(), ref_label.to_owned())
        }
    }
}

/// Fit scripture passages across all target outputs using `renderer` (M3.3 / M3-A2).
pub fn fit_scripture_slides_with_renderer(
    renderer: &mut TextRenderer,
    input: &ScriptureFitInput,
    targets: &[FitTarget],
) -> Result<ScriptureFitResult> {
    let default_target;
    let active_targets = if targets.is_empty() {
        default_target = vec![FitTarget {
            output_index: 0,
            output_name: "Main Output".into(),
            width: 1920,
            height: 1080,
            style: TemplateStyle::default(),
            max_lines_override: None,
        }];
        default_target.as_slice()
    } else {
        targets
    };

    let req_max_lines = if input.max_lines == 0 {
        4
    } else {
        input.max_lines.clamp(1, 100)
    };
    let verse_cap = if input.one_verse_per_slide {
        Some(1usize)
    } else {
        input.verses_per_slide
    };

    // Baseline slide count estimation matches M2 char-width packing so `extra_splits` reflects
    // true additional splits required by narrow or LED outputs.
    let baseline_body_count = estimate_baseline_scripture_slide_count(
        &input.verses,
        input.show_verse_numbers,
        req_max_lines as usize,
        input.baseline_chars_per_line.max(16),
        verse_cap,
    );
    let baseline_extra = usize::from(input.title_slide) + usize::from(input.blank_slide_at_end);
    let original_slide_count = (baseline_body_count + baseline_extra) as u32;

    let mut per_output = Vec::with_capacity(active_targets.len());

    for target in active_targets {
        let mut resolved =
            target
                .style
                .resolve_for_output(target.width, target.height, target.max_lines_override);
        match input.reference_placement {
            ReferencePlacement::SlideHeader => resolved.roles.reference.align_v = TextAlignV::Top,
            ReferencePlacement::SlideFooter => {
                resolved.roles.reference.align_v = TextAlignV::Bottom
            }
            _ => {}
        }
        let eff_max_lines = target
            .max_lines_override
            .or_else(|| {
                resolved.max_lines.map(|vm| {
                    if input.max_lines > 0 {
                        req_max_lines.min(vm)
                    } else {
                        vm
                    }
                })
            })
            .unwrap_or(req_max_lines)
            .clamp(1, 100);
        // Set `body.max_lines` only if template variant or target requested a visual line cap.
        let mut resolved_for_fit = resolved.clone();
        if resolved_for_fit.roles.body.max_lines.is_none() && resolved_for_fit.max_lines.is_some() {
            resolved_for_fit.roles.body.max_lines = Some(eff_max_lines);
        }

        let base_body_size = resolved_for_fit.roles.body.size.max(8.0);
        let min_body_size = resolved_for_fit
            .roles
            .body
            .min_size
            .clamp(8.0, base_body_size);

        let mut slides = Vec::new();
        let mut overflow_any = false;

        if input.title_slide {
            let mut title_lines = Vec::new();
            if !input.passage_label.is_empty() {
                title_lines.push(LyricLine {
                    text: input.passage_label.clone(),
                    chords: Vec::new(),
                });
            }
            if !input.translation_name.is_empty() {
                title_lines.push(LyricLine {
                    text: input.translation_name.clone(),
                    chords: Vec::new(),
                });
            }
            let sc_title_ctx = FitContext {
                footer_text: "",
                reference_text: "",
                resolved: &resolved_for_fit,
                is_title: true,
                ref_placement: None,
            };
            let units =
                fit_chunk_sequence(renderer, vec![(title_lines, Vec::new())], &sc_title_ctx);
            for u in units {
                overflow_any |= u.overflows_safe_area;
                slides.push(GeneratedScriptureSlide {
                    reference: input.passage_label.clone(),
                    part_index: 1,
                    part_count: 1,
                    lines: u.lines.into_iter().map(|l| l.text).collect(),
                    header_reference: String::new(),
                    footer_reference: String::new(),
                    is_title: true,
                    is_blank: false,
                    applied_size: u.applied_size,
                });
            }
        }

        // Pack verses into slides greedily using Skia measurements at `base_body_size` first,
        // then shrink / re-split if a verse or group overflows.
        let mut content_slides: Vec<GeneratedScriptureSlide> = Vec::new();
        let mut current_lines: Vec<LyricLine> = Vec::new();
        let mut current_verses: Vec<&ScriptureVerse> = Vec::new();

        for verse in &input.verses {
            let display = if input.show_verse_numbers {
                format!("{} {}", verse.verse, verse.text.trim())
            } else {
                verse.text.trim().to_owned()
            };
            if display.trim().is_empty() {
                continue;
            }
            let v_line = LyricLine {
                text: display,
                chords: Vec::new(),
            };

            if !current_lines.is_empty() && verse_cap.is_none_or(|cap| current_verses.len() < cap) {
                let mut trial_lines = current_lines.clone();
                trial_lines.push(v_line.clone());
                let mut trial_verses = current_verses.clone();
                trial_verses.push(verse);
                let ref_label = format_verse_span_label(&trial_verses, &input.translation_code);
                let ref_text = if input.reference_placement == ReferencePlacement::None {
                    ""
                } else {
                    ref_label.as_str()
                };
                let trial_ctx = FitContext {
                    footer_text: "",
                    reference_text: ref_text,
                    resolved: &resolved_for_fit,
                    is_title: false,
                    ref_placement: Some(input.reference_placement),
                };
                let chk = check_chunk_fits(renderer, &trial_lines, &[], &trial_ctx, base_body_size);
                if chk.fits {
                    current_lines = trial_lines;
                    current_verses = trial_verses;
                    continue;
                }
            }

            if !current_lines.is_empty() {
                let ref_label = format_verse_span_label(&current_verses, &input.translation_code);
                let (h_ref, f_ref) = split_reference_tuple(input.reference_placement, &ref_label);
                let ref_text = if input.reference_placement == ReferencePlacement::None {
                    ""
                } else {
                    ref_label.as_str()
                };
                let body_ctx = FitContext {
                    footer_text: "",
                    reference_text: ref_text,
                    resolved: &resolved_for_fit,
                    is_title: false,
                    ref_placement: Some(input.reference_placement),
                };
                let units = fit_chunk_sequence(
                    renderer,
                    vec![(std::mem::take(&mut current_lines), Vec::new())],
                    &body_ctx,
                );
                current_verses.clear();
                for u in units {
                    overflow_any |= u.overflows_safe_area;
                    content_slides.push(GeneratedScriptureSlide {
                        reference: ref_label.clone(),
                        part_index: 1,
                        part_count: 1,
                        lines: u.lines.into_iter().map(|l| l.text).collect(),
                        header_reference: h_ref.clone(),
                        footer_reference: f_ref.clone(),
                        is_title: false,
                        is_blank: false,
                        applied_size: u.applied_size,
                    });
                }
            }

            current_lines.push(v_line);
            current_verses.push(verse);

            if verse_cap == Some(1) {
                let ref_label = format_verse_span_label(&current_verses, &input.translation_code);
                let (h_ref, f_ref) = split_reference_tuple(input.reference_placement, &ref_label);
                let ref_text = if input.reference_placement == ReferencePlacement::None {
                    ""
                } else {
                    ref_label.as_str()
                };
                let body_ctx = FitContext {
                    footer_text: "",
                    reference_text: ref_text,
                    resolved: &resolved_for_fit,
                    is_title: false,
                    ref_placement: Some(input.reference_placement),
                };
                let units = fit_chunk_sequence(
                    renderer,
                    vec![(std::mem::take(&mut current_lines), Vec::new())],
                    &body_ctx,
                );
                current_verses.clear();
                for u in units {
                    overflow_any |= u.overflows_safe_area;
                    content_slides.push(GeneratedScriptureSlide {
                        reference: ref_label.clone(),
                        part_index: 1,
                        part_count: 1,
                        lines: u.lines.into_iter().map(|l| l.text).collect(),
                        header_reference: h_ref.clone(),
                        footer_reference: f_ref.clone(),
                        is_title: false,
                        is_blank: false,
                        applied_size: u.applied_size,
                    });
                }
            }
        }

        if !current_lines.is_empty() {
            let ref_label = format_verse_span_label(&current_verses, &input.translation_code);
            let (h_ref, f_ref) = split_reference_tuple(input.reference_placement, &ref_label);
            let ref_text = if input.reference_placement == ReferencePlacement::None {
                ""
            } else {
                ref_label.as_str()
            };
            let body_ctx = FitContext {
                footer_text: "",
                reference_text: ref_text,
                resolved: &resolved_for_fit,
                is_title: false,
                ref_placement: Some(input.reference_placement),
            };
            let units = fit_chunk_sequence(renderer, vec![(current_lines, Vec::new())], &body_ctx);
            for u in units {
                overflow_any |= u.overflows_safe_area;
                content_slides.push(GeneratedScriptureSlide {
                    reference: ref_label.clone(),
                    part_index: 1,
                    part_count: 1,
                    lines: u.lines.into_iter().map(|l| l.text).collect(),
                    header_reference: h_ref.clone(),
                    footer_reference: f_ref.clone(),
                    is_title: false,
                    is_blank: false,
                    applied_size: u.applied_size,
                });
            }
        }

        let content_total = content_slides.len() as u32;
        for (idx, s) in content_slides.iter_mut().enumerate() {
            s.part_index = (idx + 1) as u32;
            s.part_count = content_total;
        }
        slides.extend(content_slides);

        if input.blank_slide_at_end {
            slides.push(GeneratedScriptureSlide {
                reference: String::new(),
                part_index: 1,
                part_count: 1,
                lines: Vec::new(),
                header_reference: String::new(),
                footer_reference: String::new(),
                is_title: false,
                is_blank: true,
                applied_size: base_body_size,
            });
        }

        let min_applied_body = slides
            .iter()
            .filter(|s| !s.is_blank && !s.is_title && !s.lines.is_empty())
            .map(|s| s.applied_size)
            .reduce(f32::min)
            .unwrap_or(base_body_size);

        let final_slide_count = slides.len() as u32;
        let extra_splits = final_slide_count.saturating_sub(original_slide_count);
        let mut warning_codes = Vec::new();
        if overflow_any {
            warning_codes.push(WARN_OVERFLOWS_SAFE_AREA.to_owned());
        }
        if min_applied_body + 0.25 < base_body_size {
            warning_codes.push(WARN_SHRUNK_TO_FIT.to_owned());
        }
        if (min_applied_body - min_body_size).abs() < 0.5
            && min_applied_body + 0.25 < base_body_size
        {
            warning_codes.push(WARN_AT_MIN_SIZE.to_owned());
        }
        if extra_splits > 0 {
            warning_codes.push(WARN_EXTRA_SPLIT.to_owned());
        }

        let mut report = OutputFitReport {
            output_index: target.output_index,
            output_name: target.output_name.clone(),
            resolution: Some(Resolution {
                width: target.width,
                height: target.height,
            }),
            matched_variant: resolved.matched_variant.clone().unwrap_or_default(),
            fits: !overflow_any,
            applied_size: min_applied_body,
            min_size: min_body_size,
            original_slide_count,
            final_slide_count,
            extra_splits,
            overflows_safe_area: overflow_any,
            warning_codes,
            summary: String::new(),
            flash_warning: false,
        };
        report.summary = format_summary(&report);
        per_output.push((report, slides));
    }

    let primary_slides = per_output
        .first()
        .map(|(_, s)| s.clone())
        .unwrap_or_default();

    Ok(ScriptureFitResult {
        primary_slides,
        per_output,
    })
}

pub fn fit_scripture_slides(
    input: &ScriptureFitInput,
    targets: &[FitTarget],
) -> Result<ScriptureFitResult> {
    with_bundled_fit_renderer(|r| fit_scripture_slides_with_renderer(r, input, targets))
}

fn estimate_baseline_scripture_slide_count(
    verses: &[ScriptureVerse],
    show_numbers: bool,
    max_lines: usize,
    max_chars: usize,
    verse_cap: Option<usize>,
) -> usize {
    let mut slide_count = 0usize;
    let mut cur_lines = 0usize;
    let mut cur_verses = 0usize;
    for v in verses {
        let len = v.text.trim().chars().count()
            + if show_numbers {
                v.verse.to_string().len() + 1
            } else {
                0
            };
        if len == 0 {
            continue;
        }
        let v_lines = len.div_ceil(max_chars).max(1);
        let fits_cur = cur_lines > 0
            && cur_lines + v_lines <= max_lines
            && verse_cap.is_none_or(|c| cur_verses < c);
        if fits_cur {
            cur_lines += v_lines;
            cur_verses += 1;
        } else {
            if cur_lines > 0 {
                slide_count += 1;
            }
            if v_lines <= max_lines {
                cur_lines = v_lines;
                cur_verses = 1;
                if verse_cap == Some(1) {
                    slide_count += 1;
                    cur_lines = 0;
                    cur_verses = 0;
                }
            } else {
                slide_count += v_lines.div_ceil(max_lines);
                cur_lines = 0;
                cur_verses = 0;
            }
        }
    }
    if cur_lines > 0 {
        slide_count += 1;
    }
    slide_count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_song(lines: Vec<&str>) -> SongFitInput {
        SongFitInput {
            sections: vec![SongSectionInput {
                section_id: None,
                section_label: "Verse 1".into(),
                section_kind: SectionKind::Verse as i32,
                section_color: None,
                section_hotkey: "1".into(),
                lines: lines
                    .into_iter()
                    .map(|t| LyricLine {
                        text: t.into(),
                        chords: Vec::new(),
                    })
                    .collect(),
                translation_lines: Vec::new(),
            }],
            title_lines: None,
            copyright_line: String::new(),
            copyright_placement: CopyrightPlacement::None,
            blank_slide_at_end: false,
            max_lines: 4,
        }
    }

    #[test]
    fn long_lines_shrink_before_splitting_and_extreme_lines_resplit_above_min_size() {
        let mut renderer = TextRenderer::bundled().unwrap();
        let tmpl = TemplateStyle::preset_sanctuary();
        let target = FitTarget {
            output_index: 0,
            output_name: "Main 16:9".into(),
            width: 1920,
            height: 1080,
            style: tmpl.clone(),
            max_lines_override: None,
        };

        // 4 moderately long lines that wrap slightly at 74px (>4 visual lines), so shrinking
        // below 74px (>= min_size 32px) fits cleanly on 1 slide without extra split!
        let medium = test_song(vec![
            "Amazing grace how sweet the sound that saved a wretch like me today",
            "I once was lost but now am found was blind but now I clearly see",
            "Through many dangers toils and snares I have already safely come",
            "Tis grace hath brought me safe thus far and grace will lead me home",
        ]);
        let medium_res =
            fit_song_slides_with_renderer(&mut renderer, &medium, std::slice::from_ref(&target))
                .unwrap();
        let (rep_med, slides_med) = &medium_res.per_output[0];
        assert!(rep_med.fits);
        assert_eq!(slides_med.len(), 1);
        assert_eq!(rep_med.extra_splits, 0);
        assert!(rep_med.applied_size < 74.0 && rep_med.applied_size >= rep_med.min_size);

        // Extremely verbose 4 lines: cannot fit in 4 visual lines even at min_size=32 -> must resplit!
        let huge_line = "Grace and mercy and peace be multiplied unto every congregation gathered across the nations of the earth forevermore";
        let huge = test_song(vec![huge_line, huge_line, huge_line, huge_line]);
        let huge_res = fit_song_slides_with_renderer(&mut renderer, &huge, &[target]).unwrap();
        let (rep_huge, slides_huge) = &huge_res.per_output[0];
        assert!(rep_huge.fits);
        assert!(slides_huge.len() > 1);
        assert!(rep_huge.extra_splits >= 1);
        assert!(rep_huge.applied_size >= rep_huge.min_size);
        assert!(!rep_huge.overflows_safe_area);
    }

    #[test]
    fn impossible_tiny_resolution_reports_safe_area_overflow_honestly() {
        let mut renderer = TextRenderer::bundled().unwrap();
        let tmpl = TemplateStyle {
            reference_height: 64,
            roles: crate::template::RoleSlots {
                body: SlotTextStyle {
                    size: 96.0,
                    min_size: 80.0,
                    ..SlotTextStyle::default()
                },
                ..Default::default()
            },
            ..TemplateStyle::default()
        };
        let tiny_target = FitTarget {
            output_index: 0,
            output_name: "Tiny".into(),
            width: 320,
            height: 64,
            style: tmpl,
            max_lines_override: None,
        };
        let input = test_song(vec!["Amazing grace"]);
        let res = fit_song_slides_with_renderer(&mut renderer, &input, &[tiny_target]).unwrap();
        let (report, _) = &res.per_output[0];
        assert!(!report.fits);
        assert!(report.overflows_safe_area);
        assert!(
            report
                .warning_codes
                .iter()
                .any(|c| c == WARN_OVERFLOWS_SAFE_AREA)
        );
    }

    #[test]
    fn fit_scripture_slides_and_rendered_pixels_stay_inside_safe_area() {
        let mut renderer = TextRenderer::bundled().unwrap();
        let tmpl = TemplateStyle::preset_sanctuary();
        let targets = vec![
            FitTarget {
                output_index: 0,
                output_name: "Hall 16:9".into(),
                width: 960,
                height: 540,
                style: tmpl.clone(),
                max_lines_override: None,
            },
            FitTarget {
                output_index: 1,
                output_name: "LED 32:9".into(),
                width: 1280,
                height: 360,
                style: tmpl.clone(),
                max_lines_override: None,
            },
        ];

        let scripture = ScriptureFitInput {
            translation_code: "KJV".into(),
            translation_name: "King James Version".into(),
            passage_label: "Psalm 119:1-2 (KJV)".into(),
            verses: vec![
                ScriptureVerse {
                    translation: "KJV".into(),
                    book_num: 19,
                    book_code: "PSA".into(),
                    book_name: "Psalms".into(),
                    chapter: 119,
                    verse: 1,
                    text: "Blessed are the undefiled in the way, who walk in the law of the LORD throughout every generation of their lives.".into(),
                },
                ScriptureVerse {
                    translation: "KJV".into(),
                    book_num: 19,
                    book_code: "PSA".into(),
                    book_name: "Psalms".into(),
                    chapter: 119,
                    verse: 2,
                    text: "Blessed are they that keep his testimonies, and that seek him with the whole heart always.".into(),
                },
            ],
            show_verse_numbers: true,
            verses_per_slide: None,
            one_verse_per_slide: false,
            reference_placement: ReferencePlacement::SlideFooter,
            title_slide: true,
            blank_slide_at_end: true,
            max_lines: 4,
            baseline_chars_per_line: 48,
        };
        let fitted =
            fit_scripture_slides_with_renderer(&mut renderer, &scripture, &targets).unwrap();
        assert_eq!(fitted.per_output.len(), 2);
        for (target, (rep, slides)) in targets.iter().zip(&fitted.per_output) {
            assert!(rep.fits, "scripture failed to fit on {}", rep.output_name);
            assert!(rep.applied_size >= rep.min_size);
            let resolved = target
                .style
                .resolve_for_output(target.width, target.height, None);
            let safe = resolved
                .safe_area
                .resolved_rect(target.width, target.height);
            for s in slides {
                let scaled = scale_resolved_for_slide(
                    &resolved,
                    s.applied_size,
                    s.is_title,
                    Some(ReferencePlacement::SlideFooter),
                );
                let layer = generated_scripture_slide_to_layer(s, None);
                let frame = renderer
                    .render_slide(&layer, &scaled, target.width, target.height)
                    .unwrap();
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
                                "pixel ({x},{y}) alpha={} escaped safe area {safe:?} on {} ({}x{})",
                                px[3],
                                target.output_name,
                                target.width,
                                target.height
                            );
                        }
                    }
                }
            }
        }
    }
}
