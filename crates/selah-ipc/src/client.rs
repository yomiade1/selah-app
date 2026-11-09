//! A reconnecting client: request/reply with correlation, events, and connection state.
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use selah_proto::v1::{Actor, Envelope, envelope};
use tokio::sync::{Mutex, Notify, broadcast, oneshot, watch};

use crate::conn::{Connection, FrameSender};
use crate::endpoint::Endpoint;
use crate::frame::reconnect_delay;
use crate::transport;

/// Whether the client currently has a live connection. `generation` goes up on every new
/// connection, so callers can resubscribe after a reconnect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionState {
    pub connected: bool,
    pub generation: u64,
}

struct Inner {
    actor: Actor,
    sender: Mutex<Option<FrameSender>>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Envelope>>>,
    events: broadcast::Sender<Envelope>,
    state: watch::Sender<ConnectionState>,
    next_id: AtomicU64,
    shutdown: Notify,
}

/// Connects in the background and keeps reconnecting (25 ms doubling to 500 ms) until dropped.
pub struct Client {
    inner: Arc<Inner>,
}

impl Drop for Client {
    fn drop(&mut self) {
        self.inner.shutdown.notify_waiters();
        self.inner.shutdown.notify_one();
    }
}

impl Client {
    /// Start a client for `endpoint`. Every envelope it sends carries `actor`.
    pub fn start(endpoint: Endpoint, actor: Actor) -> Self {
        let (events, _) = broadcast::channel(1024);
        let (state, _) = watch::channel(ConnectionState {
            connected: false,
            generation: 0,
        });
        let inner = Arc::new(Inner {
            actor,
            sender: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            events,
            state,
            next_id: AtomicU64::new(initial_envelope_id()),
            shutdown: Notify::new(),
        });
        tokio::spawn(supervise(endpoint, Arc::downgrade(&inner)));
        Self { inner }
    }

    pub fn state(&self) -> watch::Receiver<ConnectionState> {
        self.inner.state.subscribe()
    }

    pub fn is_connected(&self) -> bool {
        self.inner.state.borrow().connected
    }

    /// Wait until connected, or fail after `timeout`.
    pub async fn wait_connected(&self, timeout: Duration) -> io::Result<()> {
        let mut state = self.state();
        tokio::time::timeout(timeout, state.wait_for(|s| s.connected))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "not connected in time"))?
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "client stopped"))?;
        Ok(())
    }

    /// Events (and any other unsolicited envelopes) from the server.
    pub fn events(&self) -> broadcast::Receiver<Envelope> {
        self.inner.events.subscribe()
    }

    fn envelope(&self, body: envelope::Body) -> Envelope {
        Envelope {
            id: self.inner.next_id.fetch_add(1, Ordering::Relaxed),
            correlation_id: 0,
            actor: Some(self.inner.actor.clone()),
            body: Some(body),
        }
    }

    /// Send a command or query and wait for the reply (or error) that correlates with it.
    pub async fn request(&self, body: envelope::Body, timeout: Duration) -> io::Result<Envelope> {
        let message = self.envelope(body);
        let id = message.id;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.inner.pending.lock().await.insert(id, reply_tx);
        let sent = self.send_envelope(message).await;
        if let Err(error) = sent {
            self.inner.pending.lock().await.remove(&id);
            return Err(error);
        }
        match tokio::time::timeout(timeout, reply_rx).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) => Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "the connection closed before the reply arrived",
            )),
            Err(_) => {
                self.inner.pending.lock().await.remove(&id);
                Err(io::Error::new(io::ErrorKind::TimedOut, "no reply in time"))
            }
        }
    }

    /// Send without waiting for a reply.
    pub async fn send(&self, body: envelope::Body) -> io::Result<()> {
        let message = self.envelope(body);
        self.send_envelope(message).await
    }

    async fn send_envelope(&self, message: Envelope) -> io::Result<()> {
        let sender = self.inner.sender.lock().await.clone();
        match sender {
            Some(sender) => sender.send(message).await,
            None => Err(io::Error::new(io::ErrorKind::NotConnected, "not connected")),
        }
    }
}

fn initial_envelope_id() -> u64 {
    static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);
    // Keep values positive within signed 64-bit SQLite INTEGER while avoiding cross-run collisions.
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(1_700_000_000_000)
        .saturating_sub(1_700_000_000_000)
        & 0x1FF_FFFF_FFFF;
    let tag = ((std::process::id() as u64) ^ NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed)) & 0xFFF;
    ((ms << 22) | (tag << 10) | 1).max(1)
}

async fn supervise(endpoint: Endpoint, weak: std::sync::Weak<Inner>) {
    let mut attempt = 0_u32;
    let mut generation = 0_u64;
    loop {
        let Some(inner) = weak.upgrade() else { return };
        let connected = tokio::select! {
            result = transport::connect(&endpoint) => result,
            _ = inner.shutdown.notified() => return,
        };
        match connected {
            Ok((stream, peer)) => {
                attempt = 0;
                generation += 1;
                let Connection {
                    mut reader, sender, ..
                } = Connection::new(stream, peer);
                *inner.sender.lock().await = Some(sender);
                inner.state.send_replace(ConnectionState {
                    connected: true,
                    generation,
                });
                loop {
                    let frame = tokio::select! {
                        frame = reader.next() => frame,
                        _ = inner.shutdown.notified() => return,
                    };
                    match frame {
                        Ok(message) => dispatch(&inner, message).await,
                        Err(error) => {
                            tracing::debug!(%error, "IPC connection lost");
                            break;
                        }
                    }
                }
                *inner.sender.lock().await = None;
                // Waiting requests fail at once rather than at their timeout.
                inner.pending.lock().await.clear();
                inner.state.send_replace(ConnectionState {
                    connected: false,
                    generation,
                });
            }
            Err(error) => {
                tracing::trace!(%error, "IPC connect failed");
            }
        }
        let delay = reconnect_delay(attempt);
        attempt = attempt.saturating_add(1);
        tokio::select! {
            _ = tokio::time::sleep(delay) => {},
            _ = inner.shutdown.notified() => return,
        }
    }
}

async fn dispatch(inner: &Inner, message: Envelope) {
    let is_reply = matches!(
        message.body,
        Some(envelope::Body::Reply(_)) | Some(envelope::Body::Error(_))
    );
    if is_reply
        && message.correlation_id != 0
        && let Some(waiter) = inner.pending.lock().await.remove(&message.correlation_id)
    {
        let _ = waiter.send(message);
        return;
    }
    let _ = inner.events.send(message);
}
