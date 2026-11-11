//! SAFE-03 chaos test (M1-A14): with core frozen, **Clear everything** from the UI's emergency
//! lane clears the output within one frame.
use std::time::Duration;

use selah_ipc::{Client, Service};
use selah_proto::v1::{
    ClearEverything, Command, Query, RenderCommand, Resolution, Scene, Subscribe, TestPattern,
    command, envelope, event, query, render_command,
};
use selah_render::Compositor;
use selah_testkit::{Stack, StackOptions, freeze, test_actor};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clear_everything_works_with_core_frozen() {
    // safe: SAFE-03
    let stack = Stack::start(StackOptions::default());
    stack.wait_ready(Duration::from_secs(30)).await;
    stack.show_test_pattern(TestPattern::SmpteBars).await;
    let showing = stack.wait_ready(Duration::from_secs(30)).await;
    let core = stack.pid("core").await.expect("core pid");

    // Listen to the engine directly: core won't forward anything while it's frozen.
    let mut events = stack.engine().events();
    stack
        .engine()
        .request(
            envelope::Body::Query(Query {
                request: Some(query::Request::Subscribe(Subscribe {
                    event_names: vec!["frames".into()],
                })),
            }),
            Duration::from_secs(5),
        )
        .await
        .expect("engine subscription");
    let lane = Client::start(stack.endpoint(Service::Emergency), test_actor());
    lane.wait_connected(Duration::from_secs(5))
        .await
        .expect("emergency lane");

    freeze::freeze(core);
    // Core really is frozen: it doesn't answer.
    assert!(
        stack.core_status().await.is_none(),
        "core still answers after being frozen"
    );
    let reply = lane
        .request(
            envelope::Body::Command(Command {
                action: Some(command::Action::Render(RenderCommand {
                    action: Some(render_command::Action::ClearEverything(ClearEverything {})),
                })),
                ..Default::default()
            }),
            Duration::from_secs(2),
        )
        .await;

    let cleared = selah_render::cpu::CpuCompositor::default()
        .render(
            &Scene {
                revision: 0,
                test_pattern: TestPattern::Unspecified as i32,
                text: String::new(),
                cleared: true,
                resolution: Some(Resolution {
                    width: 1920,
                    height: 1080,
                }),
                ..Default::default()
            },
            1920,
            1080,
        )
        .unwrap()
        .checksum()
        .to_vec();
    let shown = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let message = events.recv().await.expect("engine events");
            if let Some(envelope::Body::Event(selah_proto::v1::Event {
                notification: Some(event::Notification::FrameShown(frame)),
            })) = message.body
                && frame.frame_checksum == cleared
            {
                return frame;
            }
        }
    })
    .await;
    freeze::thaw(core);

    assert!(reply.is_ok_and(|r| matches!(r.body, Some(envelope::Body::Reply(_)))));
    let shown = shown.expect("a cleared frame while core was frozen");
    assert_ne!(showing.frame_checksum, cleared);
    let latency = shown
        .monotonic_nanoseconds
        .saturating_sub(shown.command_received_nanoseconds);
    assert!(
        latency <= shown.frame_period_nanoseconds,
        "the clear took {:.2} ms (one frame is {:.2} ms)",
        latency as f64 / 1e6,
        shown.frame_period_nanoseconds as f64 / 1e6
    );
    stack.shutdown().await;
}
