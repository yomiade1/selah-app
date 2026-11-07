#![forbid(unsafe_code)]
//! `selah-kb build [--out PATH] [--release]` writes kb.db (gate stage 9);
//! `selah-kb ast <id>` prints an article's AST as JSON.
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let root = selah_kb::find_root(&std::env::current_dir()?)
        .ok_or_else(|| anyhow::anyhow!("run inside the repository"))?;
    match args.next().as_deref() {
        Some("build") => {
            let mut out = root.join("target/kb/kb.db");
            let mut options = selah_kb::Options::default();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--out" => {
                        out =
                            PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("--out PATH"))?)
                    }
                    "--release" => options.release = true,
                    other => anyhow::bail!("unknown option {other}"),
                }
            }
            let corpus = selah_kb::build(&root, &out, &options)?;
            let errors = corpus.errors().count();
            for finding in corpus.errors() {
                eprintln!(
                    "{}:{}: {} {}",
                    finding.path, finding.line, finding.rule, finding.message
                );
            }
            anyhow::ensure!(
                errors == 0,
                "{errors} lint error(s); kb.db not written (run selah-kb-lint)"
            );
            println!(
                "kb.db: {} articles → {}",
                corpus.articles.len(),
                out.display()
            );
            Ok(())
        }
        Some("ast") => {
            let id = args
                .next()
                .ok_or_else(|| anyhow::anyhow!("selah-kb ast <id>"))?;
            let context = selah_kb::Context::load(&root)?;
            let corpus = selah_kb::load(&root, &context, &selah_kb::Options::default());
            let article = corpus
                .articles
                .iter()
                .find(|a| a.front.id == id)
                .ok_or_else(|| anyhow::anyhow!("no article {id}"))?;
            println!("{}", serde_json::to_string_pretty(&article.doc)?);
            Ok(())
        }
        _ => {
            eprintln!("usage: selah-kb build [--out PATH] [--release] | selah-kb ast <id>");
            std::process::exit(2);
        }
    }
}
