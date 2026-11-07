#![forbid(unsafe_code)]
//! `selah-net`: the single gate for outgoing network connections (SEC-13, ARCHITECTURE §11).
//!
//! - Every outgoing request names a [`Purpose`]. The [`Policy`] allows nothing by default, so
//!   with default settings Selah makes no outgoing connections. Core turns purposes on only when
//!   the person turns on the feature that needs them (for example **Let Selah check for updates
//!   when I ask**).
//! - Only `https://` addresses, with no user name or password in them, are accepted.
//! - Every attempt, allowed or refused, is recorded in the connection log (host and purpose
//!   only: paths and queries can hold tokens) and written to the redacted log.
//! - Bodies are read with a size limit and requests have timeouts.
//!
//! No other crate may depend on an HTTP client or open sockets; a test checks the workspace.
use std::collections::{HashSet, VecDeque};
use std::sync::{Mutex, RwLock};
use std::time::{Duration, SystemTime};

/// Why Selah wants to connect. Each feature that needs the network adds one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Purpose {
    /// **Check for updates**: read the update feed (M1.11).
    UpdateCheck,
    /// **Install update**: download the package the feed names (M1.11).
    UpdateDownload,
}

impl Purpose {
    pub const ALL: [Purpose; 2] = [Purpose::UpdateCheck, Purpose::UpdateDownload];

    pub fn name(self) -> &'static str {
        match self {
            Purpose::UpdateCheck => "update-check",
            Purpose::UpdateDownload => "update-download",
        }
    }
}

/// Which purposes may connect. The default allows nothing (SEC-13).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    allowed: HashSet<Purpose>,
}

impl Policy {
    pub fn allow(mut self, purpose: Purpose) -> Self {
        self.allowed.insert(purpose);
        self
    }

    pub fn allows(&self, purpose: Purpose) -> bool {
        self.allowed.contains(&purpose)
    }

    pub fn is_offline(&self) -> bool {
        self.allowed.is_empty()
    }
}

/// What happened to one attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The request was made and this many bytes came back.
    Done { bytes: u64 },
    /// The gate refused it before any connection was made.
    Refused(String),
    /// The request was made and failed.
    Failed(String),
}

/// One entry in the connection log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub at: SystemTime,
    pub purpose: Purpose,
    /// The host only, never the path or query.
    pub host: String,
    pub outcome: Outcome,
}

impl Connection {
    /// Did this attempt reach the network?
    pub fn connected(&self) -> bool {
        !matches!(self.outcome, Outcome::Refused(_))
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NetError {
    #[error("{0} is not allowed by the network policy")]
    NotAllowed(&'static str),
    #[error("the address is not allowed: {0}")]
    BadAddress(&'static str),
    #[error("the request failed: {0}")]
    Failed(String),
    #[error("the response is larger than {0} bytes")]
    TooLarge(u64),
}

/// Makes the actual request. The real one is [`HttpsTransport`]; tests use fakes.
pub trait Transport: Send + Sync {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, NetError>;
}

/// The parts of an address the gate checks.
#[derive(Debug, PartialEq, Eq)]
pub struct Address<'a> {
    pub host: &'a str,
}

/// Accept only `https://host[:port]/...` with no credentials and a sensible host.
pub fn check_address(url: &str) -> Result<Address<'_>, NetError> {
    if url.len() > 4096 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(NetError::BadAddress("it isn't a valid address"));
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return Err(NetError::BadAddress("only https:// addresses are allowed"));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err(NetError::BadAddress(
            "addresses can't hold a user name or password",
        ));
    }
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if !host.ends_with(']') || authority.starts_with('[') => {
            if port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
                return Err(NetError::BadAddress("the port isn't valid"));
            }
            host
        }
        _ => authority,
    };
    let valid = !host.is_empty()
        && host.len() <= 253
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':'));
    if !valid {
        return Err(NetError::BadAddress("the host name isn't valid"));
    }
    Ok(Address { host })
}

/// Kept entries in the in-memory connection log.
pub const LOG_CAPACITY: usize = 500;

/// The gate.
pub struct Net {
    policy: RwLock<Policy>,
    log: Mutex<VecDeque<Connection>>,
    transport: Box<dyn Transport>,
}

impl Net {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            policy: RwLock::new(Policy::default()),
            log: Mutex::new(VecDeque::new()),
            transport,
        }
    }

    /// The real gate: HTTPS with rustls and the computer's certificate store.
    pub fn https() -> Self {
        Self::new(Box::new(HttpsTransport::new()))
    }

    pub fn set_policy(&self, policy: Policy) {
        *self.policy.write().unwrap_or_else(|p| p.into_inner()) = policy;
    }

    pub fn policy(&self) -> Policy {
        self.policy
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// Every attempt so far, oldest first (the last [`LOG_CAPACITY`]).
    pub fn connections(&self) -> Vec<Connection> {
        self.log
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    fn record(&self, purpose: Purpose, host: &str, outcome: Outcome) {
        match &outcome {
            Outcome::Done { bytes } => {
                tracing::info!(purpose = purpose.name(), host, bytes, "outgoing connection")
            }
            Outcome::Refused(reason) => {
                tracing::warn!(purpose = purpose.name(), host, reason = %reason, "outgoing connection refused")
            }
            Outcome::Failed(reason) => {
                tracing::warn!(purpose = purpose.name(), host, reason = %reason, "outgoing connection failed")
            }
        }
        let mut log = self.log.lock().unwrap_or_else(|p| p.into_inner());
        if log.len() == LOG_CAPACITY {
            log.pop_front();
        }
        log.push_back(Connection {
            at: SystemTime::now(),
            purpose,
            host: host.to_owned(),
            outcome,
        });
    }

    /// GET `url` for `purpose`, reading at most `limit` bytes.
    pub fn get(&self, purpose: Purpose, url: &str, limit: u64) -> Result<Vec<u8>, NetError> {
        let host = match check_address(url) {
            Ok(address) => address.host.to_owned(),
            Err(error) => {
                self.record(purpose, "", Outcome::Refused(error.to_string()));
                return Err(error);
            }
        };
        if !self.policy().allows(purpose) {
            let error = NetError::NotAllowed(purpose.name());
            self.record(purpose, &host, Outcome::Refused(error.to_string()));
            return Err(error);
        }
        match self.transport.get(url, limit) {
            Ok(body) if body.len() as u64 > limit => {
                let error = NetError::TooLarge(limit);
                self.record(purpose, &host, Outcome::Failed(error.to_string()));
                Err(error)
            }
            Ok(body) => {
                self.record(
                    purpose,
                    &host,
                    Outcome::Done {
                        bytes: body.len() as u64,
                    },
                );
                Ok(body)
            }
            Err(error) => {
                self.record(purpose, &host, Outcome::Failed(error.to_string()));
                Err(error)
            }
        }
    }
}

/// HTTPS with rustls (ring) and the operating system's certificate verifier. Plain HTTP,
/// including redirects to it, is refused.
pub struct HttpsTransport {
    agent: ureq::Agent,
}

impl HttpsTransport {
    pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
    pub const TOTAL_TIMEOUT: Duration = Duration::from_secs(600);

    pub fn new() -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::Rustls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(5)
            .timeout_connect(Some(Self::CONNECT_TIMEOUT))
            .timeout_global(Some(Self::TOTAL_TIMEOUT))
            .user_agent(format!("Selah/{}", env!("CARGO_PKG_VERSION")))
            .tls_config(tls)
            .build()
            .into();
        Self { agent }
    }
}

impl Default for HttpsTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for HttpsTransport {
    fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>, NetError> {
        let mut response = self
            .agent
            .get(url)
            .call()
            .map_err(|error| NetError::Failed(error.to_string()))?;
        response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|error| match error {
                ureq::Error::BodyExceedsLimit(_) => NetError::TooLarge(limit),
                other => NetError::Failed(other.to_string()),
            })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Counts calls and answers with a fixed body.
    struct Counting {
        calls: Arc<AtomicUsize>,
        body: Vec<u8>,
    }

    impl Transport for Counting {
        fn get(&self, _url: &str, _limit: u64) -> Result<Vec<u8>, NetError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.body.clone())
        }
    }

    fn gate(body: &[u8]) -> (Net, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let net = Net::new(Box::new(Counting {
            calls: calls.clone(),
            body: body.to_vec(),
        }));
        (net, calls)
    }

    const FEED: &str = "https://updates.example.invalid/releases.dev.json?token=TEST-SECRET-NET-1";

    #[test]
    fn the_default_policy_connects_to_nothing() {
        // sec: SEC-13
        let (net, calls) = gate(b"{}");
        assert!(net.policy().is_offline());
        for purpose in Purpose::ALL {
            assert_eq!(
                net.get(purpose, FEED, 1024),
                Err(NetError::NotAllowed(purpose.name()))
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let log = net.connections();
        assert_eq!(log.len(), Purpose::ALL.len());
        assert!(log.iter().all(|c| !c.connected()));
    }

    #[test]
    fn allowed_purposes_connect_and_are_logged_by_host_only() {
        // sec: SEC-13, SEC-06
        let (net, calls) = gate(b"{\"Assets\":[]}");
        net.set_policy(Policy::default().allow(Purpose::UpdateCheck));
        assert_eq!(
            net.get(Purpose::UpdateCheck, FEED, 1024).unwrap(),
            b"{\"Assets\":[]}"
        );
        assert!(net.get(Purpose::UpdateDownload, FEED, 1024).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let log = net.connections();
        assert_eq!(log[0].host, "updates.example.invalid");
        assert_eq!(log[0].outcome, Outcome::Done { bytes: 13 });
        assert!(
            log.iter()
                .all(|c| !format!("{c:?}").contains("TEST-SECRET-NET-1"))
        );
    }

    #[test]
    fn only_plain_https_addresses_get_through() {
        // sec: SEC-12, SEC-13
        let (net, calls) = gate(b"x");
        net.set_policy(Policy::default().allow(Purpose::UpdateCheck));
        for bad in [
            "http://updates.example.invalid/feed.json",
            "ftp://updates.example.invalid/feed.json",
            "file:///C:/feed.json",
            "https://user:pass@updates.example.invalid/feed.json",
            "https:///feed.json",
            "https://bad host/feed.json",
            "https://updates.example.invalid:http/feed.json",
            "https://updates.example.invalid/feed.json\r\nX-Evil: 1",
        ] {
            assert!(
                matches!(
                    net.get(Purpose::UpdateCheck, bad, 10),
                    Err(NetError::BadAddress(_))
                ),
                "{bad}"
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            check_address("https://updates.example.invalid:8443/x")
                .unwrap()
                .host,
            "updates.example.invalid"
        );
    }

    #[test]
    fn responses_over_the_limit_are_refused() {
        // sec: SEC-03
        let (net, _) = gate(&[0u8; 100]);
        net.set_policy(Policy::default().allow(Purpose::UpdateDownload));
        assert_eq!(
            net.get(Purpose::UpdateDownload, FEED, 99),
            Err(NetError::TooLarge(99))
        );
    }

    #[test]
    fn the_log_keeps_the_most_recent_attempts() {
        let (net, _) = gate(b"x");
        for _ in 0..LOG_CAPACITY + 10 {
            let _ = net.get(Purpose::UpdateCheck, FEED, 10);
        }
        assert_eq!(net.connections().len(), LOG_CAPACITY);
    }

    #[test]
    fn the_real_transport_builds_without_touching_the_network() {
        let _ = HttpsTransport::new();
    }

    /// SEC-13: no other crate may reach the network on its own. Only `selah-net` may depend on
    /// an HTTP client, and only it may open TCP or UDP sockets.
    #[test]
    fn only_selah_net_reaches_the_network() {
        // sec: SEC-13
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
        const CLIENTS: [&str; 8] = [
            "ureq",
            "reqwest",
            "hyper",
            "isahc",
            "curl",
            "attohttpc",
            "surf",
            "ehttp",
        ];
        let selah: Vec<String> = std::fs::read_dir(root.join("crates"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        for block in lock.split("[[package]]") {
            let Some(name) = block
                .lines()
                .find_map(|line| line.strip_prefix("name = \""))
                .map(|rest| rest.trim_end_matches('"'))
            else {
                continue;
            };
            if !selah.iter().any(|crate_name| crate_name == name) || name == "selah-net" {
                continue;
            }
            for client in CLIENTS {
                let direct = block.lines().any(|line| {
                    let line = line.trim().trim_matches(|c| c == '"' || c == ',');
                    line == client || line.starts_with(&format!("{client} "))
                });
                assert!(
                    !direct,
                    "{name} depends on {client}; go through selah-net (SEC-13)"
                );
            }
        }
        for crate_name in selah.iter().filter(|name| *name != "selah-net") {
            let src = root.join("crates").join(crate_name);
            for path in walk(&src) {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                for socket in ["TcpStream", "UdpSocket", "TcpListener", "ToSocketAddrs"] {
                    assert!(
                        !text.contains(socket),
                        "{} uses {socket}; go through selah-net (SEC-13)",
                        path.display()
                    );
                }
            }
        }
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.file_name().is_some_and(|n| n != "target") {
                out.extend(walk(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
        out
    }
}
