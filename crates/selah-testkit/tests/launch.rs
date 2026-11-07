//! M1-A2: starting Selah launches engine, core and UI; Show test pattern puts SMPTE bars on
//! the output, and FrameShown comes no more than one frame after the engine got the command.
use std::time::Duration;

use selah_ipc::Service;
use selah_proto::v1::{
    Query, Resolution, Scene, Subscribe, TestPattern, envelope, event, query, reply,
};
use selah_render::Compositor;
use selah_testkit::{Stack, StackOptions, eventually};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn show_test_pattern_reaches_the_output_within_one_frame() {
    let stack = Stack::start(StackOptions {
        fake_ui: true,
        ..StackOptions::default()
    });
    stack.wait_ready(Duration::from_secs(30)).await;
    let (processes, _) = eventually(Duration::from_secs(10), "all three processes", || async {
        let status = stack.watchdog_status().await?;
        let running: Vec<_> = status
            .processes
            .iter()
            .filter(|p| p.running)
            .map(|p| p.role.clone())
            .collect();
        (running.len() == 3).then_some(running)
    })
    .await;
    assert_eq!(processes, ["engine", "core", "ui"]);

    let mut events = stack.core().events();
    let subscribed = stack
        .core()
        .request(
            envelope::Body::Query(Query {
                request: Some(query::Request::Subscribe(Subscribe {
                    event_names: vec!["frames".into()],
                })),
            }),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert!(matches!(
        subscribed.body,
        Some(envelope::Body::Reply(selah_proto::v1::Reply {
            result: Some(reply::Result::Status(_))
        }))
    ));

    stack.show_test_pattern(TestPattern::SmpteBars).await;
    let revision = stack.core_status().await.unwrap().scene_revision;
    let shown = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let message = events.recv().await.expect("event stream");
            if let Some(envelope::Body::Event(selah_proto::v1::Event {
                notification: Some(event::Notification::FrameShown(frame)),
            })) = message.body
                && frame.scene_revision == revision
            {
                return frame;
            }
        }
    })
    .await
    .expect("FrameShown for the test pattern");

    let latency = shown
        .monotonic_nanoseconds
        .saturating_sub(shown.command_received_nanoseconds);
    assert!(
        latency <= shown.frame_period_nanoseconds,
        "FrameShown came {:.2} ms after the command (one frame is {:.2} ms)",
        latency as f64 / 1e6,
        shown.frame_period_nanoseconds as f64 / 1e6
    );

    // The output really shows SMPTE bars: same pixels as the CPU reference.
    let reference = selah_render::cpu::CpuCompositor
        .render(
            &Scene {
                revision,
                test_pattern: TestPattern::SmpteBars as i32,
                text: String::new(),
                cleared: false,
                resolution: Some(Resolution {
                    width: 1920,
                    height: 1080,
                }),
            },
            1920,
            1080,
        )
        .unwrap();
    assert_eq!(shown.frame_checksum, reference.checksum().to_vec());

    // And the UI's preview ring shows them too: the first bar is 75% grey.
    let ring_path = selah_ipc::runtime_dir().unwrap().join(format!(
        "{}-preview-0.ring",
        stack.endpoint(Service::Engine).namespace()
    ));
    let (preview, _) = eventually(Duration::from_secs(5), "a bars preview frame", || async {
        let reader = selah_ipc::shm::RingReader::open(&ring_path).ok()?;
        let frame = reader.latest()?;
        (frame.pixels[0..3] == [191, 191, 191]).then_some(frame)
    })
    .await;
    assert_eq!((preview.width, preview.height), (480, 270));
    stack.shutdown().await;
}
