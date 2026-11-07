//! M1-A6: golden text frames for English, Yorùbá, Korean and Arabic (M1.6).
//! Run with `cargo test -p selah-testkit --features golden`.
#![cfg(feature = "golden")]

use selah_proto::v1::{Resolution, Scene, TestPattern};
use selah_render::Compositor;
use selah_render::gpu::{AdapterChoice, GpuCompositor};
use selah_testkit::golden::{self, Tolerance};

/// Public-domain texts (Psalm 23:1 and a public-domain hymn line).
pub const SAMPLES: [(&str, &str); 4] = [
    (
        "en",
        "Amazing grace, how sweet the sound\nThat saved a wretch like me",
    ),
    // Precomposed (ọ́, ṣ) and combining sequences (e + dot below + grave) on purpose.
    (
        "yo",
        "Olúwa ni olùṣọ́ àgùntàn mi;\ne\u{0323}\u{0300}mi kì yóò ṣe aláìní. Ọlọ́run",
    ),
    ("ko", "주님은 나의 목자시니\n내게 부족함이 없으리로다"),
    ("ar", "الرب راعيّ\nفلا يعوزني شيء"),
];

fn scene(text: &str) -> Scene {
    Scene {
        revision: 1,
        test_pattern: TestPattern::Unspecified as i32,
        text: text.into(),
        cleared: false,
        resolution: Some(Resolution {
            width: 1920,
            height: 1080,
        }),
    }
}

#[test]
fn text_frames_match_the_golden_references() {
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");
    let mut failures = Vec::new();
    for (language, text) in SAMPLES {
        let frame = gpu.render(&scene(text), 1920, 1080).unwrap();
        if let Err(error) = golden::check(&format!("text-{language}"), &frame, Tolerance::TEXT) {
            failures.push(error.to_string());
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_sample_shapes_without_missing_glyphs() {
    let renderer = selah_render::text::TextRenderer::bundled().unwrap();
    for (language, text) in SAMPLES {
        assert_eq!(
            renderer.unresolved_glyphs(text),
            0,
            "{language} needs a glyph the slide fonts don't have"
        );
    }
}

#[test]
fn gpu_and_cpu_text_frames_agree() {
    let mut gpu = GpuCompositor::new(AdapterChoice::Software).expect("software GPU adapter");
    let mut cpu = selah_render::cpu::CpuCompositor::default();
    for (language, text) in SAMPLES {
        let a = gpu.render(&scene(text), 960, 540).unwrap();
        let b = cpu.render(&scene(text), 960, 540).unwrap();
        let difference = golden::compare(
            &a,
            &b,
            Tolerance {
                channel: 1,
                max_fraction: 0.0,
            },
        )
        .unwrap();
        assert_eq!(
            difference.differing, 0,
            "{language}: GPU and CPU blending differ"
        );
    }
}

#[test]
fn rendered_text_is_cached_by_content_style_and_resolution() {
    let mut renderer = selah_render::text::TextRenderer::bundled().unwrap();
    let style = selah_render::text::SlideTextStyle::for_output(540);
    let first = renderer.render("Hello", &style, 960, 540).unwrap();
    let again = renderer.render("Hello", &style, 960, 540).unwrap();
    assert!(
        std::sync::Arc::ptr_eq(&first, &again),
        "cache hit returns the same texture"
    );
    renderer.render("Hello", &style, 1920, 1080).unwrap();
    renderer.render("Hello!", &style, 960, 540).unwrap();
    assert_eq!(renderer.cached(), 3);
}
