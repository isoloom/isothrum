//! `msrv` and `features`, through cargo-hack (`cargo install --locked cargo-hack`):
//!
//! - `msrv`: every crate checked with the toolchain its `rust-version` names (`cargo hack check
//!   --rust-version`), so the declared minimum is a fact, not a hope. rustup must have that
//!   toolchain (`rustup toolchain install <version> --profile minimal`).
//! - `features`: every combination of every crate's features checked (`cargo hack check
//!   --feature-powerset`), so features stay additive and no combination breaks the build.
//!
//! A checkout Isoloom's `dev-link` points at the local Isoloom resolves into a lockfile of its own
//! (`resolver.lockfile-path`), which the older Cargo of the minimum toolchain doesn't know yet: it
//! resolves the linked crates into the committed `Cargo.lock` instead. `msrv` puts the committed
//! lockfile back afterwards, so the check never leaves it changed.

use std::path::Path;
use std::process::ExitCode;

use crate::{Res, process, repo};

/// Checks every crate with the toolchain of its `rust-version`.
pub(crate) fn msrv() -> Res<ExitCode> {
    let root = repo::root();
    let lock = root.join("Cargo.lock");
    let committed = std::fs::read(&lock)?;
    let checked = hack(&root, &["check", "--rust-version", "--workspace"]);
    if std::fs::read(&lock)? != committed {
        std::fs::write(&lock, committed)?;
        println!("msrv: Cargo.lock put back as committed (the minimum toolchain's Cargo resolved the dev-link into it)");
    }
    checked
}

/// Checks every crate with every combination of its features.
pub(crate) fn features() -> Res<ExitCode> {
    hack(&repo::root(), &["check", "--feature-powerset", "--workspace"])
}

fn hack(root: &Path, args: &[&str]) -> Res<ExitCode> {
    process::require_cargo_tool(root, "hack")?;
    // The toolchain `--rust-version` asks for (`cargo +<version>`) must win over the one rustup
    // picked for this xtask.
    let mut cmd = process::rustup_cargo(root);
    cmd.arg("hack").args(args);
    Ok(process::exit_code(process::passed(&mut cmd)?))
}
