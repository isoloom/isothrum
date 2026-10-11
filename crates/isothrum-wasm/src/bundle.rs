//! The zips the user downloads: the whole lab, or one target's files.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

use crate::files::{config_files, starter_files};
use crate::{File, SPEC_PATH, plan};

/// The zip the user downloads: `isoloom.yml` at the root, and each generated file at its path.
pub fn bundle_bytes(topology_json: &str) -> Result<Vec<u8>, String> {
    bundle_filtered(topology_json, None)
}

/// One target's zip: `isoloom.yml` and only that target's folder (`docker`, `hybrid`,
/// `cloud-docker/aws`...), so `isoloom run` and the target's own tool both work from it.
pub fn bundle_target_bytes(topology_json: &str, target_dir: &str) -> Result<Vec<u8>, String> {
    bundle_filtered(topology_json, Some(target_dir))
}

fn bundle_filtered(topology_json: &str, target_dir: Option<&str>) -> Result<Vec<u8>, String> {
    let plan = plan(topology_json);
    let yaml = plan
        .isoloom_yml
        .ok_or_else(|| plan.error.unwrap_or_default())?;
    if !plan.problems.is_empty() {
        return Err(format!("fix the spec first: {}", plan.problems.join("; ")));
    }
    let files = target_files(plan.files, target_dir)?;
    // The lab's own files, next to isoloom.yml where its paths point; a one-target download only
    // gets those its files use (no starter scripts for Docker).
    let mut own = config_files(topology_json);
    own.extend(starter_files(&yaml)?);
    own.retain(|o| target_dir.is_none() || files.iter().any(|f| f.contents.contains(&o.path)));
    let spec = File {
        path: SPEC_PATH.into(),
        contents: yaml,
    };
    zip_of(std::iter::once(&spec).chain(&own).chain(&files))
}

/// The generated files of the target in `target_dir` (`docker`, `cloud-docker/aws`…), or all.
fn target_files(files: Vec<File>, target_dir: Option<&str>) -> Result<Vec<File>, String> {
    let prefix = target_dir.map(|d| format!(".isoloom/{}/", d.trim_matches('/')));
    let files: Vec<File> = files
        .into_iter()
        .filter(|f| prefix.as_ref().is_none_or(|p| f.path.starts_with(p)))
        .collect();
    if files.is_empty() {
        return Err(format!(
            "this lab has no files for `{}`",
            target_dir.unwrap_or("any target")
        ));
    }
    Ok(files)
}

/// A zip of `files`, deflated, in order.
fn zip_of<'a>(files: impl Iterator<Item = &'a File>) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for f in files {
        zip.start_file(f.path.as_str(), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(f.contents.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let cursor = zip.finish().map_err(|e| e.to_string())?;
    Ok(cursor.into_inner())
}
