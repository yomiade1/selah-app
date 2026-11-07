//! **Help ▸ Diagnostics ▸ Run benchmark** (M1.13, ADR-023): measures this computer and saves a
//! report the person can attach to a PR or a problem report. This is how hardware results reach
//! cloud sessions.
//!
//! It runs `selah-bench` (installed next to the other programs): the IPC round trip and a video
//! decode-and-composite run on the 4K60 HEVC clip (the 1080p H.264 clip for a quick run). It's
//! refused while anything is on the screens, because it loads the GPU and CPU (SAFE-06).
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// Error catalogue codes (content/errors/errors.yaml).
pub mod codes {
    /// Something is on the screens.
    pub const LIVE: &str = "SEL-DIAG-0001";
    /// The benchmark couldn't run or its report couldn't be saved.
    pub const FAILED: &str = "SEL-DIAG-0002";
    /// The support bundle couldn't be saved.
    pub const BUNDLE: &str = "SEL-SUP-0001";
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DiagnosticsError {
    #[error("something is on the screens")]
    Live,
    #[error("the benchmark failed: {0}")]
    Failed(String),
}

impl DiagnosticsError {
    pub fn code(&self) -> &'static str {
        match self {
            DiagnosticsError::Live => codes::LIVE,
            DiagnosticsError::Failed(_) => codes::FAILED,
        }
    }

    pub fn message_key(&self) -> &'static str {
        match self {
            DiagnosticsError::Live => "error-diagnostics-live",
            DiagnosticsError::Failed(_) => "error-diagnostics-failed",
        }
    }
}

/// What to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub bench: PathBuf,
    pub clip: PathBuf,
    pub ipc_iterations: u32,
}

/// The numbers the Diagnostics window shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Summary {
    pub ipc_p99_ms: f64,
    pub ipc_pass: bool,
    pub video_clip: String,
    pub video_decode: String,
    pub video_frames: u64,
    pub video_dropped: u64,
    pub video_composite_p99_ms: f64,
    pub video_pass: bool,
}

/// Find `selah-bench` and a clip next to the running programs (`bench/`), or in the repository
/// during development. `SELAH_BENCH_CLIP` overrides the clip.
pub fn plan(exe_dir: &Path, quick: bool) -> Result<Plan, DiagnosticsError> {
    let bench = exe_dir.join(format!("selah-bench{}", std::env::consts::EXE_SUFFIX));
    if !bench.is_file() {
        return Err(DiagnosticsError::Failed(
            "selah-bench isn't installed".into(),
        ));
    }
    let name = if quick {
        "h264-1080p60.mp4"
    } else {
        "hevc-2160p60.mp4"
    };
    let clip = std::env::var_os("SELAH_BENCH_CLIP")
        .map(PathBuf::from)
        .into_iter()
        .chain([
            exe_dir.join("bench").join(name),
            // target/<profile>/ → the repository's clips (generated, or committed on macOS).
            exe_dir
                .join("../../tests/fixtures/media/generated")
                .join(name),
            exe_dir
                .join("../../tests/fixtures/media/committed")
                .join(name),
        ])
        .find(|path| path.is_file())
        .ok_or_else(|| DiagnosticsError::Failed(format!("the clip {name} isn't installed")))?;
    Ok(Plan {
        bench,
        clip,
        ipc_iterations: if quick { 500 } else { 10_000 },
    })
}

fn run_bench(
    plan: &Plan,
    args: &[&str],
    json: &Path,
) -> Result<serde_json::Value, DiagnosticsError> {
    let output = Command::new(&plan.bench)
        .args(args)
        .arg("--json")
        .arg(json)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| DiagnosticsError::Failed(error.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let last = stderr.lines().last().unwrap_or_default().to_owned();
        return Err(DiagnosticsError::Failed(format!(
            "selah-bench {} exited with {}: {last}",
            args.first().unwrap_or(&""),
            output.status
        )));
    }
    let text = std::fs::read_to_string(json)
        .map_err(|error| DiagnosticsError::Failed(error.to_string()))?;
    serde_json::from_str(&text).map_err(|error| DiagnosticsError::Failed(error.to_string()))
}

fn number(value: &serde_json::Value, key: &str) -> f64 {
    value
        .get(key)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or_default()
}

fn flag(value: &serde_json::Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_default()
}

fn text(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Summarise the two `selah-bench` reports.
pub fn summarise(ipc: &serde_json::Value, video: &serde_json::Value) -> Summary {
    Summary {
        ipc_p99_ms: number(ipc, "p99_ms"),
        ipc_pass: flag(ipc, "pass"),
        video_clip: Path::new(&text(video, "clip"))
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        video_decode: text(video, "decode"),
        video_frames: number(video, "frames") as u64,
        video_dropped: number(video, "dropped") as u64,
        video_composite_p99_ms: number(video, "composite_p99_ms"),
        video_pass: flag(video, "pass"),
    }
}

/// Run both benchmarks and save the combined report in `out_dir`. `context` (version, safe mode,
/// renderer…) goes in the report as it is.
pub fn run(
    plan: &Plan,
    out_dir: &Path,
    context: serde_json::Value,
) -> Result<(PathBuf, Summary), DiagnosticsError> {
    let failed = |error: std::io::Error| DiagnosticsError::Failed(error.to_string());
    let work = tempfile_dir(out_dir).map_err(failed)?;
    let ipc = run_bench(
        plan,
        &["ipc", "--iterations", &plan.ipc_iterations.to_string()],
        &work.join("ipc.json"),
    );
    let video = ipc.as_ref().ok().map(|_| {
        run_bench(
            plan,
            &["video", "--clip", &plan.clip.to_string_lossy()],
            &work.join("video.json"),
        )
    });
    let _ = std::fs::remove_dir_all(&work);
    let ipc = ipc?;
    let video = video.unwrap_or_else(|| Err(DiagnosticsError::Failed("not run".into())))?;
    let summary = summarise(&ipc, &video);
    let stamp = crate::support::file_stamp(std::time::SystemTime::now());
    let report = serde_json::json!({
        "report": "selah-diagnostics",
        "created": stamp,
        "selah": selah_runtime::VERSION,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "context": context,
        "summary": summary,
        "ipc": ipc,
        "video": video,
    });
    let path = out_dir.join(format!("selah-diagnostics-{stamp}.json"));
    let text = serde_json::to_string_pretty(&report)
        .map_err(|e| DiagnosticsError::Failed(e.to_string()))?;
    selah_runtime::paths::write_atomic(&path, selah_runtime::redact::redact(&text).as_bytes())
        .map_err(failed)?;
    tracing::info!(file = %path.display(), ?summary, "diagnostics report saved");
    Ok((path, summary))
}

fn tempfile_dir(out_dir: &Path) -> std::io::Result<PathBuf> {
    let dir = out_dir.join(format!(".bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_reads_both_reports() {
        let ipc = serde_json::json!({"p99_ms": 0.12, "pass": true});
        let video = serde_json::json!({
            "clip": "C:/x/bench/hevc-2160p60.mp4", "decode": "d3d11va", "frames": 180,
            "dropped": 9, "composite_p99_ms": 16.0, "pass": false
        });
        let summary = summarise(&ipc, &video);
        assert_eq!(summary.video_clip, "hevc-2160p60.mp4");
        assert_eq!(summary.video_dropped, 9);
        assert!(summary.ipc_pass && !summary.video_pass);
    }

    #[test]
    fn a_missing_benchmark_is_a_plain_error() {
        let dir = tempfile::tempdir().unwrap();
        let error = plan(dir.path(), true).unwrap_err();
        assert_eq!(error.code(), codes::FAILED);
    }
}
