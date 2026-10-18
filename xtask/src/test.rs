//! `test`: the workspace's tests under cargo-nextest (each test in its own process, nothing
//! retried, slow tests reported: `.config/nextest.toml`), then the doctests, which nextest
//! doesn't run, with `cargo test --doc`. Both run, so a failure in one doesn't hide the other.

use std::process::ExitCode;

use crate::{Res, process, repo};

/// The arguments of `test`.
#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// More arguments for `cargo nextest run` (a filter, `--no-capture`); with them, the doctests
    /// don't run.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    nextest: Vec<String>,
}

/// Runs the tests, then the doctests; fails when either does.
pub(crate) fn run(a: &Args) -> Res<ExitCode> {
    let root = repo::root();
    process::require_cargo_tool(&root, "nextest")?;
    let tests = process::passed(process::cargo(&root).args(["nextest", "run", "--workspace"]).args(&a.nextest))?;
    if !a.nextest.is_empty() {
        return Ok(process::exit_code(tests));
    }
    let doctests = process::passed(process::cargo(&root).args(["test", "--workspace", "--doc"]))?;
    Ok(process::exit_code(tests && doctests))
}
