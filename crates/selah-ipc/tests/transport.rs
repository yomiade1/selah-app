//! Real-transport tests: named pipes on Windows, Unix sockets elsewhere (SEC-02).
use std::time::Duration;

use selah_ipc::conn::read_frame_async;
use selah_ipc::{Client, Connection, Endpoint, Listener, Service, reply_to};
use selah_proto::v1::{
    Actor, ActorOrigin, Envelope, Event, GetStatus, Heartbeat, Query, Reply, Status, envelope,
    event, query, reply,
};
use tokio::io::AsyncWriteExt;

fn actor(origin: ActorOrigin) -> Actor {
    Actor {
        user_id: "local-owner".into(),
        device_id: "test".into(),
        origin: origin as i32,
    }
}

fn unique_endpoint(service: Service) -> Endpoint {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    Endpoint::new(&format!("t{}x{}", std::process::id(), n), service)
}

fn status_query() -> envelope::Body {
    envelope::Body::Query(Query {
        request: Some(query::Request::GetStatus(GetStatus {})),
    })
}

/// A server that answers every GetStatus with a Status and keeps running after bad clients.
fn spawn_echo_server(mut listener: Listener) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Connections live in this task's JoinSet, so aborting the server (a "restart")
        // closes them all, as a process exit would.
        let mut connections = tokio::task::JoinSet::new();
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                return;
            };
            connections.spawn(async move {
                let Connection {
                    mut reader, sender, ..
                } = Connection::new(stream, peer);
                while let Ok(request) = reader.next().await {
                    let reply = reply_to(
                        &request,
                        actor(ActorOrigin::Core),
                        Reply {
                            result: Some(reply::Result::Status(Status {
                                connected: true,
                                safe_mode: false,
                                scene_revision: request.id,
                                ..Default::default()
                            })),
                        },
                    );
                    if sender.send(reply).await.is_err() {
                        break;
                    }
                }
            });
        }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ten_thousand_round_trips_keep_correlation() {
    let endpoint = unique_endpoint(Service::Core);
    let server = spawn_echo_server(Listener::bind(&endpoint).await.unwrap());
    let client = Client::start(endpoint, actor(ActorOrigin::Ui));
    client.wait_connected(Duration::from_secs(5)).await.unwrap();
    for _ in 0..10_000 {
        let reply = client
            .request(status_query(), Duration::from_secs(5))
            .await
            .unwrap();
        let Some(envelope::Body::Reply(Reply {
            result: Some(reply::Result::Status(status)),
        })) = reply.body
        else {
            panic!("unexpected reply {reply:?}");
        };
        assert_eq!(status.scene_revision, reply.correlation_id);
    }
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_and_oversized_frames_close_only_that_connection() {
    // sec: SEC-02
    let endpoint = unique_endpoint(Service::Core);
    let server = spawn_echo_server(Listener::bind(&endpoint).await.unwrap());
    let bad_inputs: Vec<Vec<u8>> = vec![
        ((16 * 1024 * 1024 + 1) as u32).to_le_bytes().to_vec(), // over the limit
        u32::MAX.to_le_bytes().to_vec(),
        0_u32.to_le_bytes().to_vec(),            // zero length
        [3, 0, 0, 0, 0x1a, 0x7f, 0x01].to_vec(), // malformed protobuf
        [2, 0, 0, 0, 0x08, 0x01].to_vec(),       // valid protobuf, no actor or body
    ];
    for bytes in bad_inputs {
        let (mut stream, _) = selah_ipc::transport::connect(&endpoint).await.unwrap();
        stream.write_all(&bytes).await.unwrap();
        stream.flush().await.unwrap();
        // The server must close the connection rather than answer or hang.
        let result =
            tokio::time::timeout(Duration::from_secs(5), read_frame_async(&mut stream)).await;
        assert!(
            matches!(result, Ok(Err(_))),
            "server answered a bad frame: {bytes:?}"
        );
    }
    // And it still serves well-behaved clients.
    let client = Client::start(endpoint, actor(ActorOrigin::Ui));
    client.wait_connected(Duration::from_secs(5)).await.unwrap();
    client
        .request(status_query(), Duration::from_secs(5))
        .await
        .unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_reconnects_after_the_server_restarts() {
    let endpoint = unique_endpoint(Service::Core);
    let server = spawn_echo_server(Listener::bind(&endpoint).await.unwrap());
    let client = Client::start(endpoint.clone(), actor(ActorOrigin::Ui));
    client.wait_connected(Duration::from_secs(5)).await.unwrap();
    let first = *client.state().borrow();
    client
        .request(status_query(), Duration::from_secs(5))
        .await
        .unwrap();

    server.abort();
    let _ = server.await;
    let mut state = client.state();
    tokio::time::timeout(Duration::from_secs(5), state.wait_for(|s| !s.connected))
        .await
        .expect("disconnect noticed")
        .unwrap();
    assert!(
        client
            .request(status_query(), Duration::from_millis(200))
            .await
            .is_err()
    );

    // Restart: the stale socket or pipe name must be reusable at once.
    let server = spawn_echo_server(Listener::bind(&endpoint).await.unwrap());
    let started = std::time::Instant::now();
    client.wait_connected(Duration::from_secs(2)).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(client.state().borrow().generation > first.generation);
    client
        .request(status_query(), Duration::from_secs(5))
        .await
        .unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsolicited_envelopes_arrive_as_events() {
    let endpoint = unique_endpoint(Service::Core);
    let mut listener = Listener::bind(&endpoint).await.unwrap();
    let server = tokio::spawn(async move {
        let (stream, peer) = listener.accept().await.unwrap();
        let connection = Connection::new(stream, peer);
        for n in 1..=3 {
            connection
                .sender
                .send(Envelope {
                    id: n,
                    correlation_id: 0,
                    actor: Some(actor(ActorOrigin::Core)),
                    body: Some(envelope::Body::Event(Event {
                        notification: Some(event::Notification::Heartbeat(Heartbeat {
                            monotonic_nanoseconds: n,
                        })),
                    })),
                })
                .await
                .unwrap();
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let client = Client::start(endpoint, actor(ActorOrigin::Ui));
    let mut events = client.events();
    for n in 1..=3 {
        let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.id, n);
    }
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_server_cannot_take_over_a_live_endpoint() {
    // sec: SEC-02 (no squatting or hijacking of a live endpoint)
    let endpoint = unique_endpoint(Service::Engine);
    let first = Listener::bind(&endpoint).await.unwrap();
    let server = spawn_echo_server(first);
    let second = Listener::bind(&endpoint).await;
    assert!(second.is_err(), "a second listener bound a live endpoint");
    server.abort();
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pipe_dacl_allows_only_the_current_user() {
    // sec: SEC-02
    let endpoint = unique_endpoint(Service::Core);
    let listener = Listener::bind(&endpoint).await.unwrap();
    let sid = selah_ipc::windows::current_user_sid().unwrap();
    let sddl = listener.dacl_sddl().unwrap();
    // Protected DACL ("P"), exactly one allow entry, for our SID.
    assert!(sddl.starts_with("D:P"), "DACL is not protected: {sddl}");
    assert_eq!(
        sddl.matches('(').count(),
        1,
        "unexpected extra ACEs: {sddl}"
    );
    assert!(
        sddl.contains(&format!(";;;{sid})")),
        "ACE is not for the current user: {sddl}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn socket_file_is_owner_only() {
    // sec: SEC-02
    use std::os::unix::fs::PermissionsExt;
    let endpoint = unique_endpoint(Service::Core);
    let _listener = Listener::bind(&endpoint).await.unwrap();
    let path = endpoint.socket_path().unwrap();
    let socket_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let dir_mode = std::fs::metadata(path.parent().unwrap())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(socket_mode, 0o600);
    assert_eq!(dir_mode, 0o700);
}
