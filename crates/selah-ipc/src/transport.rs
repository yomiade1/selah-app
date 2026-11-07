//! Private local transports (ADR-004, SEC-02).
//!
//! Both ends check that the other end runs as the same user. On Windows the pipe also has an
//! owner-only DACL, rejects remote clients, and the server must be the first instance (so a
//! squatter that created the name first is detected instead of joined).
use std::io;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::endpoint::Endpoint;

/// A connected byte stream to or from a verified same-user peer.
pub trait IpcStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> IpcStream for T {}

pub type BoxStream = Box<dyn IpcStream>;

/// What we know about the verified peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    pub pid: Option<u32>,
}

fn refused(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

/// Accept connections on an endpoint. Peers that aren't the current user are dropped.
pub struct Listener {
    endpoint: Endpoint,
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
    #[cfg(unix)]
    path: std::path::PathBuf,
    #[cfg(windows)]
    next: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
    #[cfg(windows)]
    security: crate::windows::OwnerOnly,
    #[cfg(windows)]
    own_sid: String,
}

impl Listener {
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    #[cfg(unix)]
    pub async fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        let path = endpoint.socket_path()?;
        if path.exists() {
            // A live server answers; a stale file from a crash doesn't.
            if tokio::net::UnixStream::connect(&path).await.is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another process is already serving this endpoint",
                ));
            }
            std::fs::remove_file(&path)?;
        }
        let inner = tokio::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            endpoint: endpoint.clone(),
            inner,
            path,
        })
    }

    #[cfg(unix)]
    pub async fn accept(&mut self) -> io::Result<(BoxStream, Peer)> {
        loop {
            let (stream, _) = self.inner.accept().await?;
            match verify_unix_peer(&stream) {
                Ok(peer) => return Ok((Box::new(stream), peer)),
                Err(error) => tracing::warn!(%error, "refused an IPC client from another user"),
            }
        }
    }

    #[cfg(windows)]
    pub async fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        let mut security = crate::windows::OwnerOnly::new()?;
        let first = create_pipe(endpoint, &mut security, true).map_err(|error| {
            if error.raw_os_error() == Some(5) {
                io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "the pipe name is already in use by another process",
                )
            } else {
                error
            }
        })?;
        Ok(Self {
            endpoint: endpoint.clone(),
            next: Some(first),
            security,
            own_sid: crate::windows::current_user_sid()?,
        })
    }

    #[cfg(windows)]
    pub async fn accept(&mut self) -> io::Result<(BoxStream, Peer)> {
        use std::os::windows::io::AsHandle;
        loop {
            let server = match self.next.take() {
                Some(server) => server,
                None => create_pipe(&self.endpoint, &mut self.security, false)?,
            };
            let connected = server.connect().await;
            // Always have the next instance ready so clients don't see "busy".
            self.next = Some(create_pipe(&self.endpoint, &mut self.security, false)?);
            connected?;
            let pid = crate::windows::pipe_client_pid(server.as_handle())?;
            match crate::windows::process_user_sid(pid) {
                Ok(sid) if sid == self.own_sid => {
                    return Ok((Box::new(server), Peer { pid: Some(pid) }));
                }
                Ok(_) => tracing::warn!(pid, "refused an IPC client from another user"),
                Err(error) => tracing::warn!(pid, %error, "could not verify an IPC client"),
            }
        }
    }

    /// The pipe's DACL in SDDL form (Windows only), for the SEC-02 tests.
    #[cfg(windows)]
    pub fn dacl_sddl(&self) -> io::Result<String> {
        use std::os::windows::io::AsHandle;
        let server = self
            .next
            .as_ref()
            .ok_or_else(|| io::Error::other("no pipe instance"))?;
        crate::windows::dacl_sddl(server.as_handle())
    }
}

#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(windows)]
fn create_pipe(
    endpoint: &Endpoint,
    security: &mut crate::windows::OwnerOnly,
    first: bool,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    crate::windows::create_pipe(&endpoint.pipe_name(), security, first)
}

#[cfg(unix)]
fn verify_unix_peer(stream: &tokio::net::UnixStream) -> io::Result<Peer> {
    let credentials = stream.peer_cred()?;
    if credentials.uid() != rustix::process::getuid().as_raw() {
        return Err(refused("the peer runs as another user"));
    }
    Ok(Peer {
        pid: credentials.pid().and_then(|pid| u32::try_from(pid).ok()),
    })
}

/// Connect once. Fails if nothing is listening or the server isn't the current user.
#[cfg(unix)]
pub async fn connect(endpoint: &Endpoint) -> io::Result<(BoxStream, Peer)> {
    let stream = tokio::net::UnixStream::connect(endpoint.socket_path()?).await?;
    let peer = verify_unix_peer(&stream)?;
    Ok((Box::new(stream), peer))
}

#[cfg(windows)]
pub async fn connect(endpoint: &Endpoint) -> io::Result<(BoxStream, Peer)> {
    use std::os::windows::io::AsHandle;
    const ERROR_PIPE_BUSY: i32 = 231;
    let name = endpoint.pipe_name();
    let mut attempts = 0;
    let client = loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(&name) {
            Ok(client) => break client,
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempts < 50 => {
                attempts += 1;
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
            Err(error) => return Err(error),
        }
    };
    let pid = crate::windows::pipe_server_pid(client.as_handle())?;
    if crate::windows::process_user_sid(pid)? != crate::windows::current_user_sid()? {
        return Err(refused("the pipe server runs as another user"));
    }
    Ok((Box::new(client), Peer { pid: Some(pid) }))
}
