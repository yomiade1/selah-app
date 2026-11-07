#![forbid(unsafe_code)]

use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from("../../proto");
    let files = ["common", "errors", "render", "settings", "help", "envelope"]
        .map(|name| root.join(format!("selah/v1/{name}.proto")));
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
    }
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    config.compile_protos(&files, &[root])?;
    Ok(())
}
