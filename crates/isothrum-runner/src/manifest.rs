//! Reading a run manifest from its file: the contract's own parser (`isoloom-manifest`, which
//! checks the version, the types and what spans fields), plus where it lives and its hash, which
//! a run's record keeps.

use std::path::{Path, PathBuf};

use isoloom_manifest::Manifest;
use sha2::{Digest, Sha256};

use crate::Result;
use crate::error::Error;

/// A manifest read from its file.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoadedManifest {
    /// The manifest.
    pub manifest: Manifest,
    /// Its file, absolute.
    pub path: PathBuf,
    /// Its folder, absolute: `${run.dir}`, and the folder its relative paths start from.
    pub dir: PathBuf,
    /// The SHA-256 of its bytes, in lowercase hex.
    pub sha256: String,
}

/// Reads and checks the manifest at `path`.
///
/// # Errors
///
/// Fails with `io` when the file can't be read, and with `manifest_invalid` or `manifest_version`
/// when the contract refuses it (the error names the field and the fix).
pub fn load_manifest(path: &Path) -> Result<LoadedManifest> {
    let path = std::path::absolute(path).map_err(Error::io(path, "read"))?;
    let bytes = std::fs::read(&path).map_err(Error::io(&path, "read"))?;
    let text = String::from_utf8_lossy(&bytes);
    let manifest = isoloom_manifest::parse(&text).map_err(|source| Error::Manifest { path: path.clone(), source })?;
    let dir = path.parent().map_or_else(|| path.clone(), Path::to_path_buf);
    Ok(LoadedManifest {
        manifest,
        sha256: sha256(&bytes),
        path,
        dir,
    })
}

/// `bytes`' SHA-256, in lowercase hex.
fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .flat_map(|b| [b >> 4, b & 0xF])
        .filter_map(|nibble| char::from_digit(u32::from(nibble), 16))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_the_files_sha256() {
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn a_missing_file_is_an_io_error_naming_it() {
        let err = load_manifest(Path::new("no/such/run.json")).unwrap_err();
        assert_eq!(err.code(), crate::ErrorCode::Io);
        assert!(err.to_string().contains("run.json couldn't be read"), "{err}");
    }
}
