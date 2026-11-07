//! "The show must go on" (ADR-001): kill each process and check the screens and recovery.
//! These are M1-A3, M1-A4, M1-A5 and the SAFE-07 crash-loop check, run on the real binaries.
use std::time::Duration;

use selah_proto::v1::TestPattern;
use selah_testkit::{Stack, StackOptions, eventually, kill_pid};

const RECOVERY: Duration = Duration::from_secs(2);
const STARTUP: Duration = Duration::from_secs(30);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn engine_killed_is_restarted_and_the_scene_restored() {
    // M1-A4
    let stack = Stack::start(StackOptions::default());
    stack.wait_ready(STARTUP).await;
    stack.show_test_pattern(TestPattern::SmpteBars).await;
    let before = stack.wait_ready(STARTUP).await;
    let engine = stack.pid("engine").await.expect("engine pid");

    kill_pid(engine);
    let (_, elapsed) = eventually(RECOVERY * 3, "engine restored", || async {
        let status = stack.core_status().await?;
        let pid = stack.pid("engine").await?;
        (pid != engine
            && status.engine_connected
            && status.frame_checksum == before.frame_checksum
            && status.engine_scene_revision == before.scene_revision)
            .then_some(())
    })
    .await;
    assert!(elapsed < RECOVERY, "engine took {elapsed:?} to come back");
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn core_killed_leaves_the_last_frame_and_resynchronises() {
    // M1-A5
    let stack = Stack::start(StackOptions::default());
    stack.wait_ready(STARTUP).await;
    stack.show_test_pattern(TestPattern::Grid).await;
    let before = stack.wait_ready(STARTUP).await;
    let core = stack.pid("core").await.expect("core pid");

    kill_pid(core);
    let started = std::time::Instant::now();
    // While core is down and coming back, the engine keeps the same frame.
    let mut saw_core_down = false;
    loop {
        let engine = stack.engine_status().await.expect("engine stays up");
        assert_eq!(
            engine.frame_checksum, before.frame_checksum,
            "the screens changed"
        );
        match stack.core_status().await {
            Some(status) if saw_core_down || stack.pid("core").await != Some(core) => {
                if status.engine_connected
                    && status.scene_revision == before.scene_revision
                    && status.engine_scene_revision == before.scene_revision
                    && status.frame_checksum == before.frame_checksum
                {
                    break;
                }
            }
            Some(_) => {}
            None => saw_core_down = true,
        }
        assert!(
            started.elapsed() < RECOVERY,
            "core didn't resynchronise within 2 s"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ui_killed_changes_nothing_on_screen_and_comes_back() {
    // M1-A3, with the real UI (headless)
    let stack = Stack::start(StackOptions {
        ui: true,
        ..StackOptions::default()
    });
    stack.wait_ready(STARTUP).await;
    stack.show_test_pattern(TestPattern::SmpteBars).await;
    let before = stack.wait_ready(STARTUP).await;
    let marker = stack.home().join("ui.txt");
    let (ui, _) = eventually(STARTUP, "the UI to connect", || async {
        let text = std::fs::read_to_string(&marker).ok()?;
        text.split_whitespace().next()?.parse::<u32>().ok()
    })
    .await;

    kill_pid(ui);
    let (_, elapsed) = eventually(RECOVERY * 3, "the UI to reconnect", || async {
        let engine = stack.engine_status().await?;
        assert_eq!(
            engine.frame_checksum, before.frame_checksum,
            "the screens changed"
        );
        let text = std::fs::read_to_string(&marker).ok()?;
        let pid: u32 = text.split_whitespace().next()?.parse().ok()?;
        (pid != ui).then_some(())
    })
    .await;
    assert!(elapsed < RECOVERY, "the UI took {elapsed:?} to reconnect");
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn three_crashes_in_a_minute_start_safe_mode() {
    // safe: SAFE-07
    let stack = Stack::start(StackOptions::default());
    stack.wait_ready(STARTUP).await;
    assert!(!stack.core_status().await.unwrap().safe_mode);
    for _ in 0..3 {
        let core = stack.pid("core").await.expect("core pid");
        kill_pid(core);
        eventually(RECOVERY * 3, "core restarted", || async {
            let pid = stack.pid("core").await?;
            let status = stack.core_status().await?;
            (pid != core).then_some(status)
        })
        .await;
    }
    let (status, _) = eventually(RECOVERY * 3, "safe mode", || async {
        let watchdog = stack.watchdog_status().await?;
        let core = stack.core_status().await?;
        (watchdog.safe_mode && core.safe_mode).then_some(watchdog)
    })
    .await;
    assert_eq!(status.safe_mode_reason, "crashes");
    stack.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_watchdog_exits_instead_of_starting_twice() {
    let stack = Stack::start(StackOptions::default());
    stack.wait_ready(STARTUP).await;
    let status = std::process::Command::new(selah_testkit::bin("selah-watchdog"))
        .args(["--headless", "--no-ui"])
        .env("SELAH_IPC_NAMESPACE", &stack.namespace)
        .env("SELAH_HOME", stack.home())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(selah_watchdog_exit_already_running()));
    stack.shutdown().await;
}

fn selah_watchdog_exit_already_running() -> i32 {
    3
}
