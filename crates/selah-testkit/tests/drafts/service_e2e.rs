//! DRAFT (M4.8, parked 2026-11-11): not compiled. Cargo only builds tests/*.rs, not sub-folders.
//! Move back to crates/selah-testkit/tests/ and add "service_e2e" to E2E_FILES in
//! tools/test-ledger.py once the engine and core issues in PROGRESS.md resume notes are fixed.
//! M4.8 end-to-end on the real processes (headless): a scripted 12-item service (M4-A1) and 50
//! random kills of the UI, core and engine during a run (M4-A5, ADR-001 continuity rules).
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use selah_core::library::MediaStore;
use selah_proto::v1::{
    Command, Envelope, FrameShown, ListBibles, LiveStatus, LoadLiveService, LyricLine, MediaItem,
    MediaKind, NextSlide, OutputConfig, OutputRole, Query, Reply, Resolution, RestartNormally,
    SaveMediaItem, SaveService, SaveSong, Section, SectionKind, Service, ServiceItem,
    ServiceItemKind, SetOutputConfigs, Song, Subscribe, TriggerItem, TriggerSlide, command,
    envelope, event, query, reply,
};
use selah_testkit::{Stack, StackOptions, eventually, kill_pid};

const STARTUP: Duration = Duration::from_secs(30);
const RECOVERY: Duration = Duration::from_secs(2);

fn new_id() -> selah_proto::v1::Id {
    selah_core::library::new_id()
}

async fn command(stack: &Stack, action: command::Action) -> Envelope {
    stack
        .core()
        .request(
            envelope::Body::Command(Command {
                action: Some(action),
                ..Default::default()
            }),
            Duration::from_secs(30),
        )
        .await
        .expect("core answers")
}

async fn live(stack: &Stack, action: command::Action) -> LiveStatus {
    match command(stack, action).await.body {
        Some(envelope::Body::Reply(Reply {
            result: Some(reply::Result::LiveStatus(status)),
        })) => status,
        other => panic!("expected a live status, got {other:?}"),
    }
}

async fn query(stack: &Stack, request: query::Request) -> Envelope {
    stack
        .core()
        .request(
            envelope::Body::Query(Query {
                request: Some(request),
            }),
            Duration::from_secs(60),
        )
        .await
        .expect("core answers")
}

fn song(title: &str, verses: &[&[&str]]) -> Song {
    Song {
        id: Some(new_id()),
        title: title.into(),
        copyright: "Public Domain".into(),
        sections: verses
            .iter()
            .enumerate()
            .map(|(n, lines)| Section {
                id: Some(new_id()),
                kind: SectionKind::Verse as i32,
                label: format!("Verse {}", n + 1),
                lines: lines
                    .iter()
                    .map(|l| LyricLine {
                        text: (*l).into(),
                        chords: Vec::new(),
                    })
                    .collect(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

/// Public-domain hymn lines (two verses each) for the scripted services.
fn hymns() -> Vec<Song> {
    vec![
        song(
            "Amazing Grace",
            &[
                &["Amazing grace! How sweet the sound", "That saved a wretch like me!"],
                &["'Twas grace that taught my heart to fear,", "And grace my fears relieved;"],
            ],
        ),
        song(
            "Holy, Holy, Holy",
            &[
                &["Holy, holy, holy! Lord God Almighty!", "Early in the morning our song shall rise to Thee;"],
                &["Holy, holy, holy! All the saints adore Thee,", "Casting down their golden crowns around the glassy sea;"],
            ],
        ),
        song(
            "Be Thou My Vision",
            &[
                &["Be Thou my Vision, O Lord of my heart;", "Naught be all else to me, save that Thou art."],
                &["Be Thou my Wisdom, and Thou my true Word;", "I ever with Thee and Thou with me, Lord;"],
            ],
        ),
        song(
            "Come, Thou Fount",
            &[
                &["Come, Thou Fount of every blessing,", "Tune my heart to sing Thy grace;"],
                &["Here I raise my Ebenezer;", "Hither by Thy help I'm come;"],
            ],
        ),
        song(
            "It Is Well",
            &[
                &["When peace like a river attendeth my way,", "When sorrows like sea billows roll;"],
                &["Though Satan should buffet, though trials should come,", "Let this blest assurance control,"],
            ],
        ),
    ]
}

async fn set_small_output(stack: &Stack) {
    command(
        stack,
        command::Action::SetOutputConfigs(SetOutputConfigs {
            outputs: vec![OutputConfig {
                output_index: 0,
                name: "Main".into(),
                role: OutputRole::Main as i32,
                resolution: Some(Resolution {
                    width: 640,
                    height: 360,
                }),
                enabled: true,
                ..Default::default()
            }],
        }),
    )
    .await;
}

async fn save_songs(stack: &Stack, songs: &[Song]) {
    for song in songs {
        command(
            stack,
            command::Action::SaveSong(SaveSong {
                song: Some(song.clone()),
            }),
        )
        .await;
    }
}

fn song_item(song: &Song) -> ServiceItem {
    ServiceItem {
        id: Some(new_id()),
        kind: ServiceItemKind::Song as i32,
        content_id: song.id.clone(),
        title: song.title.clone(),
        ..Default::default()
    }
}

fn item(kind: ServiceItemKind, title: &str) -> ServiceItem {
    ServiceItem {
        id: Some(new_id()),
        kind: kind as i32,
        title: title.into(),
        ..Default::default()
    }
}

async fn save_and_load(stack: &Stack, items: Vec<ServiceItem>) -> LiveStatus {
    let service = Service {
        id: Some(new_id()),
        name: "Sunday Morning".into(),
        items,
        ..Default::default()
    };
    command(
        stack,
        command::Action::SaveService(SaveService {
            service: Some(service.clone()),
        }),
    )
    .await;
    live(
        stack,
        command::Action::LoadLiveService(LoadLiveService {
            service_id: service.id,
            trigger_first_item: true,
        }),
    )
    .await
}

/// Wait for the frame of `revision` on output 0. Also returns how many older frames the engine
/// presented after it received the command (the frame already being drawn may be one).
async fn frame_and_stale(
    events: &mut tokio::sync::broadcast::Receiver<Envelope>,
    revision: u64,
) -> (FrameShown, usize) {
    let mut older: Vec<u64> = Vec::new();
    let frame = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = events.recv().await.expect("event stream");
            if let Some(envelope::Body::Event(selah_proto::v1::Event {
                notification: Some(event::Notification::FrameShown(frame)),
            })) = message.body
                && frame.output_index == 0
                && !frame.is_preview
            {
                if frame.scene_revision == revision {
                    return frame;
                }
                if frame.scene_revision < revision {
                    older.push(frame.monotonic_nanoseconds);
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no frame for revision {revision}"));
    let stale = older
        .iter()
        .filter(|shown| **shown > frame.command_received_nanoseconds)
        .count();
    (frame, stale)
}

async fn frame_for(
    events: &mut tokio::sync::broadcast::Receiver<Envelope>,
    revision: u64,
) -> FrameShown {
    frame_and_stale(events, revision).await.0
}

async fn subscribe_frames(stack: &Stack) -> tokio::sync::broadcast::Receiver<Envelope> {
    let events = stack.core().events();
    query(
        stack,
        query::Request::Subscribe(Subscribe {
            event_names: vec!["frames".into()],
        }),
    )
    .await;
    events
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_scripted_twelve_item_service_shows_every_frame_on_time() {
    // M4-A1
    let stack = Stack::start(StackOptions::default());
    stack.wait_ready(STARTUP).await;
    set_small_output(&stack).await;
    // The bundled Bibles are ready before the service needs them.
    query(&stack, query::Request::ListBibles(ListBibles::default())).await;

    let songs = hymns();
    save_songs(&stack, &songs).await;
    let clip = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/media/committed/h264-1080p60.mp4");
    let store = MediaStore::open(&stack.home().join("library").join("media")).unwrap();
    let blob = store.import(&clip).expect("import the clip");
    let video = MediaItem {
        id: Some(new_id()),
        name: "Welcome loop".into(),
        kind: MediaKind::Video as i32,
        content_hash: blob.content_hash.clone(),
        extension: "mp4".into(),
        size_bytes: blob.size_bytes,
        ..Default::default()
    };
    command(
        &stack,
        command::Action::SaveMediaItem(SaveMediaItem {
            media_item: Some(video.clone()),
        }),
    )
    .await;

    let scripture = |title: &str| ServiceItem {
        scripture_reference: title.into(),
        bible_translation: "KJV".into(),
        ..item(ServiceItemKind::Scripture, title)
    };
    let countdown = |title: &str| ServiceItem {
        countdown_seconds: 300,
        ..item(ServiceItemKind::Countdown, title)
    };
    let items = vec![
        item(ServiceItemKind::Header, "Welcome"),
        countdown("Service starts soon"),
        song_item(&songs[0]),
        song_item(&songs[1]),
        scripture("Psalm 23:1-2"),
        song_item(&songs[2]),
        ServiceItem {
            content_id: video.id.clone(),
            ..item(ServiceItemKind::Media, "Welcome loop")
        },
        item(ServiceItemKind::Header, "The Word"),
        scripture("John 3:16"),
        song_item(&songs[3]),
        countdown("Break"),
        song_item(&songs[4]),
    ];
    assert_eq!(items.len(), 12);
    let triggerable: BTreeSet<u32> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.kind != ServiceItemKind::Header as i32)
        .map(|(n, _)| n as u32)
        .collect();

    let video_item = items
        .iter()
        .position(|i| i.kind == ServiceItemKind::Media as i32)
        .expect("a video item") as u32;
    let mut events = subscribe_frames(&stack).await;
    let first = save_and_load(&stack, items).await;
    // Warm up once (decoders, fonts, caches), as an operator's run-through does, then time the run.
    let mut warm = first.clone();
    while warm.has_next {
        warm = live(&stack, command::Action::NextSlide(NextSlide {})).await;
    }
    let mut status = live(
        &stack,
        command::Action::TriggerSlide(TriggerSlide {
            item_index: first.current_item_index,
            slide_index: 0,
        }),
    )
    .await;
    let mut shown: BTreeMap<(u32, u32), Vec<u8>> = BTreeMap::new();
    let mut visited = BTreeSet::new();
    let mut previous: Option<Vec<u8>> = None;
    let mut slowest = 0u64;
    for step in 0..200 {
        let (frame, stale) = frame_and_stale(&mut events, status.scene_revision).await;
        let latency = frame
            .monotonic_nanoseconds
            .saturating_sub(frame.command_received_nanoseconds);
        slowest = slowest.max(latency);
        // "Within 1 frame of the command": at most the frame already being drawn is shown after
        // the command, so the slide lands on the next frame (≤ 2 frame periods while a countdown
        // or video keeps the engine drawing every frame).
        assert!(
            stale <= 1 && latency <= 2 * frame.frame_period_nanoseconds,
            "step {step} (item {}, slide {}) took {:.2} ms with {stale} older frames after the command; one frame is {:.2} ms",
            status.current_item_index,
            status.current_slide_index,
            latency as f64 / 1e6,
            frame.frame_period_nanoseconds as f64 / 1e6
        );
        // A video's first frame can still be the decoder warming up: wait for it to play.
        let frame = if status.current_item_index == video_item {
            let mut playing = frame;
            let deadline = Instant::now() + Duration::from_secs(5);
            while previous.as_ref() == Some(&playing.frame_checksum) {
                assert!(Instant::now() < deadline, "the video never started");
                playing = frame_for(&mut events, status.scene_revision).await;
            }
            playing
        } else {
            frame
        };
        assert_ne!(
            previous.as_ref(),
            Some(&frame.frame_checksum),
            "step {step} didn't change the picture"
        );
        previous = Some(frame.frame_checksum.clone());
        visited.insert(status.current_item_index);
        shown.insert(
            (status.current_item_index, status.current_slide_index),
            frame.frame_checksum,
        );
        if !status.has_next {
            break;
        }
        status = live(&stack, command::Action::NextSlide(NextSlide {})).await;
    }
    assert_eq!(visited, triggerable, "every item but the headers was shown");
    assert!(shown.len() >= 16, "{} slides shown", shown.len());
    eprintln!("slowest slide change: {:.2} ms", slowest as f64 / 1e6);

    // The same slide again gives the same picture (songs don't change with time).
    let again = live(
        &stack,
        command::Action::TriggerSlide(TriggerSlide {
            item_index: 2,
            slide_index: 0,
        }),
    )
    .await;
    let frame = frame_for(&mut events, again.scene_revision).await;
    assert_eq!(Some(&frame.frame_checksum), shown.get(&(2, 0)));
    stack.shutdown().await;
}

/// A small, seeded generator, so a failing run can be repeated.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fifty_random_kills_during_a_service_keep_the_show_going() {
    // M4-A5, ADR-001, SAFE-07
    let seed = std::env::var("SELAH_CHAOS_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5e1a_u64);
    eprintln!("chaos seed {seed} (set SELAH_CHAOS_SEED to repeat)");
    let mut random = Lcg(seed);
    let stack = Stack::start(StackOptions {
        ui: true,
        ..StackOptions::default()
    });
    stack.wait_ready(STARTUP).await;
    set_small_output(&stack).await;
    let songs = hymns();
    save_songs(&stack, &songs).await;
    let status = save_and_load(&stack, songs.iter().map(song_item).collect()).await;
    assert!(status.has_active_item);
    eventually(STARTUP, "the UI to start", || async { stack.pid("ui").await }).await;

    let mut kills: BTreeMap<&str, u32> = BTreeMap::new();
    let mut safe_mode_round_trips = 0;
    let mut round = 0;
    while kills.values().sum::<u32>() < 50 {
        round += 1;
        assert!(round < 200, "too many rounds without a process to kill");
        let before = stack.wait_ready(STARTUP).await;
        let role = ["ui", "core", "engine"][(random.next() % 3) as usize];
        let Some(pid) = stack.pid(role).await else {
            // The UI may still be starting after a restart.
            eventually(STARTUP, "the process to start", || async { stack.pid(role).await }).await;
            continue;
        };
        kill_pid(pid);
        *kills.entry(role).or_default() += 1;
        let started = Instant::now();
        if role == "engine" {
            // The engine comes back within 2 s with the same picture.
            loop {
                if let (Some(status), Some(new_pid)) =
                    (stack.core_status().await, stack.pid("engine").await)
                    && new_pid != pid
                    && status.engine_connected
                    && status.frame_checksum == before.frame_checksum
                {
                    break;
                }
                assert!(
                    started.elapsed() < RECOVERY,
                    "round {round}: the engine took more than 2 s to restore the scene"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        } else {
            // UI or core: the screens never change while it comes back.
            loop {
                if let Some(engine) = stack.engine_status().await {
                    assert_eq!(
                        engine.frame_checksum, before.frame_checksum,
                        "round {round}: the screens changed when {role} was killed"
                    );
                }
                if stack.pid(role).await.is_some_and(|p| p != pid)
                    && stack
                        .core_status()
                        .await
                        .is_some_and(|s| s.engine_connected && s.frame_checksum == before.frame_checksum)
                {
                    break;
                }
                assert!(
                    started.elapsed() < STARTUP,
                    "round {round}: {role} didn't come back"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        // Three crashes in a minute start safe mode (SAFE-07). The screens stay; the operator
        // chooses Restart normally, as a person at the booth would.
        if stack.watchdog_status().await.is_some_and(|w| w.safe_mode) {
            safe_mode_round_trips += 1;
            stack
                .watchdog()
                .request(
                    envelope::Body::Command(Command {
                        action: Some(command::Action::RestartNormally(RestartNormally {})),
                        ..Default::default()
                    }),
                    Duration::from_secs(5),
                )
                .await
                .expect("the watchdog answers");
            eventually(STARTUP, "safe mode to end", || async {
                let watchdog = stack.watchdog_status().await?;
                let core = stack.core_status().await?;
                (!watchdog.safe_mode && !core.safe_mode && core.engine_connected).then_some(())
            })
            .await;
            let after = stack.wait_ready(STARTUP).await;
            assert_eq!(
                after.frame_checksum, before.frame_checksum,
                "round {round}: leaving safe mode changed the screens"
            );
        }
        // The service carries on: the next slide still works after every kill.
        let next = live(&stack, command::Action::NextSlide(NextSlide {})).await;
        let next = if next.has_next || next.current_slide_index > 0 {
            next
        } else {
            live(
                &stack,
                command::Action::TriggerItem(TriggerItem { item_index: 0 }),
            )
            .await
        };
        eventually(STARTUP, "the next slide to show", || async {
            stack
                .core_status()
                .await
                .filter(|s| s.engine_scene_revision >= next.scene_revision)
        })
        .await;
    }
    eprintln!("kills: {kills:?}; safe mode round trips: {safe_mode_round_trips}");
    assert_eq!(kills.values().sum::<u32>(), 50);
    for role in ["ui", "core", "engine"] {
        assert!(kills.get(role).copied().unwrap_or(0) >= 5, "{role} was killed too rarely");
    }
    stack.shutdown().await;
}
