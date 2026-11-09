//! M1-A15 on the real processes: Help ▸ Report a problem saves a bundle, and Help ▸ Diagnostics
//! shows the engine's details and writes a benchmark report (refused while live).
use std::time::Duration;

use selah_proto::v1::{
    Command, CreateSupportBundle, GetDiagnostics, Query, Reply, RunDiagnostics, TestPattern,
    command, envelope, query, reply,
};
use selah_testkit::{Stack, StackOptions};

const STARTUP: Duration = Duration::from_secs(30);

async fn ask(stack: &Stack, body: envelope::Body, timeout: Duration) -> envelope::Body {
    stack
        .core()
        .request(body, timeout)
        .await
        .expect("core answers")
        .body
        .expect("a reply")
}

fn command_body(action: command::Action) -> envelope::Body {
    envelope::Body::Command(Command {
        action: Some(action),
        ..Default::default()
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn report_a_problem_saves_a_bundle_with_the_version() {
    // sec: SEC-06
    let stack = Stack::start(StackOptions::default());
    let status = stack.wait_ready(STARTUP).await;
    let answer = ask(
        &stack,
        command_body(command::Action::CreateSupportBundle(CreateSupportBundle {})),
        Duration::from_secs(10),
    )
    .await;
    let envelope::Body::Reply(Reply {
        result: Some(reply::Result::SupportBundle(bundle)),
    }) = answer
    else {
        panic!("expected a support bundle, got {answer:?}");
    };
    let path = std::path::PathBuf::from(&bundle.path);
    assert!(path.is_file(), "{path:?}");
    assert!(path.starts_with(stack.home().join("support")));
    assert!(
        bundle
            .report_url
            .ends_with(&format!("version={}", status.version))
    );
    let entries = selah_core::support::read_entries(&std::fs::read(&path).unwrap()).unwrap();
    assert!(entries.iter().any(|(name, _)| name == "versions.json"));
    assert!(
        entries
            .iter()
            .any(|(name, _)| name.starts_with("logs/core")),
        "core's log is in the bundle"
    );
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn diagnostics_shows_the_engine_and_writes_a_benchmark_report() {
    let stack = Stack::start(StackOptions::default());
    stack.wait_ready(STARTUP).await;

    let answer = ask(
        &stack,
        envelope::Body::Query(Query {
            request: Some(query::Request::GetDiagnostics(GetDiagnostics {})),
        }),
        Duration::from_secs(5),
    )
    .await;
    let envelope::Body::Reply(Reply {
        result: Some(reply::Result::Diagnostics(diagnostics)),
    }) = answer
    else {
        panic!("expected diagnostics, got {answer:?}");
    };
    assert!(diagnostics.engine_connected);
    assert!(!diagnostics.renderer.is_empty());
    assert_eq!(
        diagnostics.output_resolution.map(|r| (r.width, r.height)),
        Some((1920, 1080))
    );

    // Refused while something is on the screens (SAFE-06).
    stack.show_test_pattern(TestPattern::SmpteBars).await;
    stack.wait_ready(STARTUP).await;
    let refused = ask(
        &stack,
        command_body(command::Action::RunDiagnostics(RunDiagnostics {
            quick: true,
        })),
        Duration::from_secs(5),
    )
    .await;
    assert!(
        matches!(&refused, envelope::Body::Error(e) if e.code == "SEL-DIAG-0001"),
        "{refused:?}"
    );

    stack
        .core()
        .request(
            command_body(command::Action::ClearOutputs(Default::default())),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    stack.wait_ready(STARTUP).await;
    let answer = ask(
        &stack,
        command_body(command::Action::RunDiagnostics(RunDiagnostics {
            quick: true,
        })),
        Duration::from_secs(240),
    )
    .await;
    let envelope::Body::Reply(Reply {
        result: Some(reply::Result::DiagnosticsReport(report)),
    }) = answer
    else {
        panic!("expected a diagnostics report, got {answer:?}");
    };
    let path = std::path::PathBuf::from(&report.path);
    assert!(path.is_file(), "{path:?}");
    assert!(report.ipc_p99_ms > 0.0);
    assert!(report.video_frames > 0);
    assert_eq!(report.video_clip, "h264-1080p60.mp4");
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["report"], "selah-diagnostics");
    assert!(saved["ipc"]["p99_ms"].as_f64().is_some());
    assert!(saved["video"]["frames"].as_u64().is_some());
    assert_eq!(saved["context"]["output"], "1920x1080");
    stack.shutdown().await;
}
