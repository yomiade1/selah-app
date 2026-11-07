#![forbid(unsafe_code)]
//! `selah-kb-lint [--release]`: lint every article (KNOWLEDGE-BASE §7.1). Errors and warnings
//! print as GitHub annotations; exit 1 if there's any error.
use selah_kb::{Context, Options, Severity};

fn main() -> anyhow::Result<()> {
    let mut options = Options::default();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--release" => options.release = true,
            other => anyhow::bail!("unknown option {other} (usage: selah-kb-lint [--release])"),
        }
    }
    let root = selah_kb::find_root(&std::env::current_dir()?)
        .ok_or_else(|| anyhow::anyhow!("run inside the repository"))?;
    let context = Context::load(&root)?;
    let corpus = selah_kb::load(&root, &context, &options);
    let mut errors = 0;
    for finding in &corpus.findings {
        let level = match finding.severity {
            Severity::Error => {
                errors += 1;
                "error"
            }
            Severity::Warning => "warning",
        };
        println!(
            "::{level} file={},line={},title={}::{}",
            finding.path, finding.line, finding.rule, finding.message
        );
    }
    let warnings = corpus.findings.len() - errors;
    println!(
        "selah-kb-lint: {} articles, {errors} error(s), {warnings} warning(s)",
        corpus.articles.len()
    );
    if errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}
