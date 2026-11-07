//! Golden-frame harness (TESTING-AND-QUALITY §1 stage 7, ADR-023).
//!
//! One reference set lives in `tests/golden/reference/`. A platform may have its own image in
//! `tests/golden/<platform>/` (added only through the bless-golden workflow, with a reason).
//! Images change only on purpose: with `SELAH_BLESS_GOLDEN=1` (set by `tools/bless-golden.sh
//! --reason`) the test writes the reference instead of comparing.
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use selah_render::Frame;

/// How different a frame may be from its reference.
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    /// A channel difference above this counts the pixel as different.
    pub channel: u8,
    /// At most this fraction of pixels may differ.
    pub max_fraction: f64,
}

impl Tolerance {
    pub const EXACT: Tolerance = Tolerance {
        channel: 0,
        max_fraction: 0.0,
    };
    /// Anti-aliased text: Skia's CPU paths use different SIMD code on x86 and ARM.
    pub const TEXT: Tolerance = Tolerance {
        channel: 4,
        max_fraction: 0.005,
    };
}

#[derive(Debug)]
pub struct Difference {
    pub differing: usize,
    pub total: usize,
    pub max_channel: u8,
}

impl Difference {
    pub fn fraction(&self) -> f64 {
        self.differing as f64 / self.total.max(1) as f64
    }
}

pub fn compare(actual: &Frame, expected: &Frame, tolerance: Tolerance) -> Result<Difference> {
    if (actual.width, actual.height) != (expected.width, expected.height) {
        bail!(
            "size {}×{} differs from the reference {}×{}",
            actual.width,
            actual.height,
            expected.width,
            expected.height
        );
    }
    let mut differing = 0;
    let mut max_channel = 0;
    for (a, e) in actual
        .pixels
        .chunks_exact(4)
        .zip(expected.pixels.chunks_exact(4))
    {
        let worst = a
            .iter()
            .zip(e)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        max_channel = max_channel.max(worst);
        if worst > tolerance.channel {
            differing += 1;
        }
    }
    Ok(Difference {
        differing,
        total: actual.pixels.len() / 4,
        max_channel,
    })
}

fn golden_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden")
}

/// `windows`, `macos` or `linux`.
pub fn platform() -> &'static str {
    std::env::consts::OS
}

/// Compare `actual` with the golden image `name`, or write it when blessing.
pub fn check(name: &str, actual: &Frame, tolerance: Tolerance) -> Result<()> {
    let root = golden_root();
    let reference = root.join("reference").join(format!("{name}.png"));
    let platform_specific = root.join(platform()).join(format!("{name}.png"));
    if std::env::var("SELAH_BLESS_GOLDEN").as_deref() == Ok("1") {
        std::fs::create_dir_all(reference.parent().expect("parent"))?;
        std::fs::write(&reference, selah_render::png::encode(actual)?)?;
        eprintln!("blessed {}", reference.display());
        return Ok(());
    }
    let path = if platform_specific.is_file() {
        platform_specific
    } else {
        reference
    };
    let bytes = std::fs::read(&path)
        .with_context(|| format!("golden image {} is missing", path.display()))?;
    let expected = selah_render::png::decode(&bytes)?;
    let difference = compare(actual, &expected, tolerance)?;
    if difference.fraction() > tolerance.max_fraction {
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/verify-report/golden")
            .join(format!("{name}.actual.png"));
        std::fs::create_dir_all(out.parent().expect("parent"))?;
        std::fs::write(&out, selah_render::png::encode(actual)?)?;
        bail!(
            "{name}: {:.3}% of pixels differ (limit {:.3}%, largest channel difference {}); \
             actual frame saved to {}",
            difference.fraction() * 100.0,
            tolerance.max_fraction * 100.0,
            difference.max_channel,
            out.display()
        );
    }
    Ok(())
}
