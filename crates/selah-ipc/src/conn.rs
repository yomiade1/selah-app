//! Async framing over a verified stream, split into a reader and a cloneable sender.
use std::io;

use prost::Message;
use selah_proto::v1::Envelope;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf};
use tokio::sync::mpsc;

use crate::frame::{decode_body, encode_frame, frame_length};
use crate::transport::{BoxStream, Peer};

/// Read one frame. The length is checked before any body memory is allocated (SEC-02).
pub async fn read_frame_async<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Envelope> {
    let mut header = [0_u8; 4];
    reader.read_exact(&mut header).await?;
    let length = frame_length(header)?;
    // Grow with the data actually received, so a header alone can't reserve 16 MiB.
    let mut body = Vec::with_capacity(length.min(crate::frame::INITIAL_READ_CAPACITY));
    (&mut *reader)
        .take(length as u64)
        .read_to_end(&mut body)
        .await?;
    if body.len() != length {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "frame body is truncated",
        ));
    }
    decode_body(&body)
}

/// Write one frame with a single write call.
pub async fn write_frame_async<W: AsyncWrite + Unpin>(
    writer: &mut W,
    message: &Envelope,
) -> io::Result<()> {
    let bytes = encode_frame(message)?;
    writer.write_all(&bytes).await?;
    writer.flush().await
}

/// Reading side of a connection.
pub struct FrameReader {
    inner: ReadHalf<BoxStream>,
}

impl FrameReader {
    pub async fn next(&mut self) -> io::Result<Envelope> {
        read_frame_async(&mut self.inner).await
    }
}

/// Sending side of a connection. Cheap to clone; a background task does the writing, so
/// event broadcasts never block the caller on a slow peer for long.
#[derive(Clone)]
pub struct FrameSender {
    queue: mpsc::Sender<Envelope>,
}

impl FrameSender {
    pub async fn send(&self, message: Envelope) -> io::Result<()> {
        if message.encoded_len() > crate::frame::MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "frame body is outside the permitted range",
            ));
        }
        self.queue
            .send(message)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "connection closed"))
    }

    /// Send without waiting; fails if the queue is full (a stalled peer) or closed.
    pub fn try_send(&self, message: Envelope) -> io::Result<()> {
        self.queue
            .try_send(message)
            .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "peer is not keeping up"))
    }

    pub fn is_closed(&self) -> bool {
        self.queue.is_closed()
    }
}

/// A verified connection.
pub struct Connection {
    pub peer: Peer,
    pub reader: FrameReader,
    pub sender: FrameSender,
}

/// Outgoing frames a connection may queue before senders wait.
const SEND_QUEUE: usize = 256;

impl Connection {
    pub fn new(stream: BoxStream, peer: Peer) -> Self {
        let (read, mut write) = tokio::io::split(stream);
        let (queue, mut outgoing) = mpsc::channel::<Envelope>(SEND_QUEUE);
        tokio::spawn(async move {
            while let Some(message) = outgoing.recv().await {
                if let Err(error) = write_frame_async(&mut write, &message).await {
                    tracing::debug!(%error, "IPC write failed; closing the connection");
                    break;
                }
            }
            let _ = write.shutdown().await;
        });
        Self {
            peer,
            reader: FrameReader { inner: read },
            sender: FrameSender { queue },
        }
    }
}
