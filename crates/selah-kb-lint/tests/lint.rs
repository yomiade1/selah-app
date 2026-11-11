//! M1-A10: the exemplars pass; each bad fixture fails with its expected rule id; kb.db builds and
//! answers searches.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use selah_kb::{Context, HelpIndex, Options, Severity, lint};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn context() -> Context {
    Context::load(&root()).expect("content loads")
}

fn lint_fixture(path: &Path, context: &Context) -> Vec<selah_kb::Finding> {
    let source = std::fs::read_to_string(path).unwrap();
    let stem = path.file_stem().unwrap().to_string_lossy().to_lowercase();
    // Each fixture's id is fixtures.<stem>, so its implied path is fixtures/<stem>.md.
    let relative = format!("fixtures/{stem}.md");
    let article = match lint::parse_article(&path.display().to_string(), "en", &relative, &source) {
        Ok(article) => article,
        Err(findings) => return findings,
    };
    let known: BTreeSet<String> = ["getting-started.report-a-problem".to_owned()].into();
    lint::lint_article(&article, context, &known, &Options::default())
}

#[test]
fn the_real_articles_have_no_errors() {
    let corpus = selah_kb::load(&root(), &context(), &Options::default());
    let errors: Vec<String> = corpus
        .errors()
        .map(|f| format!("{}:{} {} {}", f.path, f.line, f.rule, f.message))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    let ids: BTreeSet<&str> = corpus
        .articles
        .iter()
        .map(|a| a.front.id.as_str())
        .collect();
    for exemplar in [
        "live.show-a-song",
        "screens.what-edge-blending-is",
        "troubleshooting.screens-are-black",
    ] {
        assert!(ids.contains(exemplar), "{exemplar} is linted");
    }
}

#[test]
fn the_good_fixture_passes() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/good.md");
    let errors: Vec<_> = lint_fixture(&fixture, &context())
        .into_iter()
        .filter(|f| f.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn each_bad_fixture_fails_with_its_rule() {
    let context = context();
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bad");
    let mut checked = 0;
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let expected = name.split("--").next().unwrap().to_owned();
        let findings = lint_fixture(&path, &context);
        let rules: Vec<&str> = findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .map(|f| f.rule)
            .collect();
        assert!(
            rules.contains(&expected.as_str()),
            "{name}: expected {expected}, got {findings:#?}"
        );
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} bad fixtures");
}

#[test]
fn planned_references_warn_until_their_milestone_is_done() {
    let mut context = context();
    let corpus = selah_kb::load(&root(), &context, &Options::default());
    assert!(
        corpus
            .findings
            .iter()
            .any(|f| f.rule == "KB-PLANNED" && f.severity == Severity::Warning)
    );
    // Whichever milestone a planned reference belongs to, marking it done turns the warning into
    // an error. (This used to mark only M4, which stopped working once M4 built its planned items.)
    for milestone in 1..=10 {
        context.done.insert(format!("M{milestone}"));
    }
    let corpus = selah_kb::load(&root(), &context, &Options::default());
    assert!(
        corpus
            .findings
            .iter()
            .any(|f| f.rule == "KB-PLANNED" && f.severity == Severity::Error)
    );
}

#[test]
fn release_mode_requires_reviews() {
    let corpus = selah_kb::load(
        &root(),
        &context(),
        &Options {
            release: true,
            ..Options::default()
        },
    );
    assert!(corpus.errors().any(|f| f.rule == "KB-REVIEW"));
}

#[test]
fn kb_db_builds_and_answers_searches() {
    let folder = tempfile::tempdir().unwrap();
    let db = folder.path().join("kb.db");
    let corpus = selah_kb::build(&root(), &db, &Options::default()).unwrap();
    assert_eq!(corpus.errors().count(), 0);
    let index = HelpIndex::open(&db).unwrap();
    let top = |query: &str| {
        index
            .search(query, "en", &[], 5)
            .unwrap()
            .first()
            .map(|e| e.id.clone())
    };
    assert_eq!(
        top("why are my screens black").as_deref(),
        Some("troubleshooting.screens-are-black")
    );
    assert_eq!(
        top("how do I put lyrics up").as_deref(),
        Some("live.show-a-song")
    );
    assert_eq!(
        top("blnding").as_deref(),
        Some("screens.what-edge-blending-is"),
        "typos still find articles"
    );
    let article = index.article("live.show-a-song", "en").unwrap().unwrap();
    assert!(article.ast_json.contains("\"type\":\"label\""));
    let context = index
        .for_context(&["shell.safe-mode-banner".into()], "en")
        .unwrap();
    assert_eq!(context[0].id, "troubleshooting.selah-started-in-safe-mode");
    // Anchors visible on screen lift their articles.
    let boosted = index
        .search("screens", "en", &["shell.safe-mode-banner".into()], 5)
        .unwrap();
    assert!(
        boosted
            .iter()
            .any(|e| e.id == "troubleshooting.selah-started-in-safe-mode")
    );
}

#[test]
fn the_committed_ast_fixture_is_current() {
    // app/Selah.UI.Tests renders this file; regenerate it with
    // `cargo run -p selah-kb --bin selah-kb -- ast live.show-a-song > tests/fixtures/kb/show-a-song.ast.json`.
    let corpus = selah_kb::load(&root(), &context(), &Options::default());
    let article = corpus
        .articles
        .iter()
        .find(|a| a.front.id == "live.show-a-song")
        .unwrap();
    let current: serde_json::Value = serde_json::to_value(&article.doc).unwrap();
    let committed: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join("tests/fixtures/kb/show-a-song.ast.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        current, committed,
        "tests/fixtures/kb/show-a-song.ast.json is out of date"
    );
}

#[test]
fn coverage_detects_unused_and_undeclared_ui_anchors() {
    let mut ctx = context();
    let corpus = selah_kb::load(&root(), &ctx, &Options::default());
    assert_eq!(corpus.errors().count(), 0);

    // Unused anchor in anchors.yaml triggers KB-COVER.
    ctx.anchors.insert("unused.test-anchor".into());
    ctx.anchor_lines.insert("unused.test-anchor".into(), 99);
    // Undeclared anchor used in UI triggers KB-COVER.
    ctx.ui_anchors.insert("undeclared.test-anchor".into());
    ctx.ui_anchor_uses.push(selah_kb::content::UiAnchorUse {
        anchor: "undeclared.test-anchor".into(),
        path: "app/Selah.UI/Views/Fake.axaml".into(),
        line: 12,
    });
    let findings = lint::lint_coverage(&ctx, &corpus.articles, &Options::default());
    assert!(
        findings.iter().any(|f| {
            f.rule == "KB-COVER"
                && f.severity == Severity::Error
                && f.message.contains("unused.test-anchor")
        }),
        "{findings:#?}"
    );
    assert!(
        findings.iter().any(|f| {
            f.rule == "KB-COVER"
                && f.severity == Severity::Error
                && f.message.contains("undeclared.test-anchor")
        }),
        "{findings:#?}"
    );
}

#[test]
fn coverage_detects_uncovered_anchors_settings_errors_and_tours() {
    let mut ctx = context();
    let mut corpus = selah_kb::load(&root(), &ctx, &Options::default());
    // Downgrade the article that covers `shell.help-panel` to `draft` -> Check 2 fails.
    for article in &mut corpus.articles {
        if article.front.id == "getting-started.get-help-in-selah" {
            article.front.status = "draft".into();
        }
    }
    ctx.setting_defs.push(selah_kb::content::SettingDef {
        key: "uncovered.setting".into(),
        hint: "missing-hint-key".into(),
        line: 5,
    });
    ctx.error_defs.push(selah_kb::content::ErrorDef {
        code: "SEL-TEST-9999".into(),
        message: "missing-error-key".into(),
        fix_article: "troubleshooting.missing-fix".into(),
        line: 8,
    });
    ctx.tour_steps.push(selah_kb::content::TourStepUse {
        anchor: "tour.missing-anchor".into(),
        path: "content/help/tours/test.yaml".into(),
        line: 4,
    });

    let findings = lint::lint_coverage(&ctx, &corpus.articles, &Options::default());
    for expected in [
        "shell.help-panel",
        "uncovered.setting",
        "missing-hint-key",
        "SEL-TEST-9999",
        "missing-error-key",
        "tour.missing-anchor",
    ] {
        assert!(
            findings.iter().any(|f| f.rule == "KB-COVER"
                && f.severity == Severity::Error
                && f.message.contains(expected)),
            "expected KB-COVER containing {expected} in {findings:#?}"
        );
    }
}

#[test]
fn context_load_scans_axaml_cs_and_tour_files() {
    let dir = tempfile::tempdir().unwrap();
    let root_path = dir.path();
    std::fs::create_dir_all(root_path.join("content/help/tours")).unwrap();
    std::fs::create_dir_all(root_path.join("content/errors")).unwrap();
    std::fs::create_dir_all(root_path.join("content/i18n/en")).unwrap();
    std::fs::create_dir_all(root_path.join("content/kb/en")).unwrap();
    std::fs::create_dir_all(root_path.join("app/Selah.UI/Views")).unwrap();

    std::fs::write(
        root_path.join("content/terms.yaml"),
        "banned_words:\n  - just\nproduct_terms:\n  slide:\n    - page\n",
    )
    .unwrap();
    std::fs::write(
        root_path.join("content/help/anchors.yaml"),
        "anchors:\n  - id: sample.region\n    description: Sample\n",
    )
    .unwrap();
    std::fs::write(
        root_path.join("content/help/settings.yaml"),
        "settings: []\n",
    )
    .unwrap();
    std::fs::write(root_path.join("content/errors/errors.yaml"), "errors: []\n").unwrap();
    std::fs::write(
        root_path.join("content/kb/en/collections.yaml"),
        "collections: []\n",
    )
    .unwrap();
    std::fs::write(root_path.join("content/kb/planned.yaml"), "planned: []\n").unwrap();
    std::fs::write(
        root_path.join("content/help/tours/demo.yaml"),
        "id: tour.demo\ntitle: Demo\nsteps:\n  - anchor: sample.region\n    text: Hello\n",
    )
    .unwrap();
    std::fs::write(
        root_path.join("app/Selah.UI/Views/Demo.axaml"),
        "<StackPanel s:Help.Anchor=\"sample.region\" />\n",
    )
    .unwrap();

    let ctx = Context::load(root_path).unwrap();
    assert!(ctx.has_ui);
    assert!(ctx.ui_anchors.contains("sample.region"));
    assert_eq!(ctx.tour_steps.len(), 1);
    assert_eq!(ctx.tour_steps[0].anchor, "sample.region");
}
