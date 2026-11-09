//! Safety and privacy on the real processes (M1.12): leaving safe mode keeps the screens
//! (SAFE-07), and at default settings no Selah process opens a network socket (SEC-13).
use std::time::Duration;

use selah_proto::v1::{Command, RestartNormally, TestPattern, command, envelope};
use selah_testkit::{Stack, StackOptions, eventually};

const STARTUP: Duration = Duration::from_secs(30);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_normally_leaves_safe_mode_and_keeps_the_screens() {
    // safe: SAFE-07
    let stack = Stack::start(StackOptions {
        safe_mode: true,
        ..StackOptions::default()
    });
    stack.wait_ready(STARTUP).await;
    assert!(stack.core_status().await.unwrap().safe_mode);
    stack.show_test_pattern(TestPattern::SmpteBars).await;
    let before = stack.wait_ready(STARTUP).await;
    let engine = stack.pid("engine").await.expect("engine pid");
    let core = stack.pid("core").await.expect("core pid");

    let reply = stack
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
    assert!(
        matches!(reply.body, Some(envelope::Body::Reply(_))),
        "{reply:?}"
    );

    eventually(
        Duration::from_secs(10),
        "core back without safe mode",
        || async {
            let watchdog = stack.watchdog_status().await?;
            let status = stack.core_status().await?;
            let new_core = stack.pid("core").await?;
            (!watchdog.safe_mode
                && !status.safe_mode
                && new_core != core
                && status.engine_connected)
                .then_some(())
        },
    )
    .await;
    // The engine kept running and kept showing the same frame.
    assert_eq!(stack.pid("engine").await, Some(engine));
    let after = stack.wait_ready(STARTUP).await;
    assert_eq!(after.frame_checksum, before.frame_checksum);
    stack.shutdown().await;
}

/// Every network socket (TCP or UDP, any state) owned by these processes. Local pipes and Unix
/// sockets aren't network sockets and aren't listed.
fn network_sockets(pids: &[u32]) -> Vec<String> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("netstat")
            .arg("-ano")
            .output()
            .expect("netstat");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| {
                let fields: Vec<&str> = line.split_whitespace().collect();
                matches!(fields.first(), Some(&"TCP") | Some(&"UDP"))
                    && fields
                        .last()
                        .and_then(|pid| pid.parse::<u32>().ok())
                        .is_some_and(|pid| pids.contains(&pid))
            })
            .map(str::to_owned)
            .collect()
    }
    #[cfg(target_os = "linux")]
    {
        let mut found = Vec::new();
        for pid in pids {
            let mut inodes = Vec::new();
            if let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) {
                for entry in entries.flatten() {
                    if let Ok(target) = std::fs::read_link(entry.path()) {
                        let target = target.to_string_lossy().into_owned();
                        if let Some(inode) = target
                            .strip_prefix("socket:[")
                            .and_then(|rest| rest.strip_suffix(']'))
                        {
                            inodes.push(inode.to_owned());
                        }
                    }
                }
            }
            for table in ["tcp", "tcp6", "udp", "udp6"] {
                let text =
                    std::fs::read_to_string(format!("/proc/{pid}/net/{table}")).unwrap_or_default();
                for line in text.lines().skip(1) {
                    let fields: Vec<&str> = line.split_whitespace().collect();
                    if fields
                        .get(9)
                        .is_some_and(|inode| inodes.iter().any(|i| i == inode))
                    {
                        found.push(format!("pid {pid} {table}: {line}"));
                    }
                }
            }
        }
        found
    }
    #[cfg(target_os = "macos")]
    {
        let list = pids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let output = std::process::Command::new("lsof")
            .args(["-nP", "-a", "-p", &list, "-i"])
            .output()
            .expect("lsof");
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .skip(1)
            .map(str::to_owned)
            .collect()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn default_settings_make_no_outgoing_connections() {
    // sec: SEC-13. The whole stack, UI included, at default settings.
    let stack = Stack::start(StackOptions {
        ui: true,
        ..StackOptions::default()
    });
    stack.wait_ready(STARTUP).await;
    let (_, _) = eventually(STARTUP, "the UI connected", || async {
        std::fs::metadata(stack.home().join("ui.txt")).ok()
    })
    .await;
    stack.show_test_pattern(TestPattern::SmpteBars).await;
    stack.wait_ready(STARTUP).await;
    // Give anything that would phone home time to try.
    tokio::time::sleep(Duration::from_secs(3)).await;

    let mut pids = vec![stack.watchdog_pid()];
    for role in ["engine", "core", "ui"] {
        pids.push(
            stack
                .pid(role)
                .await
                .unwrap_or_else(|| panic!("{role} pid")),
        );
    }
    let sockets = network_sockets(&pids);
    stack.shutdown().await;
    assert!(
        sockets.is_empty(),
        "Selah opened network sockets at default settings:\n{}",
        sockets.join("\n")
    );
}
