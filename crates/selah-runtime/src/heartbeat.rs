//! Children tell the watchdog they're alive every 250 ms (ARCHITECTURE §2).
use std::time::{Duration, Instant};

use selah_ipc::{Client, Endpoint, Service};
use selah_proto::v1::{Actor, Event, Heartbeat, envelope, event};

pub const INTERVAL: Duration = Duration::from_millis(250);

/// Send heartbeats to the watchdog until the task is dropped. A missing watchdog is fine
/// (development runs without one); the client keeps trying to connect.
pub fn spawn(actor: Actor) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = Client::start(Endpoint::for_current_user(Service::Watchdog), actor);
        let started = Instant::now();
        let mut ticker = tokio::time::interval(INTERVAL);
        // Tests only: leave when the watchdog goes, so no test leaves processes behind. In
        // normal use the processes keep running (and the screens keep showing) without it.
        let exit_with_watchdog = std::env::var("SELAH_EXIT_WITH_WATCHDOG").as_deref() == Ok("1");
        let mut was_connected = false;
        loop {
            ticker.tick().await;
            if exit_with_watchdog {
                let connected = client.is_connected();
                if (was_connected && !connected)
                    || (!was_connected && started.elapsed() > Duration::from_secs(10))
                {
                    tracing::info!("watchdog has gone; exiting (SELAH_EXIT_WITH_WATCHDOG)");
                    std::process::exit(0);
                }
                was_connected |= connected;
            }
            if client.is_connected() {
                let nanos = started.elapsed().as_nanos() as u64;
                let _ = client
                    .send(envelope::Body::Event(Event {
                        notification: Some(event::Notification::Heartbeat(Heartbeat {
                            monotonic_nanoseconds: nanos,
                        })),
                    }))
                    .await;
            }
        }
    })
}
