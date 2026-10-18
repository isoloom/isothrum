//! `wasm`: the editor's engine built for the browser, which downloads it. `wasm-pack` builds
//! `crates/isothrum-wasm` into `web/pkg/` with Cargo's release profile set for size (opt-level
//! `z`, LTO, one codegen unit: Isoloom's targets bring every backend, and the editor's zip needs
//! them all), only for this build. With a website checkout, its `public/wasm/` gets the two files
//! the site serves (the JavaScript glue and the WebAssembly).

use std::path::PathBuf;
use std::process::Command;

use crate::{Res, process, repo};

/// The files the website serves from `public/wasm/`.
const SERVED: [&str; 2] = ["isothrum_wasm.js", "isothrum_wasm_bg.wasm"];

/// The arguments of `wasm`.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// A website checkout whose `public/wasm/` gets the engine.
    #[arg(long)]
    website: Option<PathBuf>,
}

/// Builds the engine into `web/pkg/`, then copies what the site serves into the website.
pub(crate) fn build(a: &Args) -> Res<()> {
    let root = repo::root();
    let mut cmd = Command::new("wasm-pack");
    cmd.args(["build", "crates/isothrum-wasm", "--target", "web", "--out-dir", "../../web/pkg", "--release"])
        .current_dir(&root)
        .env("CARGO_PROFILE_RELEASE_OPT_LEVEL", "z")
        .env("CARGO_PROFILE_RELEASE_LTO", "true")
        .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1");
    process::run(&mut cmd)?;
    let Some(site) = &a.website else { return Ok(()) };
    let served = site.join("public").join("wasm");
    for file in SERVED {
        std::fs::copy(root.join("web/pkg").join(file), served.join(file)).map_err(|e| format!("copying {file} into {}: {e}", served.display()))?;
    }
    Ok(())
}
