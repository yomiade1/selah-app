#![forbid(unsafe_code)]
//! `selah-bench`: measurements for the spike report and **Help ▸ Diagnostics ▸ Run benchmark**.
//!
//! ```text
//! selah-bench ipc [--iterations N] [--check] [--json PATH]
//! selah-bench video --clip PATH [--decode auto|software] [--loops N] [--gpu auto|software]
//!                   [--check] [--json PATH]
//! ```
//! `ipc` starts a child process that serves the core endpoint, then times request/reply round
//! trips across the real transport (named pipes on Windows, Unix sockets elsewhere).
//! `video` decodes a clip and composites it under text at the clip's frame rate (M1.7).
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use selah_ipc::{Client, Connection, Endpoint, Listener, Service, reply_to};
use selah_proto::v1::{
    Actor, ActorOrigin, GetStatus, Query, Reply, Status, envelope, query, reply,
};
use serde::Serialize;

/// ARCHITECTURE §12: UI ↔ core round trip p99 ≤ 2 ms.
const IPC_P99_BUDGET_MS: f64 = 2.0;
/// ARCHITECTURE §12: output frame time p99 ≤ 8 ms.
const OUTPUT_P99_BUDGET_MS: f64 = 8.0;

mod video;

#[derive(Serialize)]
struct IpcReport {
    benchmark: &'static str,
    transport: &'static str,
    os: &'static str,
    iterations: usize,
    p50_ms: f64,
    p99_ms: f64,
    max_ms: f64,
    mean_ms: f64,
    budget_p99_ms: f64,
    pass: bool,
}

fn actor(origin: ActorOrigin) -> Actor {
    Actor {
        user_id: "local-owner".into(),
        device_id: "selah-bench".into(),
        origin: origin as i32,
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let rank = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank.min(sorted.len() - 1)]
}

async fn serve(namespace: String) -> Result<()> {
    let endpoint = Endpoint::new(&namespace, Service::Core);
    let mut listener = Listener::bind(&endpoint).await?;
    println!("ready");
    let (stream, peer) = listener.accept().await?;
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
        sender.send(reply).await?;
    }
    Ok(())
}

async fn ipc(iterations: usize, check: bool, json: Option<PathBuf>) -> Result<()> {
    let namespace = format!("bench{}", std::process::id());
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args(["ipc-serve", &namespace])
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("start the IPC server process")?;
    {
        use tokio::io::AsyncBufReadExt;
        let stdout = child.stdout.take().context("server stdout")?;
        let mut line = String::new();
        tokio::io::BufReader::new(stdout)
            .read_line(&mut line)
            .await?;
        if line.trim() != "ready" {
            bail!("the IPC server didn't start");
        }
    }
    let client = Client::start(
        Endpoint::new(&namespace, Service::Core),
        actor(ActorOrigin::Ui),
    );
    client.wait_connected(Duration::from_secs(5)).await?;
    let body = || {
        envelope::Body::Query(Query {
            request: Some(query::Request::GetStatus(GetStatus {})),
        })
    };
    for _ in 0..500 {
        client.request(body(), Duration::from_secs(5)).await?;
    }
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        client.request(body(), Duration::from_secs(5)).await?;
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    drop(client);
    let _ = child.kill().await;
    samples.sort_by(f64::total_cmp);
    let p99 = percentile(&samples, 99.0);
    let report = IpcReport {
        benchmark: "ipc-round-trip",
        transport: if cfg!(windows) {
            "named-pipe"
        } else {
            "unix-socket"
        },
        os: std::env::consts::OS,
        iterations,
        p50_ms: percentile(&samples, 50.0),
        p99_ms: p99,
        max_ms: *samples.last().unwrap_or(&0.0),
        mean_ms: samples.iter().sum::<f64>() / samples.len().max(1) as f64,
        budget_p99_ms: IPC_P99_BUDGET_MS,
        pass: p99 <= IPC_P99_BUDGET_MS,
    };
    let text = serde_json::to_string_pretty(&report)?;
    println!("{text}");
    if let Some(path) = json {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, text + "\n")?;
    }
    if check && !report.pass {
        bail!("IPC p99 {p99:.3} ms is over the {IPC_P99_BUDGET_MS} ms budget");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("ipc") => {
            let mut iterations = 10_000;
            let mut check = false;
            let mut json = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--iterations" => {
                        iterations = args.next().context("--iterations N")?.parse()?
                    }
                    "--check" => check = true,
                    "--json" => json = Some(PathBuf::from(args.next().context("--json PATH")?)),
                    other => bail!("unknown option {other}"),
                }
            }
            if iterations == 0 {
                bail!("--iterations must be at least 1");
            }
            ipc(iterations, check, json).await
        }
        Some("ipc-serve") => serve(args.next().context("namespace")?).await,
        Some("video") => {
            let mut options = video::Options {
                clip: PathBuf::new(),
                mode: selah_media::DecodeMode::Auto,
                loops: 1,
                gpu: selah_render::gpu::AdapterChoice::from_env(),
            };
            let (mut check, mut json) = (false, None);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--clip" => options.clip = PathBuf::from(args.next().context("--clip PATH")?),
                    "--decode" => {
                        options.mode = match args.next().as_deref() {
                            Some("auto") => selah_media::DecodeMode::Auto,
                            Some("software") => selah_media::DecodeMode::Software,
                            _ => bail!("--decode auto|software"),
                        }
                    }
                    "--gpu" => {
                        options.gpu = match args.next().as_deref() {
                            Some("auto") => selah_render::gpu::AdapterChoice::Auto,
                            Some("software") => selah_render::gpu::AdapterChoice::Software,
                            _ => bail!("--gpu auto|software"),
                        }
                    }
                    "--loops" => options.loops = args.next().context("--loops N")?.parse()?,
                    "--check" => check = true,
                    "--json" => json = Some(PathBuf::from(args.next().context("--json PATH")?)),
                    other => bail!("unknown option {other}"),
                }
            }
            if options.clip.as_os_str().is_empty() {
                bail!("--clip PATH is required");
            }
            // Decoding and the GPU wait block: keep them off the async runtime.
            let report = tokio::task::spawn_blocking(move || video::run(&options)).await??;
            let text = serde_json::to_string_pretty(&report)?;
            println!("{text}");
            if let Some(path) = json {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(
                    &path,
                    text + "
",
                )?;
            }
            if check && !report.pass {
                bail!(
                    "video benchmark over budget: {} dropped, {} late, composite p99 {:.2} ms",
                    report.dropped,
                    report.late_ticks,
                    report.composite_p99_ms
                );
            }
            Ok(())
        }
        _ => {
            eprintln!("usage: selah-bench ipc [--iterations N] [--check] [--json PATH]");
            eprintln!(
                "       selah-bench video --clip PATH [--decode auto|software] [--loops N] [--gpu auto|software] [--check] [--json PATH]"
            );
            std::process::exit(2);
        }
    }
}
