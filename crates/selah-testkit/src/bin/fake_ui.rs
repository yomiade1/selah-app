#![forbid(unsafe_code)]
//! `selah-fake-ui`: a stand-in for the Avalonia UI in Rust chaos tests. It does what the real
//! UI does at the process level: heartbeats as the UI, connects to core, subscribes, and
//! records each (re)connection in `$SELAH_HOME/fake-ui.txt` as `<pid> <generation>`.
use std::time::Duration;

use selah_ipc::{Client, Endpoint, Service};
use selah_proto::v1::{Actor, ActorOrigin, Query, Subscribe, envelope, query};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let actor = Actor {
        user_id: "local-owner".into(),
        device_id: "fake-ui".into(),
        origin: ActorOrigin::Ui as i32,
    };
    let _heartbeat = selah_runtime::heartbeat::spawn(actor.clone());
    let client = Client::start(Endpoint::for_current_user(Service::Core), actor);
    let marker = std::path::PathBuf::from(std::env::var("SELAH_HOME")?).join("fake-ui.txt");
    let mut state = client.state();
    loop {
        state.wait_for(|s| s.connected).await?;
        let generation = state.borrow().generation;
        let subscribed = client
            .request(
                envelope::Body::Query(Query {
                    request: Some(query::Request::Subscribe(Subscribe {
                        event_names: vec!["status".into()],
                    })),
                }),
                Duration::from_secs(2),
            )
            .await;
        if subscribed.is_ok() {
            selah_runtime::paths::write_atomic(
                &marker,
                format!("{} {generation}", std::process::id()).as_bytes(),
            )?;
        }
        state
            .wait_for(|s| !s.connected || s.generation != generation)
            .await?;
    }
}
