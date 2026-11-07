//! `tracing` JSON logs, rotated daily and kept for 14 days, with redaction (SEC-06).
use std::io::{self, Write};
use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::redact::redact;

pub const KEEP_DAYS: usize = 14;

/// Wraps a writer so every buffer (one formatted event) is redacted before it's written.
pub struct Redacting<W>(pub W);

impl<W: Write> Write for Redacting<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.0.write_all(redact(&text).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// `MakeWriter` adapter that hands out [`Redacting`] writers.
#[derive(Clone)]
pub struct RedactingMakeWriter<M>(pub M);

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for RedactingMakeWriter<M> {
    type Writer = Redacting<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        Redacting(self.0.make_writer())
    }
}

/// Start logging for one process. Files are `<logs>/<role>.<date>.log`. Keep the returned
/// guard alive for the life of the process so buffered lines are flushed.
pub fn init(role: &str, logs: &Path) -> anyhow::Result<WorkerGuard> {
    std::fs::create_dir_all(logs)?;
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(role)
        .filename_suffix("log")
        .max_log_files(KEEP_DAYS)
        .build(logs)?;
    let (file, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_env("SELAH_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let file_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(false)
        .with_writer(RedactingMakeWriter(file));
    let console_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(false)
        .with_writer(RedactingMakeWriter(io::stderr));
    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(console_layer)
        .try_init()?;
    tracing::info!(role, version = crate::VERSION, "process started");
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacting_writer_removes_seeded_secrets_before_writing() {
        // sec: SEC-06
        crate::redact::register_secret("TEST-SECRET-WRITER-99");
        let mut out = Redacting(Vec::new());
        write!(out, "{{\"token_value\":\"TEST-SECRET-WRITER-99\"}}").unwrap();
        let written = String::from_utf8(out.0).unwrap();
        assert!(!written.contains("TEST-SECRET-WRITER-99"), "{written}");
    }
}
