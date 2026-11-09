//! `selah-bench search`: 10,000-song search latency & accuracy benchmark (M2.3, M2-A4).
use std::time::Instant;

use anyhow::{Result, bail};
use selah_core::library::{Library, new_id};
use selah_proto::v1::{LyricLine, Section, SectionKind, Song};
use serde::Serialize;

/// ARCHITECTURE §12 & M2-A4: song search p95 <= 50 ms on 10,000 songs.
pub const SEARCH_P95_BUDGET_MS: f64 = 50.0;

pub struct Options {
    pub songs: usize,
    pub iterations: usize,
}

#[derive(Serialize)]
pub struct SearchReport {
    pub benchmark: &'static str,
    pub os: &'static str,
    pub songs: usize,
    pub iterations: usize,
    pub correctness_pass: bool,
    pub seed_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub amzing_grace_p95_ms: f64,
    pub max_ms: f64,
    pub mean_ms: f64,
    pub budget_p95_ms: f64,
    pub pass: bool,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

struct Lcg(u64);

impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        (self.0 >> 32) as u32
    }

    fn pick<'a>(&mut self, items: &'a [&'a str]) -> &'a str {
        items[(self.next_u32() as usize) % items.len()]
    }
}

fn make_song(
    title: &str,
    author: &str,
    ccli: &str,
    tags: &[&str],
    verses: &[(&str, &[&str])],
) -> Song {
    let sections = verses
        .iter()
        .map(|(label, lines)| Section {
            id: Some(new_id()),
            label: (*label).into(),
            kind: if *label == "Chorus" {
                SectionKind::Chorus as i32
            } else {
                SectionKind::Verse as i32
            },
            lines: lines
                .iter()
                .map(|line| LyricLine {
                    text: (*line).into(),
                    chords: Vec::new(),
                })
                .collect(),
            ..Default::default()
        })
        .collect();

    Song {
        id: Some(new_id()),
        title: title.into(),
        authors: vec![author.into()],
        ccli_number: ccli.into(),
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        public_domain: true,
        sections,
        ..Default::default()
    }
}

fn anchor_songs() -> Vec<Song> {
    vec![
        make_song(
            "Amazing Grace",
            "John Newton",
            "22025",
            &["Hymn", "Classic", "Closing"],
            &[
                (
                    "Verse 1",
                    &[
                        "Amazing grace how sweet the sound",
                        "That saved a wretch like me",
                        "I once was lost but now am found",
                        "Was blind but now I see",
                    ],
                ),
                (
                    "Verse 2",
                    &[
                        "Twas grace that taught my heart to fear",
                        "And grace my fears relieved",
                        "How precious did that grace appear",
                        "The hour I first believed",
                    ],
                ),
            ],
        ),
        make_song(
            "Christmas Carol Medley",
            "Traditional Carol",
            "10042",
            &["Seasonal", "Hymn"],
            &[
                (
                    "Verse 1",
                    &[
                        "Joy to the world on Christmas morn",
                        "Glad tidings to the earth are borne",
                    ],
                ),
                (
                    "Chorus",
                    &["Sing Noel and lift your voice", "Let every heart rejoice"],
                ),
            ],
        ),
        make_song(
            "Beneath the Cross",
            "Isaac Watts",
            "33018",
            &["Good Friday", "Hymn"],
            &[(
                "Verse 1",
                &[
                    "Beneath the cross of Jesus I fain would take my stand",
                    "The shadow of a mighty rock within a weary land",
                ],
            )],
        ),
        make_song(
            "All Creatures Praise",
            "Francis of Assisi",
            "44019",
            &["Call to Worship", "Praise"],
            &[(
                "Chorus",
                &[
                    "Hallelujah to the King of heaven",
                    "Praise Him in the highest place",
                ],
            )],
        ),
        make_song(
            "Ọlọ́run Ẹlẹ́dàá",
            "Yoruba Traditional",
            "55001",
            &["Yoruba", "Worship"],
            &[(
                "Verse 1",
                &["Ọlọ́run alààyè ni Ọ", "Ìwọ ni Olùgbàlà wa títí láé"],
            )],
        ),
        make_song(
            "Amazing Love",
            "Charles Wesley",
            "24567",
            &["Hymn", "Classic"],
            &[(
                "Verse 1",
                &[
                    "And can it be that I should gain",
                    "An interest in the Saviours blood",
                ],
            )],
        ),
        make_song(
            "Grace Unmeasured",
            "Bob Kauflin",
            "4591234",
            &["Modern", "Communion"],
            &[(
                "Verse 1",
                &[
                    "Grace unmeasured vast and free",
                    "That you knew me from eternity",
                ],
            )],
        ),
        make_song(
            "Grace Greater Than All Our Sin",
            "Julia Johnston",
            "31690",
            &["Hymn"],
            &[(
                "Chorus",
                &[
                    "Grace grace Gods grace",
                    "Grace that will pardon and cleanse within",
                ],
            )],
        ),
    ]
}

fn build_synthetic_song(index: usize, rng: &mut Lcg) -> Song {
    const ADJECTIVES: &[&str] = &[
        "Everlasting",
        "Faithful",
        "Glorious",
        "Merciful",
        "Sovereign",
        "Radiant",
        "Risen",
        "Blessed",
        "Abiding",
        "Eternal",
        "Steadfast",
        "Victorious",
        "Humble",
        "Living",
        "Graceful",
        "Gentle",
    ];
    const NOUNS: &[&str] = &[
        "Mercy",
        "Kingdom",
        "Refuge",
        "Shepherd",
        "Morning",
        "Covenant",
        "Promise",
        "Hope",
        "Strength",
        "Light",
        "River",
        "Banner",
        "Cornerstone",
        "Anchor",
        "Redeemer",
        "Crown",
    ];
    const VERBS: &[&str] = &[
        "sing", "rejoice", "praise", "proclaim", "trust", "stand", "remember", "declare", "honour",
        "bless", "behold", "follow", "seek", "offer", "gather", "listen",
    ];
    const AUTHORS: &[&str] = &[
        "Isaac Watts",
        "Charles Wesley",
        "Fanny Crosby",
        "Thomas Chisholm",
        "Frances Havergal",
        "William Cowper",
        "Horatio Spafford",
        "Anne Steele",
        "Samuel Medley",
        "John Bakewell",
        "Robert Robinson",
        "Augustus Toplady",
    ];
    const TAGS: &[&str] = &[
        "Praise",
        "Morning",
        "Communion",
        "Offerings",
        "Closing",
        "Thanksgiving",
        "Confession",
        "Assurance",
    ];

    let a1 = rng.pick(ADJECTIVES);
    let n1 = rng.pick(NOUNS);
    let a2 = rng.pick(ADJECTIVES);
    let n2 = rng.pick(NOUNS);
    let v1 = rng.pick(VERBS);
    let v2 = rng.pick(VERBS);
    let author = rng.pick(AUTHORS);
    let tag1 = rng.pick(TAGS);
    let tag2 = rng.pick(TAGS);
    let include_grace = index.is_multiple_of(17);
    let grace_clause = if include_grace { " in grace" } else { "" };

    let title = format!("{a1} {n1} #{index}");
    let ccli = format!("{}", 100_000 + index);
    let line1 = format!("We {v1} of {a1} {n1} singing before the throne{grace_clause}");
    let line2 = format!("Through every {a2} {n2} we {v2} your faithful way");
    let line3 = "Let every tribe and nation proclaiming peace today".to_string();
    let line4 = "Hold fast the living promise that guides our steps alway".to_string();
    let line5 = "Forever reigning in glory our joyful song we bring".to_string();
    let line6 = "Lift every voice together in praise to Heaven's King".to_string();

    make_song(
        &title,
        author,
        &ccli,
        &[tag1, tag2],
        &[
            ("Verse 1", &[&line1, &line2]),
            ("Chorus", &[&line3, &line4]),
            ("Verse 2", &[&line5, &line6]),
        ],
    )
}

pub fn run(options: &Options) -> Result<SearchReport> {
    let dir = tempfile::tempdir()?;
    let mut library = Library::open(dir.path())?;

    let anchors = anchor_songs();
    let total_songs = options.songs.max(anchors.len());
    let mut rng = Lcg(0x5E1A_B3C4_0000_0001);

    // Spread anchor songs naturally through the corpus rather than placing all at rowid 1..8.
    let anchor_positions: Vec<usize> = (0..anchors.len())
        .map(|i| ((i + 1) * (total_songs / (anchors.len() + 1))).min(total_songs - 1))
        .collect();

    let seed_start = Instant::now();
    let mut batch = Vec::with_capacity(1000);
    let mut anchor_idx = 0usize;
    for i in 0..total_songs {
        let song = if anchor_idx < anchors.len() && anchor_positions[anchor_idx] == i {
            let s = anchors[anchor_idx].clone();
            anchor_idx += 1;
            s
        } else {
            build_synthetic_song(i, &mut rng)
        };
        batch.push(song);
        if batch.len() >= 1000 {
            library.save_songs_batch(&batch)?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        library.save_songs_batch(&batch)?;
    }
    let seed_ms = seed_start.elapsed().as_secs_f64() * 1000.0;

    // Verify correctness before timing.
    let typo_res = library.search_songs("amzing grace", 10, 0)?;
    let typo_top_ok = typo_res
        .hits
        .first()
        .and_then(|h| h.song.as_ref())
        .is_some_and(|s| s.title == "Amazing Grace");

    let xmas_res = library.search_songs("xmas", 10, 0)?;
    let xmas_ok = xmas_res.hits.iter().any(|h| {
        h.song
            .as_ref()
            .is_some_and(|s| s.title == "Christmas Carol Medley")
    });

    let calvary_res = library.search_songs("calvary", 10, 0)?;
    let calvary_ok = calvary_res.hits.iter().any(|h| {
        h.song
            .as_ref()
            .is_some_and(|s| s.title == "Beneath the Cross")
    });

    let alleluia_res = library.search_songs("alleluia", 10, 0)?;
    let alleluia_ok = alleluia_res.hits.iter().any(|h| {
        h.song
            .as_ref()
            .is_some_and(|s| s.title == "All Creatures Praise")
    });

    let ccli_res = library.search_songs("22025", 10, 0)?;
    let ccli_ok = ccli_res
        .hits
        .first()
        .and_then(|h| h.song.as_ref())
        .is_some_and(|s| s.title == "Amazing Grace");

    let author_res = library.search_songs("John Newton", 10, 0)?;
    let author_ok = author_res
        .hits
        .first()
        .and_then(|h| h.song.as_ref())
        .is_some_and(|s| s.title == "Amazing Grace");

    let lyric_typo_res = library.search_songs("wrecth", 10, 0)?;
    let lyric_typo_ok = lyric_typo_res
        .hits
        .first()
        .and_then(|h| h.song.as_ref())
        .is_some_and(|s| s.title == "Amazing Grace");

    let yoruba_res = library.search_songs("olorun", 10, 0)?;
    let yoruba_ok = yoruba_res
        .hits
        .first()
        .and_then(|h| h.song.as_ref())
        .is_some_and(|s| s.title == "Ọlọ́run Ẹlẹ́dàá");

    let correctness_pass = typo_top_ok
        && xmas_ok
        && calvary_ok
        && alleluia_ok
        && ccli_ok
        && author_ok
        && lyric_typo_ok
        && yoruba_ok;

    if !correctness_pass {
        let top_title = |r: &selah_proto::v1::SongSearchResults| {
            r.hits
                .first()
                .and_then(|h| h.song.as_ref())
                .map(|s| {
                    format!(
                        "{} (ccli={}, score={:.2}, fields={:?})",
                        s.title, s.ccli_number, r.hits[0].score, r.hits[0].matched_fields
                    )
                })
                .unwrap_or_else(|| "<none>".to_owned())
        };
        bail!(
            "search accuracy check failed: typo_top={typo_top_ok} [{}], xmas={xmas_ok}, calvary={calvary_ok}, alleluia={alleluia_ok}, ccli={ccli_ok} [{}], author={author_ok}, lyric_typo={lyric_typo_ok} [{}], yoruba={yoruba_ok}",
            top_title(&typo_res),
            top_title(&ccli_res),
            top_title(&lyric_typo_res),
        );
    }

    let queries = [
        "Amazing Grace",
        "amzing grace",
        "grace",
        "sweet the sound",
        "wrecth",
        "xmas",
        "alleluia",
        "22025",
        "John Newton",
        "olorun",
    ];

    // Warm up SQLite page cache and statement cache evenly.
    for q in &queries {
        let _ = library.search_songs(q, 20, 0)?;
    }

    let mut samples = Vec::with_capacity(options.iterations);
    let mut amzing_samples = Vec::with_capacity(options.iterations / queries.len() + 2);
    for i in 0..options.iterations {
        let query = queries[i % queries.len()];
        let started = Instant::now();
        let results = library.search_songs(query, 20, 0)?;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        if results.hits.is_empty() {
            bail!("benchmark query '{query}' returned no results");
        }
        samples.push(elapsed_ms);
        if query == "amzing grace" {
            amzing_samples.push(elapsed_ms);
        }
    }

    samples.sort_by(f64::total_cmp);
    amzing_samples.sort_by(f64::total_cmp);

    let p95_ms = percentile(&samples, 95.0);
    let amzing_grace_p95_ms = percentile(&amzing_samples, 95.0);
    let pass = correctness_pass
        && p95_ms <= SEARCH_P95_BUDGET_MS
        && amzing_grace_p95_ms <= SEARCH_P95_BUDGET_MS;

    Ok(SearchReport {
        benchmark: "song-search-10k",
        os: std::env::consts::OS,
        songs: total_songs,
        iterations: options.iterations,
        correctness_pass,
        seed_ms,
        p50_ms: percentile(&samples, 50.0),
        p95_ms,
        p99_ms: percentile(&samples, 99.0),
        amzing_grace_p95_ms,
        max_ms: *samples.last().unwrap_or(&0.0),
        mean_ms: samples.iter().sum::<f64>() / samples.len().max(1) as f64,
        budget_p95_ms: SEARCH_P95_BUDGET_MS,
        pass,
    })
}
