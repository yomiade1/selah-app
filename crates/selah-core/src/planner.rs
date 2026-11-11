//! Service planner domain logic (M4.1):
//! - "Inherit from last week" (`CreateServiceFromPrevious`)
//! - Service runtime projection (`ProjectServiceRuntime`)
//! - Printable HTML & PDF export for service order and chord charts (`ExportServicePrint`)

use selah_proto::v1::{
    CreateServiceFromPrevious, ExportServicePrint, MediaItem, ProjectServiceRuntime,
    ProjectedServiceItem, Service, ServiceItem, ServiceItemKind, ServicePrintExport,
    ServicePrintKind, ServiceRuntimeProjection, ServiceType, Song, Team, Timestamp, reply,
};

use crate::library::{
    CommandBusError, CommandMeta, ExecutionOutcome, Library, exchange, new_id, validate_id,
};
use crate::songs::{parse_key, resolve_arrangement_sections, transpose_song};

const SEVEN_DAYS_MS: i64 = 7 * 86_400 * 1_000;
const DEFAULT_START_SECONDS_OF_DAY: u32 = 9 * 3600; // 09:00

pub fn format_hh_mm(seconds_of_day: u32) -> String {
    let wrapped = seconds_of_day.rem_euclid(86_400);
    let hours = wrapped / 3600;
    let minutes = (wrapped % 3600) / 60;
    format!("{hours:02}:{minutes:02}")
}

pub fn format_mm_ss(duration_seconds: u32) -> String {
    let mins = duration_seconds / 60;
    let secs = duration_seconds % 60;
    format!("{mins}:{secs:02}")
}

pub fn format_total_duration(total_seconds: u32) -> String {
    let total_mins = total_seconds.div_ceil(60);
    let hours = total_mins / 60;
    let mins = total_mins % 60;
    if hours > 0 && mins > 0 {
        format!("{hours} hr {mins} min")
    } else if hours > 0 {
        format!("{hours} hr")
    } else {
        format!("{mins} min")
    }
}

fn parse_hh_mm_seconds(raw: &str) -> Option<u32> {
    let trimmed = raw.trim();
    let (h_str, m_str) = trimmed.split_once(':')?;
    let h: u32 = h_str.trim().parse().ok()?;
    let m: u32 = m_str
        .trim()
        .chars()
        .take(2)
        .collect::<String>()
        .parse()
        .ok()?;
    if h < 24 && m < 60 {
        Some(h * 3600 + m * 60)
    } else {
        None
    }
}

fn start_seconds_of_day(service: &Service, service_type: Option<&ServiceType>) -> u32 {
    if let Some(ts) = service.starts_at.as_ref()
        && ts.unix_milliseconds > 0
    {
        let secs = (ts.unix_milliseconds / 1000).rem_euclid(86_400) as u32;
        return secs;
    }
    if let Some(st) = service_type
        && let Some(secs) = parse_hh_mm_seconds(&st.default_start_time)
    {
        return secs;
    }
    DEFAULT_START_SECONDS_OF_DAY
}

fn infer_item_duration_seconds(library: &Library, item: &ServiceItem) -> u32 {
    let kind = ServiceItemKind::try_from(item.kind).unwrap_or(ServiceItemKind::Unspecified);
    if kind == ServiceItemKind::Header {
        return 0;
    }
    if item.duration_seconds > 0 {
        return item.duration_seconds;
    }
    match kind {
        ServiceItemKind::Countdown => {
            if item.countdown_seconds > 0 {
                item.countdown_seconds
            } else {
                300
            }
        }
        ServiceItemKind::Media | ServiceItemKind::Audio => {
            if let Some(cid) = item.content_id.as_ref()
                && let Ok(Some(media)) = library.get::<MediaItem>(cid)
                && media.duration_milliseconds > 0
            {
                return (media.duration_milliseconds / 1000).max(1) as u32;
            }
            180
        }
        ServiceItemKind::Song => 240,
        ServiceItemKind::Scripture => 180,
        ServiceItemKind::Presentation => 300,
        ServiceItemKind::Note => 120,
        ServiceItemKind::Header | ServiceItemKind::Unspecified => 0,
    }
}

/// Compute item-by-item start/end times and projected service end time.
pub fn project_service_runtime(
    library: &Library,
    req: &ProjectServiceRuntime,
) -> Result<ServiceRuntimeProjection, CommandBusError> {
    let service = if let Some(inline) = req.service.as_ref() {
        inline.clone()
    } else if let Some(id) = req.service_id.as_ref() {
        validate_id(id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        library
            .get::<Service>(id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("service not found".into()))?
    } else {
        return Err(CommandBusError::Invalid(
            "missing service or service_id".into(),
        ));
    };

    let service_type = service
        .service_type_id
        .as_ref()
        .and_then(|stid| library.get::<ServiceType>(stid).ok().flatten());

    let base_display_secs = start_seconds_of_day(&service, service_type.as_ref());
    let base_unix_ms = service
        .starts_at
        .as_ref()
        .map(|t| t.unix_milliseconds)
        .filter(|&ms| ms > 0)
        .unwrap_or(i64::from(base_display_secs) * 1000);

    // If the first item is a pre-service Countdown (before the first Header or Song),
    // check whether it precedes the service start or starts at base_display_secs.
    // Standard planner behavior: items accumulate sequentially from the service start time;
    // headers have 0 duration and display "—".
    let mut running_offset_seconds = 0u32;
    let mut projected_items = Vec::with_capacity(service.items.len());

    for (idx, item) in service.items.iter().enumerate() {
        let is_header = item.kind == ServiceItemKind::Header as i32;
        let dur = infer_item_duration_seconds(library, item);
        let item_start_ms = base_unix_ms + i64::from(running_offset_seconds) * 1000;
        let item_end_ms = item_start_ms + i64::from(dur) * 1000;
        let display_secs = base_display_secs + running_offset_seconds;

        projected_items.push(ProjectedServiceItem {
            item_id: item.id.clone(),
            index: idx as u32,
            offset_seconds: running_offset_seconds,
            duration_seconds: dur,
            starts_at: Some(Timestamp {
                unix_milliseconds: item_start_ms,
            }),
            ends_at: Some(Timestamp {
                unix_milliseconds: item_end_ms,
            }),
            formatted_time: if is_header {
                "—".into()
            } else {
                format_hh_mm(display_secs)
            },
            formatted_duration: if is_header {
                "—".into()
            } else {
                format_mm_ss(dur)
            },
            is_header,
        });

        running_offset_seconds = running_offset_seconds.saturating_add(dur);
    }

    let projected_ends_at = Timestamp {
        unix_milliseconds: base_unix_ms + i64::from(running_offset_seconds) * 1000,
    };

    Ok(ServiceRuntimeProjection {
        items: projected_items,
        total_duration_seconds: running_offset_seconds,
        projected_ends_at: Some(projected_ends_at),
        formatted_start_time: format_hh_mm(base_display_secs),
        formatted_end_time: format_hh_mm(base_display_secs + running_offset_seconds),
        formatted_total_duration: format_total_duration(running_offset_seconds),
    })
}

/// Create a new service by inheriting structure, items, looks/templates, and team from last week
/// (or from a `ServiceType` template if no previous service is specified).
pub fn create_service_from_previous_op(
    library: &mut Library,
    req: &CreateServiceFromPrevious,
    meta: &CommandMeta<'_>,
) -> Result<ExecutionOutcome, CommandBusError> {
    let source_service: Option<Service> = if let Some(src_id) = req.source_service_id.as_ref() {
        validate_id(src_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        let found = library
            .get::<Service>(src_id)
            .map_err(CommandBusError::Internal)?
            .ok_or_else(|| CommandBusError::NotFound("source service not found".into()))?;
        Some(found)
    } else {
        // Find the most recent saved service (matching service_type_id if given).
        let existing = library
            .list::<Service>(200, 0)
            .map_err(CommandBusError::Internal)?;
        existing
            .into_iter()
            .filter(|s| {
                req.service_type_id
                    .as_ref()
                    .is_none_or(|stid| s.service_type_id.as_ref() == Some(stid))
            })
            .max_by_key(|s| s.starts_at.as_ref().map_or(0, |t| t.unix_milliseconds))
    };

    let service_type_id = req.service_type_id.clone().or_else(|| {
        source_service
            .as_ref()
            .and_then(|s| s.service_type_id.clone())
    });

    let service_type: Option<ServiceType> = if let Some(stid) = service_type_id.as_ref() {
        validate_id(stid).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
        library
            .get::<ServiceType>(stid)
            .map_err(CommandBusError::Internal)?
    } else {
        None
    };

    let template_items: &[ServiceItem] = if let Some(src) = source_service.as_ref() {
        &src.items
    } else if let Some(st) = service_type.as_ref() {
        &st.template_items
    } else {
        &[]
    };

    let mut inherited_items = Vec::with_capacity(template_items.len());
    for src_item in template_items {
        let mut cloned = src_item.clone();
        cloned.id = Some(new_id());
        let kind = ServiceItemKind::try_from(cloned.kind).unwrap_or(ServiceItemKind::Unspecified);
        if kind == ServiceItemKind::Song && !req.keep_songs && source_service.is_some() {
            // Preserve the song slot metadata (or keep content if keep_songs is true);
            // mark status as Ready if content is linked, else Draft.
            if cloned.status_tag.is_empty() {
                cloned.status_tag = if cloned.content_id.is_some() {
                    "Ready".into()
                } else {
                    "Draft".into()
                };
            }
        } else if cloned.status_tag.is_empty() && kind != ServiceItemKind::Header {
            cloned.status_tag = "Ready".into();
        }
        inherited_items.push(cloned);
    }

    let starts_at = req.starts_at.or_else(|| {
        source_service
            .as_ref()
            .and_then(|s| s.starts_at.as_ref())
            .map(|ts| Timestamp {
                unix_milliseconds: ts.unix_milliseconds + SEVEN_DAYS_MS,
            })
    });

    let name = if !req.name.trim().is_empty() {
        req.name.trim().to_owned()
    } else if let Some(src) = source_service.as_ref() {
        format!("{} (Next Week)", src.name)
    } else if let Some(st) = service_type.as_ref() {
        st.name.clone()
    } else {
        "Sunday Morning".into()
    };

    let series = if !req.series.trim().is_empty() {
        req.series.trim().to_owned()
    } else {
        source_service
            .as_ref()
            .map(|s| s.series.clone())
            .unwrap_or_default()
    };

    let location = source_service
        .as_ref()
        .map(|s| s.location.clone())
        .filter(|l| !l.is_empty())
        .or_else(|| service_type.as_ref().map(|st| st.default_location.clone()))
        .unwrap_or_default();

    let default_template_id = source_service
        .as_ref()
        .and_then(|s| s.default_template_id.clone())
        .or_else(|| {
            service_type
                .as_ref()
                .and_then(|st| st.default_template_id.clone())
        });

    let default_look_name = source_service
        .as_ref()
        .map(|s| s.default_look_name.clone())
        .filter(|l| !l.is_empty())
        .or_else(|| service_type.as_ref().map(|st| st.default_look_name.clone()))
        .unwrap_or_default();

    let planned_duration_seconds = source_service
        .as_ref()
        .map(|s| s.planned_duration_seconds)
        .filter(|&d| d > 0)
        .or_else(|| {
            service_type
                .as_ref()
                .map(|st| st.default_duration_seconds)
                .filter(|&d| d > 0)
        })
        .unwrap_or(4500);

    let new_service = Service {
        id: Some(new_id()),
        name,
        starts_at,
        service_type_id,
        items: inherited_items,
        team_ids: source_service
            .as_ref()
            .map(|s| s.team_ids.clone())
            .unwrap_or_default(),
        series,
        location,
        notes: source_service
            .as_ref()
            .map(|s| s.notes.clone())
            .unwrap_or_default(),
        source_service_id: source_service.and_then(|s| s.id),
        planned_duration_seconds,
        default_template_id,
        default_look_name,
    };

    library.save_document_with_reply_op(
        &new_service,
        meta,
        reply::Result::Service(new_service.clone()),
    )
}

fn html_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn pdf_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        let ascii = if ch.is_ascii() && !ch.is_ascii_control() {
            ch
        } else {
            '-'
        };
        match ascii {
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out
}

/// Build a line of positioned chords above a lyric line in fixed-width columns.
pub fn format_chord_over_lyric_pair(line: &selah_proto::v1::LyricLine) -> (Option<String>, String) {
    if line.chords.is_empty() {
        return (None, line.text.clone());
    }
    let mut sorted = line.chords.clone();
    sorted.sort_by_key(|c| c.position);
    let mut chord_row = String::new();
    let mut current_col = 0usize;
    for chord in &sorted {
        let target_col = chord.position as usize;
        if target_col > current_col {
            chord_row.extend(std::iter::repeat_n(' ', target_col - current_col));
            current_col = target_col;
        } else if !chord_row.is_empty() {
            chord_row.push(' ');
            current_col += 1;
        }
        chord_row.push_str(&chord.text);
        current_col += chord.text.chars().count();
    }
    (Some(chord_row), line.text.clone())
}

fn load_and_transpose_song(library: &Library, item: &ServiceItem) -> Option<Song> {
    let cid = item.content_id.as_ref()?;
    let mut song = library.get::<Song>(cid).ok().flatten()?;
    let target_key = item.song_key.trim();
    if !target_key.is_empty()
        && !target_key.eq_ignore_ascii_case(song.key.trim())
        && let (Some((from_semi, _, _)), Some((to_semi, _, prefer_flats))) =
            (parse_key(&song.key), parse_key(target_key))
    {
        let semitones = (i32::from(to_semi) - i32::from(from_semi)).rem_euclid(12);
        let _ = transpose_song(&mut song, semitones, Some(prefer_flats));
        song.key = target_key.to_owned();
    }
    Some(song)
}

fn item_kind_label(kind: i32) -> &'static str {
    match ServiceItemKind::try_from(kind).unwrap_or(ServiceItemKind::Unspecified) {
        ServiceItemKind::Song => "Song",
        ServiceItemKind::Scripture => "Scripture",
        ServiceItemKind::Media => "Media",
        ServiceItemKind::Presentation => "Presentation",
        ServiceItemKind::Note => "Note",
        ServiceItemKind::Countdown => "Countdown",
        ServiceItemKind::Header => "Header",
        ServiceItemKind::Audio => "Audio",
        ServiceItemKind::Unspecified => "Item",
    }
}

/// Build a valid multi-page PDF 1.4 document from structured page line streams.
/// Each page is a list of `(is_monospace, font_size, text)` lines.
fn build_pdf_bytes(pages: &[Vec<(bool, u32, String)>]) -> Vec<u8> {
    let page_list = if pages.is_empty() {
        vec![vec![(false, 12, "Empty service".to_owned())]]
    } else {
        pages.to_vec()
    };
    let num_pages = page_list.len();
    // Object layout:
    // 1: Catalog
    // 2: Pages
    // 3: Font F1 (Helvetica)
    // 4: Font F2 (Courier)
    // For page i in 0..num_pages:
    //   page_obj = 5 + 2*i
    //   content_obj = 6 + 2*i
    let total_objs = 4 + 2 * num_pages;
    let mut offsets = vec![0usize; total_objs + 1];
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n");

    let write_obj = |out: &mut Vec<u8>, offsets: &mut [usize], id: usize, body: &str| {
        offsets[id] = out.len();
        out.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
    };

    write_obj(
        &mut out,
        &mut offsets,
        1,
        "<< /Type /Catalog /Pages 2 0 R >>",
    );

    let kids: Vec<String> = (0..num_pages)
        .map(|i| format!("{} 0 R", 5 + 2 * i))
        .collect();
    write_obj(
        &mut out,
        &mut offsets,
        2,
        &format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            num_pages
        ),
    );
    write_obj(
        &mut out,
        &mut offsets,
        3,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    );
    write_obj(
        &mut out,
        &mut offsets,
        4,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>",
    );

    for (i, lines) in page_list.iter().enumerate() {
        let page_obj = 5 + 2 * i;
        let content_obj = 6 + 2 * i;
        let mut stream = String::from("BT\n");
        let mut y = 750i32;
        for (mono, size, text) in lines {
            if y < 48 {
                break;
            }
            let font = if *mono { "/F2" } else { "/F1" };
            let escaped = pdf_escape(text);
            stream.push_str(&format!(
                "{font} {size} Tf\n1 0 0 1 48 {y} Tm\n({escaped}) Tj\n"
            ));
            y -= (*size as i32) + 5;
        }
        stream.push_str("ET\n");
        let page_body = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {content_obj} 0 R >>"
        );
        write_obj(&mut out, &mut offsets, page_obj, &page_body);
        let content_body = format!(
            "<< /Length {} >>\nstream\n{}endstream",
            stream.len(),
            stream
        );
        write_obj(&mut out, &mut offsets, content_obj, &content_body);
    }

    let xref_offset = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", total_objs + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
            total_objs + 1
        )
        .as_bytes(),
    );
    out
}

/// Export a printable service order and/or transposed chord charts as HTML and PDF.
pub fn export_service_print(
    library: &Library,
    req: &ExportServicePrint,
) -> Result<ServicePrintExport, CommandBusError> {
    let Some(service_id) = req.service_id.as_ref() else {
        return Err(CommandBusError::Invalid("missing service_id".into()));
    };
    validate_id(service_id).map_err(|e| CommandBusError::Invalid(e.to_string()))?;
    let service = library
        .get::<Service>(service_id)
        .map_err(CommandBusError::Internal)?
        .ok_or_else(|| CommandBusError::NotFound("service not found".into()))?;

    let kind = match ServicePrintKind::try_from(req.kind) {
        Ok(ServicePrintKind::Unspecified) | Ok(ServicePrintKind::ServiceOrder) => {
            ServicePrintKind::ServiceOrder
        }
        Ok(ServicePrintKind::ChordCharts) => ServicePrintKind::ChordCharts,
        Ok(ServicePrintKind::Both) => ServicePrintKind::Both,
        Err(_) => {
            return Err(CommandBusError::Invalid(
                "unknown service print kind".into(),
            ));
        }
    };

    let projection = project_service_runtime(
        library,
        &ProjectServiceRuntime {
            service_id: Some(service_id.clone()),
            service: Some(service.clone()),
        },
    )?;

    let mut team_members = Vec::new();
    for tid in &service.team_ids {
        if let Ok(Some(team)) = library.get::<Team>(tid) {
            for member in team.members {
                team_members.push(format!("{} ({})", member.name, member.role));
            }
        }
    }

    let include_order = matches!(
        kind,
        ServicePrintKind::ServiceOrder | ServicePrintKind::Both
    );
    let include_charts = matches!(kind, ServicePrintKind::ChordCharts | ServicePrintKind::Both);

    let mut html = String::from(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>",
    );
    html.push_str(&html_escape(&service.name));
    html.push_str(
        "</title>\n<style>\n\
        body { font-family: Inter, -apple-system, sans-serif; color: #1b1b1b; margin: 24px; }\n\
        h1 { font-size: 24px; margin: 0 0 4px 0; }\n\
        .meta { color: #5c5c5c; font-size: 13px; margin-bottom: 16px; }\n\
        table { width: 100%; border-collapse: collapse; margin-bottom: 24px; }\n\
        th, td { border-bottom: 1px solid #dfe1e2; text-align: left; padding: 8px; font-size: 13px; }\n\
        th { background: #f0f0f0; font-size: 11px; text-transform: uppercase; }\n\
        .header-row td { background: #f5f5f7; font-weight: 700; text-transform: uppercase; font-size: 11px; }\n\
        .chart-page { page-break-before: always; margin-top: 24px; }\n\
        .chord-block { font-family: 'JetBrains Mono', monospace; white-space: pre; line-height: 1.35; margin: 8px 0; }\n\
        .chord-line { color: #005ea2; font-weight: 700; }\n\
        </style>\n</head>\n<body>\n",
    );

    let mut pdf_pages: Vec<Vec<(bool, u32, String)>> = Vec::new();

    if include_order {
        html.push_str(&format!("<h1>{}</h1>\n", html_escape(&service.name)));
        let meta_line = format!(
            "Start: {} · Projected end: {} ({}) · Series: {} · Location: {}",
            projection.formatted_start_time,
            projection.formatted_end_time,
            projection.formatted_total_duration,
            if service.series.is_empty() {
                "—"
            } else {
                &service.series
            },
            if service.location.is_empty() {
                "Sanctuary"
            } else {
                &service.location
            }
        );
        html.push_str(&format!(
            "<div class=\"meta\">{}</div>\n",
            html_escape(&meta_line)
        ));
        if !team_members.is_empty() {
            html.push_str(&format!(
                "<div class=\"meta\">Team: {}</div>\n",
                html_escape(&team_members.join(" · "))
            ));
        }
        html.push_str(
            "<table>\n<thead><tr><th>Time</th><th>Item</th><th>Look / Template</th><th>Owner</th><th>Length</th><th>Status</th></tr></thead>\n<tbody>\n",
        );

        let mut order_lines: Vec<(bool, u32, String)> = Vec::new();
        order_lines.push((false, 18, service.name.clone()));
        order_lines.push((false, 10, meta_line));
        if !team_members.is_empty() {
            order_lines.push((false, 10, format!("Team: {}", team_members.join(", "))));
        }
        order_lines.push((
            true,
            10,
            "TIME   KIND          TITLE                          OWNER          LEN".to_owned(),
        ));

        for (item, proj) in service.items.iter().zip(projection.items.iter()) {
            if proj.is_header {
                html.push_str(&format!(
                    "<tr class=\"header-row\"><td colspan=\"6\">{}</td></tr>\n",
                    html_escape(&item.title)
                ));
                order_lines.push((true, 10, format!("--- {} ---", item.title.to_uppercase())));
            } else {
                let title_with_key = if !item.song_key.is_empty() {
                    format!("{} (Key: {})", item.title, item.song_key)
                } else if !item.scripture_reference.is_empty() {
                    format!("{} ({})", item.title, item.scripture_reference)
                } else {
                    item.title.clone()
                };
                let subtitle = if !item.subtitle.is_empty() {
                    format!(" — {}", item.subtitle)
                } else {
                    String::new()
                };
                html.push_str(&format!(
                    "<tr><td>{}</td><td><strong>{}</strong>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                    html_escape(&proj.formatted_time),
                    html_escape(&title_with_key),
                    html_escape(&subtitle),
                    html_escape(if item.look_name.is_empty() {
                        "—"
                    } else {
                        &item.look_name
                    }),
                    html_escape(if item.owner.is_empty() {
                        "—"
                    } else {
                        &item.owner
                    }),
                    html_escape(&proj.formatted_duration),
                    html_escape(if item.status_tag.is_empty() {
                        "Ready"
                    } else {
                        &item.status_tag
                    })
                ));
                order_lines.push((
                    true,
                    10,
                    format!(
                        "{:<6} {:<13} {:<30} {:<14} {:<6}",
                        proj.formatted_time,
                        item_kind_label(item.kind),
                        title_with_key.chars().take(30).collect::<String>(),
                        item.owner.chars().take(14).collect::<String>(),
                        proj.formatted_duration
                    ),
                ));
            }
        }
        html.push_str("</tbody>\n</table>\n");
        pdf_pages.push(order_lines);
    }

    if include_charts {
        for item in &service.items {
            if item.kind != ServiceItemKind::Song as i32 {
                continue;
            }
            let Some(song) = load_and_transpose_song(library, item) else {
                continue;
            };
            let sections = resolve_arrangement_sections(&song, item.arrangement_id.as_ref())
                .unwrap_or_else(|_| song.sections.iter().collect());
            html.push_str("<div class=\"chart-page\">\n");
            let key_label = if song.key.is_empty() {
                "—"
            } else {
                &song.key
            };
            html.push_str(&format!(
                "<h2>{} <small>(Key: {} · Tempo: {} BPM · CCLI: {})</small></h2>\n",
                html_escape(&song.title),
                html_escape(key_label),
                song.tempo as u32,
                html_escape(if song.ccli_number.is_empty() {
                    "—"
                } else {
                    &song.ccli_number
                })
            ));

            let mut chart_lines: Vec<(bool, u32, String)> = Vec::new();
            chart_lines.push((false, 16, format!("{} (Key: {})", song.title, key_label)));
            if !song.authors.is_empty() {
                chart_lines.push((false, 10, song.authors.join(", ")));
            }

            for sec in sections {
                html.push_str(&format!(
                    "<h3>{}</h3>\n<div class=\"chord-block\">",
                    html_escape(&sec.label)
                ));
                chart_lines.push((false, 11, format!("[{}]", sec.label)));
                for line in &sec.lines {
                    let (maybe_chords, lyric) = format_chord_over_lyric_pair(line);
                    if let Some(chord_row) = maybe_chords {
                        html.push_str(&format!(
                            "<span class=\"chord-line\">{}</span>\n",
                            html_escape(&chord_row)
                        ));
                        chart_lines.push((true, 10, chord_row));
                    }
                    html.push_str(&format!("{}\n", html_escape(&lyric)));
                    chart_lines.push((true, 10, lyric));
                }
                html.push_str("</div>\n");
            }
            html.push_str("</div>\n");
            pdf_pages.push(chart_lines);
        }
    }

    html.push_str("</body>\n</html>\n");
    let pdf_bytes = build_pdf_bytes(&pdf_pages);
    let page_count = pdf_pages.len().max(1) as u32;

    let mut written_path = String::new();
    if !req.output_path.trim().is_empty() {
        let target = exchange::validate_output_file_path(&req.output_path)?;
        let is_html = target
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"));
        if is_html {
            exchange::write_atomic_file_checked(&target, html.as_bytes())?;
        } else {
            exchange::write_atomic_file_checked(&target, &pdf_bytes)?;
        }
        written_path = target.to_string_lossy().into_owned();
    }

    Ok(ServicePrintExport {
        path: written_path,
        html,
        pdf_bytes,
        page_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use selah_proto::v1::{Chord, LyricLine};

    #[test]
    fn chord_over_lyric_pair_aligns_chords_at_character_positions() {
        let line = LyricLine {
            text: "Amazing grace how sweet the sound".into(),
            chords: vec![
                Chord {
                    position: 0,
                    text: "G".into(),
                },
                Chord {
                    position: 8,
                    text: "G7/B".into(),
                },
                Chord {
                    position: 14,
                    text: "C".into(),
                },
                Chord {
                    position: 28,
                    text: "G".into(),
                },
            ],
        };
        let (chords, lyric) = format_chord_over_lyric_pair(&line);
        let chord_line = chords.expect("chords present");
        assert_eq!(lyric, "Amazing grace how sweet the sound");
        assert_eq!(&chord_line[0..1], "G");
        assert_eq!(&chord_line[8..12], "G7/B");
        assert_eq!(&chord_line[14..15], "C");
        assert_eq!(&chord_line[28..29], "G");
    }

    #[test]
    fn time_and_duration_formatting_is_tabular_and_deterministic() {
        assert_eq!(format_hh_mm(9 * 3600), "09:00");
        assert_eq!(format_hh_mm(10 * 3600 + 15 * 60), "10:15");
        assert_eq!(format_mm_ss(270), "4:30");
        assert_eq!(format_mm_ss(1920), "32:00");
        assert_eq!(format_total_duration(4500), "1 hr 15 min");
    }
}
