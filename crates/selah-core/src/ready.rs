//! Ready for Sunday (M4.7): the checks to run before a service.
//!
//! Each check is a small function over plain inputs, so each has its own unit tests (M4-A8).
//! A check that doesn't pass carries an error code from `content/errors/errors.yaml`; the code
//! names the fix article, and `selah-kb-lint` (KB-COVER) fails the build if that article doesn't
//! exist. Words for the UI are message keys in `content/i18n/en/*.ftl` (ADR-010).
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use selah_proto::v1::{
    Id, MediaItem, OutputConfig, OutputFrameStatus, OutputRole, ReadyCheck, ReadyCheckKind,
    ReadyCheckStatus, ReadyReport, Service, ServiceItem, ServiceItemKind, Song, Template,
};
use selah_render::fit::FitTarget;
use selah_render::template::TemplateStyle;

use crate::library::Library;
use crate::songs::split::{SplitOptions, generate_slides_with_fit};

pub const MEDIA_MISSING: &str = "SEL-RDY-0001";
pub const FONT_MISSING: &str = "SEL-RDY-0002";
pub const TEXT_DOESNT_FIT: &str = "SEL-RDY-0003";
pub const NO_COPYRIGHT: &str = "SEL-RDY-0004";
pub const NO_CCLI_LICENCE: &str = "SEL-RDY-0005";
pub const NO_SCREENS: &str = "SEL-RDY-0006";
pub const NO_BACKUP: &str = "SEL-RDY-0007";
pub const FLASHING: &str = "SEL-RDY-0008";
pub const TOO_LOUD: &str = "SEL-RDY-0009";

/// The church's CCLI licence number (a policy setting; first-run setup asks for it).
pub const CCLI_LICENCE_KEY: &str = "church.ccli-licence";

/// SAFE-05: the master limiter keeps peaks under -1 dBTP; Selah never lets it be turned off.
pub const LIMITER_ALWAYS_ON: bool = true;

/// Fonts Selah ships with; anything else falls back to Noto Sans on the screens.
pub const BUNDLED_FONTS: [&str; 6] = [
    "Inter",
    "Manrope",
    "JetBrains Mono",
    "Noto Sans",
    "Noto Serif",
    "Noto Sans KR",
];

/// Error code → fix article, from the error catalogue compiled into core.
pub fn fix_article(code: &str) -> &'static str {
    static TABLE: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            let source: &'static str = include_str!("../../../content/errors/errors.yaml");
            let mut table = HashMap::new();
            let mut code_now: Option<String> = None;
            for line in source.lines() {
                let line = line.trim();
                if let Some(c) = line.strip_prefix("- code:") {
                    code_now = Some(c.trim().to_owned());
                } else if let Some(article) = line.strip_prefix("fix_article:")
                    && let Some(c) = code_now.take()
                {
                    table.insert(c, article.trim());
                }
            }
            table
        })
        .get(code)
        .copied()
        .unwrap_or("")
}

fn pass(kind: ReadyCheckKind, message_key: &str) -> ReadyCheck {
    ReadyCheck {
        kind: kind as i32,
        status: ReadyCheckStatus::Pass as i32,
        message_key: message_key.into(),
        item_index: -1,
        ..ReadyCheck::default()
    }
}

fn problem(
    kind: ReadyCheckKind,
    status: ReadyCheckStatus,
    code: &str,
    message_key: &str,
    item: Option<(usize, &ServiceItem)>,
    args: &[(&str, &str)],
) -> ReadyCheck {
    let mut map: HashMap<String, String> = args
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    if let Some((_, item)) = item {
        map.entry("item".into())
            .or_insert_with(|| item.title.clone());
    }
    ReadyCheck {
        kind: kind as i32,
        status: status as i32,
        code: code.into(),
        message_key: message_key.into(),
        args: map,
        fix_article_id: fix_article(code).into(),
        item_index: item.map_or(-1, |(i, _)| i as i32),
        item_title: item.map(|(_, it)| it.title.clone()).unwrap_or_default(),
    }
}

fn is(item: &ServiceItem, kind: ServiceItemKind) -> bool {
    item.kind == kind as i32
}

fn some_id(id: &Option<Id>) -> Option<&Id> {
    id.as_ref().filter(|i| !i.value.is_empty())
}

/// What the media check needs to know about one file: does it exist with the right contents?
pub trait MediaFiles {
    fn item(&self, id: &Id) -> Option<MediaItem>;
    fn verified(&self, item: &MediaItem) -> bool;
}

impl MediaFiles for Library {
    fn item(&self, id: &Id) -> Option<MediaItem> {
        self.get::<MediaItem>(id).ok().flatten()
    }

    fn verified(&self, item: &MediaItem) -> bool {
        self.media_store()
            .and_then(|store| store.verify(&item.content_hash, &item.extension, item.size_bytes))
            .is_ok()
    }
}

/// Media ids the service uses: media and audio items, and item backgrounds.
fn media_ids(service: &Service) -> Vec<(usize, &ServiceItem, &Id)> {
    let mut out = Vec::new();
    for (i, item) in service.items.iter().enumerate() {
        if (is(item, ServiceItemKind::Media) || is(item, ServiceItemKind::Audio))
            && let Some(id) = some_id(&item.content_id)
        {
            out.push((i, item, id));
        }
        if let Some(id) = some_id(&item.background_media_id) {
            out.push((i, item, id));
        }
    }
    out
}

/// Every picture, video and sound file is there and unchanged (BLAKE3 hash, SEC-04).
pub fn check_media(service: &Service, files: &impl MediaFiles) -> Vec<ReadyCheck> {
    let mut out = Vec::new();
    for (i, item, id) in media_ids(service) {
        let ok = files.item(id).is_some_and(|m| files.verified(&m));
        if !ok {
            out.push(problem(
                ReadyCheckKind::Media,
                ReadyCheckStatus::Fail,
                MEDIA_MISSING,
                "error-ready-media-missing",
                Some((i, item)),
                &[],
            ));
        }
    }
    if out.is_empty() {
        out.push(pass(ReadyCheckKind::Media, "ready-media-ok"));
    }
    out
}

/// Every flagged file in the service (SAFE-04): flashing content needs a warning first.
pub fn check_flash(service: &Service, files: &impl MediaFiles) -> Vec<ReadyCheck> {
    let mut out = Vec::new();
    for (i, item, id) in media_ids(service) {
        if files.item(id).is_some_and(|m| {
            m.flash_warning || m.tags.iter().any(|t| t.eq_ignore_ascii_case("flashing"))
        }) {
            out.push(problem(
                ReadyCheckKind::Flash,
                ReadyCheckStatus::Warn,
                FLASHING,
                "error-ready-flashing",
                Some((i, item)),
                &[],
            ));
        }
    }
    if out.is_empty() {
        out.push(pass(ReadyCheckKind::Flash, "ready-flash-ok"));
    }
    out
}

/// The font names a template asks for that Selah doesn't have.
pub fn missing_fonts(style: &TemplateStyle) -> Vec<String> {
    let roles = &style.roles;
    let mut missing: Vec<String> = [
        &roles.body.font_family,
        &roles.title.font_family,
        &roles.reference.font_family,
        &roles.footer.font_family,
        &roles.translation.font_family,
        &roles.chords.font_family,
    ]
    .into_iter()
    .filter(|name| {
        let wanted = name.trim();
        !wanted.is_empty()
            && !BUNDLED_FONTS.iter().any(|f| f.eq_ignore_ascii_case(wanted))
            && selah_render::text::canonical_font_family(wanted) == "Noto Sans"
    })
    .cloned()
    .collect();
    missing.sort();
    missing.dedup();
    missing
}

/// Templates the service uses, with the first item that uses each.
fn templates<'a>(
    service: &'a Service,
    library: &Library,
) -> Vec<(Option<(usize, &'a ServiceItem)>, TemplateStyle)> {
    let mut seen: BTreeMap<Vec<u8>, ()> = BTreeMap::new();
    let mut out = Vec::new();
    let default = some_id(&service.default_template_id);
    for (i, item) in service.items.iter().enumerate() {
        let Some(id) = some_id(&item.template_id).or(default) else {
            continue;
        };
        if seen.insert(id.value.clone(), ()).is_some() {
            continue;
        }
        if let Ok(Some(template)) = library.get::<Template>(id)
            && let Ok(style) = TemplateStyle::from_proto_template(&template)
        {
            out.push((Some((i, item)), style));
        }
    }
    out
}

pub fn check_fonts(styles: &[(Option<(usize, &ServiceItem)>, TemplateStyle)]) -> Vec<ReadyCheck> {
    let mut out = Vec::new();
    for (item, style) in styles {
        for font in missing_fonts(style) {
            out.push(problem(
                ReadyCheckKind::Fonts,
                ReadyCheckStatus::Warn,
                FONT_MISSING,
                "error-ready-font-missing",
                *item,
                &[("font", font.as_str())],
            ));
        }
    }
    if out.is_empty() {
        out.push(pass(ReadyCheckKind::Fonts, "ready-fonts-ok"));
    }
    out
}

/// Audience screens: every enabled output except stage screens.
pub fn audience_outputs(outputs: &[OutputConfig]) -> Vec<&OutputConfig> {
    outputs
        .iter()
        .filter(|o| o.enabled && o.role != OutputRole::Stage as i32)
        .collect()
}

/// The words of every song fit on every audience screen at the template's smallest size.
pub fn check_text_fits(
    songs: &[(usize, &ServiceItem, Song, TemplateStyle)],
    outputs: &[OutputConfig],
) -> Vec<ReadyCheck> {
    let mut out = Vec::new();
    let screens = audience_outputs(outputs);
    for (i, item, song, style) in songs {
        let targets: Vec<FitTarget> = screens
            .iter()
            .map(|o| {
                let res = o.resolution.unwrap_or(selah_proto::v1::Resolution {
                    width: 1920,
                    height: 1080,
                });
                FitTarget {
                    output_index: o.output_index,
                    output_name: o.name.clone(),
                    width: res.width.max(1),
                    height: res.height.max(1),
                    style: style.clone(),
                    max_lines_override: None,
                }
            })
            .collect();
        if targets.is_empty() {
            continue;
        }
        let options = SplitOptions {
            arrangement_id: item.arrangement_id.clone(),
            max_lines: 0,
            copyright_placement: selah_proto::v1::CopyrightPlacement::Unspecified,
            title_slide: false,
            blank_slide_at_end: false,
            translation_language: None,
        };
        let Ok(generated) = generate_slides_with_fit(song, &options, &targets) else {
            continue;
        };
        for report in generated
            .fit_reports
            .iter()
            .filter(|r| !r.fits || r.overflows_safe_area)
        {
            out.push(problem(
                ReadyCheckKind::TextFits,
                ReadyCheckStatus::Warn,
                TEXT_DOESNT_FIT,
                "error-ready-text-doesnt-fit",
                Some((*i, item)),
                &[("output", report.output_name.as_str())],
            ));
        }
    }
    if out.is_empty() {
        out.push(pass(ReadyCheckKind::TextFits, "ready-text-fits-ok"));
    }
    out
}

/// Songs show a copyright line, and the church's CCLI licence number is set.
pub fn check_copyright(
    songs: &[(usize, &ServiceItem, Song)],
    ccli_licence: &str,
) -> Vec<ReadyCheck> {
    let mut out = Vec::new();
    for (i, item, song) in songs {
        if song.copyright.trim().is_empty() && song.ccli_number.trim().is_empty() {
            out.push(problem(
                ReadyCheckKind::Copyright,
                ReadyCheckStatus::Warn,
                NO_COPYRIGHT,
                "error-ready-no-copyright",
                Some((*i, item)),
                &[],
            ));
        }
    }
    let licence = ccli_licence.trim();
    if !songs.is_empty() && (licence.is_empty() || !licence.chars().all(|c| c.is_ascii_digit())) {
        out.push(problem(
            ReadyCheckKind::Copyright,
            ReadyCheckStatus::Warn,
            NO_CCLI_LICENCE,
            "error-ready-no-ccli-licence",
            None,
            &[],
        ));
    }
    if out.is_empty() {
        out.push(pass(ReadyCheckKind::Copyright, "ready-copyright-ok"));
    }
    out
}

/// The engine runs and every enabled screen is showing frames.
pub fn check_outputs(
    outputs: &[OutputConfig],
    engine_connected: bool,
    statuses: &[OutputFrameStatus],
) -> Vec<ReadyCheck> {
    let enabled: Vec<&OutputConfig> = outputs.iter().filter(|o| o.enabled).collect();
    if !engine_connected || enabled.is_empty() {
        return vec![problem(
            ReadyCheckKind::Outputs,
            ReadyCheckStatus::Fail,
            NO_SCREENS,
            "error-ready-no-screens",
            None,
            &[],
        )];
    }
    let mut out = Vec::new();
    for output in enabled {
        if !statuses
            .iter()
            .any(|s| s.output_index == output.output_index)
        {
            out.push(problem(
                ReadyCheckKind::Outputs,
                ReadyCheckStatus::Fail,
                NO_SCREENS,
                "error-ready-screen-not-showing",
                None,
                &[("output", output.name.as_str())],
            ));
        }
    }
    if out.is_empty() {
        out.push(pass(ReadyCheckKind::Outputs, "ready-outputs-ok"));
    }
    out
}

/// Backup sync arrives in M7; until then the check says so and links to how to back up.
pub fn check_backup() -> Vec<ReadyCheck> {
    vec![problem(
        ReadyCheckKind::Backup,
        ReadyCheckStatus::Info,
        NO_BACKUP,
        "error-ready-no-backup-sync",
        None,
        &[],
    )]
}

/// SAFE-05: the master limiter is on. Selah never lets it be turned off, so this passes unless
/// a future build changes that, which this check (and its test) would catch.
pub fn check_sound(limiter_on: bool) -> Vec<ReadyCheck> {
    if limiter_on {
        vec![pass(ReadyCheckKind::Sound, "ready-sound-ok")]
    } else {
        vec![problem(
            ReadyCheckKind::Sound,
            ReadyCheckStatus::Fail,
            TOO_LOUD,
            "error-ready-limiter-off",
            None,
            &[],
        )]
    }
}

/// Everything core needs to run every check for one service.
pub struct ReadyInputs<'a> {
    pub library: &'a Library,
    pub service: &'a Service,
    pub outputs: &'a [OutputConfig],
    pub engine_connected: bool,
    pub output_statuses: &'a [OutputFrameStatus],
}

pub fn run(inputs: &ReadyInputs<'_>) -> ReadyReport {
    let library = inputs.library;
    let service = inputs.service;
    let styles = templates(service, library);
    let default_style = |item: &ServiceItem| {
        some_id(&item.template_id)
            .or(some_id(&service.default_template_id))
            .and_then(|id| library.get::<Template>(id).ok().flatten())
            .and_then(|t| TemplateStyle::from_proto_template(&t).ok())
            .unwrap_or_default()
    };
    let mut songs = Vec::new();
    let mut fit_songs = Vec::new();
    for (i, item) in service.items.iter().enumerate() {
        if is(item, ServiceItemKind::Song)
            && let Some(id) = some_id(&item.content_id)
            && let Ok(Some(song)) = library.get::<Song>(id)
        {
            fit_songs.push((i, item, song.clone(), default_style(item)));
            songs.push((i, item, song));
        }
    }
    let ccli = library
        .setting(CCLI_LICENCE_KEY)
        .ok()
        .flatten()
        .and_then(|s| s.value.as_str().map(str::to_owned))
        .unwrap_or_default();

    let mut checks = Vec::new();
    checks.extend(check_media(service, library));
    checks.extend(check_fonts(&styles));
    checks.extend(check_text_fits(&fit_songs, inputs.outputs));
    checks.extend(check_copyright(&songs, &ccli));
    checks.extend(check_outputs(
        inputs.outputs,
        inputs.engine_connected,
        inputs.output_statuses,
    ));
    checks.extend(check_backup());
    checks.extend(check_flash(service, library));
    checks.extend(check_sound(LIMITER_ALWAYS_ON));
    let ready = !checks
        .iter()
        .any(|c| c.status == ReadyCheckStatus::Fail as i32);
    ReadyReport {
        service_id: service.id.clone(),
        checks,
        ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use selah_proto::v1::{LyricLine, Resolution, Section, SectionKind};

    struct Files {
        items: Vec<MediaItem>,
        good: Vec<String>,
    }

    impl MediaFiles for Files {
        fn item(&self, id: &Id) -> Option<MediaItem> {
            self.items
                .iter()
                .find(|m| m.id.as_ref().map(|i| &i.value) == Some(&id.value))
                .cloned()
        }

        fn verified(&self, item: &MediaItem) -> bool {
            self.good.contains(&item.content_hash)
        }
    }

    fn id(v: &str) -> Id {
        Id { value: v.into() }
    }

    fn media(v: &str, hash: &str, flashing: bool) -> MediaItem {
        MediaItem {
            id: Some(id(v)),
            name: v.into(),
            content_hash: hash.into(),
            flash_warning: flashing,
            ..MediaItem::default()
        }
    }

    fn item(kind: ServiceItemKind, title: &str, content: Option<&str>) -> ServiceItem {
        ServiceItem {
            id: Some(id(title)),
            kind: kind as i32,
            title: title.into(),
            content_id: content.map(id),
            ..ServiceItem::default()
        }
    }

    fn statuses(checks: &[ReadyCheck]) -> Vec<(i32, String)> {
        checks.iter().map(|c| (c.status, c.code.clone())).collect()
    }

    #[test]
    fn every_ready_code_links_to_a_fix_article() {
        for code in [
            MEDIA_MISSING,
            FONT_MISSING,
            TEXT_DOESNT_FIT,
            NO_COPYRIGHT,
            NO_CCLI_LICENCE,
            NO_SCREENS,
            NO_BACKUP,
            FLASHING,
            TOO_LOUD,
        ] {
            assert!(!fix_article(code).is_empty(), "{code} has no fix article");
        }
    }

    #[test]
    fn media_check_fails_for_missing_or_changed_files() {
        let service = Service {
            items: vec![
                item(ServiceItemKind::Media, "Welcome loop", Some("m1")),
                item(ServiceItemKind::Audio, "Walk-in music", Some("m2")),
                ServiceItem {
                    background_media_id: Some(id("m3")),
                    ..item(ServiceItemKind::Song, "Amazing Grace", None)
                },
            ],
            ..Service::default()
        };
        let files = Files {
            items: vec![media("m1", "aa", false), media("m2", "bb", false)],
            good: vec!["aa".into()],
        };
        let checks = check_media(&service, &files);
        // m2 is there but its contents changed; m3 is gone.
        assert_eq!(checks.len(), 2);
        assert!(
            checks
                .iter()
                .all(|c| c.status == ReadyCheckStatus::Fail as i32
                    && c.code == MEDIA_MISSING
                    && c.fix_article_id == "troubleshooting.media-is-missing")
        );
        assert_eq!(checks[0].item_index, 1);
        assert_eq!(
            checks[1].args.get("item").map(String::as_str),
            Some("Amazing Grace")
        );

        let all_good = Files {
            items: vec![
                media("m1", "aa", false),
                media("m2", "bb", false),
                media("m3", "cc", false),
            ],
            good: vec!["aa".into(), "bb".into(), "cc".into()],
        };
        assert_eq!(
            statuses(&check_media(&service, &all_good)),
            vec![(ReadyCheckStatus::Pass as i32, String::new())]
        );
    }

    #[test]
    fn flash_check_warns_about_flagged_media() {
        // safe: SAFE-04
        let service = Service {
            items: vec![item(ServiceItemKind::Media, "Strobe intro", Some("m1"))],
            ..Service::default()
        };
        let flagged = Files {
            items: vec![media("m1", "aa", true)],
            good: vec![],
        };
        let checks = check_flash(&service, &flagged);
        assert_eq!(
            statuses(&checks),
            vec![(ReadyCheckStatus::Warn as i32, FLASHING.into())]
        );
        assert_eq!(
            checks[0].fix_article_id,
            "troubleshooting.flashing-content-warnings"
        );
        let tagged = Files {
            items: vec![MediaItem {
                tags: vec!["Flashing".into()],
                ..media("m1", "aa", false)
            }],
            good: vec![],
        };
        assert_eq!(check_flash(&service, &tagged).len(), 1);
        let calm = Files {
            items: vec![media("m1", "aa", false)],
            good: vec![],
        };
        assert_eq!(
            check_flash(&service, &calm)[0].status,
            ReadyCheckStatus::Pass as i32
        );
    }

    #[test]
    fn font_check_names_fonts_selah_does_not_have() {
        let mut style = TemplateStyle::preset_sanctuary();
        assert!(missing_fonts(&style).is_empty());
        style.roles.title.font_family = "Comic Papyrus".into();
        assert_eq!(missing_fonts(&style), vec!["Comic Papyrus".to_owned()]);
        let it = item(ServiceItemKind::Song, "Holy, Holy, Holy", None);
        let checks = check_fonts(&[(Some((0, &it)), style)]);
        assert_eq!(
            statuses(&checks),
            vec![(ReadyCheckStatus::Warn as i32, FONT_MISSING.into())]
        );
        assert_eq!(
            checks[0].args.get("font").map(String::as_str),
            Some("Comic Papyrus")
        );
        assert_eq!(check_fonts(&[])[0].status, ReadyCheckStatus::Pass as i32);
    }

    fn song(words: &str) -> Song {
        Song {
            id: Some(id("s1")),
            title: "Test".into(),
            sections: vec![Section {
                id: Some(id("v1")),
                kind: SectionKind::Verse as i32,
                label: "Verse 1".into(),
                lines: vec![LyricLine {
                    text: words.into(),
                    chords: Vec::new(),
                }],
                ..Section::default()
            }],
            ..Song::default()
        }
    }

    #[test]
    fn text_fit_check_warns_when_words_cannot_fit_a_screen() {
        let outputs = vec![
            OutputConfig {
                output_index: 0,
                name: "Main".into(),
                role: OutputRole::Main as i32,
                resolution: Some(Resolution {
                    width: 640,
                    height: 360,
                }),
                enabled: true,
                ..OutputConfig::default()
            },
            OutputConfig {
                output_index: 1,
                name: "Stage".into(),
                role: OutputRole::Stage as i32,
                enabled: true,
                ..OutputConfig::default()
            },
        ];
        // A safe area too short for even one line at the smallest size.
        let mut big = TemplateStyle::preset_sanctuary();
        big.roles.body.size = 120.0;
        big.roles.body.min_size = 120.0;
        big.safe_area.top = 0.45;
        big.safe_area.bottom = 0.45;
        let it = item(ServiceItemKind::Song, "Long word", Some("s1"));
        let long = song(&"Hallelujahhallelujahhallelujah".repeat(3));
        let checks = check_text_fits(&[(0, &it, long, big.clone())], &outputs);
        // Only the audience screen is checked; the stage has its own layout.
        assert_eq!(
            statuses(&checks),
            vec![(ReadyCheckStatus::Warn as i32, TEXT_DOESNT_FIT.into())]
        );
        assert_eq!(
            checks[0].args.get("output").map(String::as_str),
            Some("Main")
        );

        let short = song("Amazing grace");
        let fine = check_text_fits(
            &[(0, &it, short, TemplateStyle::preset_sanctuary())],
            &outputs,
        );
        assert_eq!(fine[0].status, ReadyCheckStatus::Pass as i32);
    }

    #[test]
    fn copyright_check_wants_song_credits_and_the_ccli_licence() {
        let it = item(ServiceItemKind::Song, "New song", Some("s1"));
        let bare = song("Words");
        let credited = Song {
            copyright: "Public Domain".into(),
            ..song("Words")
        };
        let checks = check_copyright(&[(0, &it, bare)], "");
        assert_eq!(
            statuses(&checks),
            vec![
                (ReadyCheckStatus::Warn as i32, NO_COPYRIGHT.into()),
                (ReadyCheckStatus::Warn as i32, NO_CCLI_LICENCE.into())
            ]
        );
        assert_eq!(
            statuses(&check_copyright(&[(0, &it, credited.clone())], "CCLI-12")),
            vec![(ReadyCheckStatus::Warn as i32, NO_CCLI_LICENCE.into())],
            "a licence number is digits"
        );
        assert_eq!(
            check_copyright(&[(0, &it, credited)], "1234567")[0].status,
            ReadyCheckStatus::Pass as i32
        );
        // No songs, no licence needed.
        assert_eq!(
            check_copyright(&[], "")[0].status,
            ReadyCheckStatus::Pass as i32
        );
    }

    #[test]
    fn output_check_fails_until_every_enabled_screen_shows_frames() {
        let outputs = vec![
            OutputConfig {
                output_index: 0,
                name: "Main".into(),
                enabled: true,
                ..OutputConfig::default()
            },
            OutputConfig {
                output_index: 1,
                name: "Lobby".into(),
                enabled: true,
                ..OutputConfig::default()
            },
        ];
        let shown = |i| OutputFrameStatus {
            output_index: i,
            ..OutputFrameStatus::default()
        };
        assert_eq!(
            statuses(&check_outputs(&outputs, false, &[])),
            vec![(ReadyCheckStatus::Fail as i32, NO_SCREENS.into())]
        );
        let lobby_dark = check_outputs(&outputs, true, &[shown(0)]);
        assert_eq!(lobby_dark.len(), 1);
        assert_eq!(
            lobby_dark[0].args.get("output").map(String::as_str),
            Some("Lobby")
        );
        assert_eq!(
            lobby_dark[0].fix_article_id,
            "troubleshooting.screens-are-black"
        );
        assert_eq!(
            check_outputs(&outputs, true, &[shown(0), shown(1)])[0].status,
            ReadyCheckStatus::Pass as i32
        );
    }

    #[test]
    fn backup_check_says_sync_is_not_set_up_yet() {
        let checks = check_backup();
        assert_eq!(
            statuses(&checks),
            vec![(ReadyCheckStatus::Info as i32, NO_BACKUP.into())]
        );
        assert!(!checks[0].fix_article_id.is_empty());
    }

    #[test]
    fn sound_check_passes_only_with_the_limiter_on() {
        // safe: SAFE-05
        assert!(LIMITER_ALWAYS_ON);
        assert_eq!(check_sound(true)[0].status, ReadyCheckStatus::Pass as i32);
        assert_eq!(
            statuses(&check_sound(false)),
            vec![(ReadyCheckStatus::Fail as i32, TOO_LOUD.into())]
        );
    }
}
