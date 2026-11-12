//! Reads the build number from the root `Cargo.toml` (`[workspace.metadata.billo] build`)
//! and exposes it to the crate as `BILLO_BUILD`, the same way the server does.

use std::path::PathBuf;

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../../Cargo.toml");
    println!("cargo:rerun-if-changed={}", root.display());

    let text = std::fs::read_to_string(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()));
    let doc: toml::Table = text.parse().expect("root Cargo.toml is not valid TOML");
    let build = doc
        .get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get("billo"))
        .and_then(|b| b.get("build"))
        .and_then(toml::Value::as_integer)
        .expect("missing [workspace.metadata.billo] build in root Cargo.toml");
    let build = u32::try_from(build).expect("build number must fit in u32");
    println!("cargo:rustc-env=BILLO_BUILD={build}");
}
