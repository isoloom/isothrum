//! `schema`: the event stream's JSON Schema, committed beside the runner so embedders in other
//! languages read the events without the crate. A test fails when the committed file is stale.

use std::path::{Path, PathBuf};

use crate::{Res, repo};

/// The schema file, relative to the repository.
const SCHEMA: &str = "crates/isothrum-runner/schema/events-v0.schema.json";

/// The schema's path in the checkout at `root`.
fn path(root: &Path) -> PathBuf {
    repo::path_in(root, SCHEMA)
}

/// The event stream's schema, as committed.
fn text() -> Res<String> {
    Ok(serde_json::to_string_pretty(&isothrum_runner::events_schema())? + "\n")
}

/// Writes the schema.
pub(crate) fn write() -> Res<()> {
    let path = path(&repo::root());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, text()?).map_err(|e| format!("{}: {e}", path.display()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_event_schema_is_current() {
        let committed = std::fs::read_to_string(path(&repo::root())).unwrap_or_default();
        assert!(committed == text().unwrap(), "{SCHEMA} is stale: run `cargo run -p xtask -- schema`");
    }
}
