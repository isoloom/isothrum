//! The editor's engine, compiled to WebAssembly: a topology (JSON) becomes an `isoloom.yml`, the
//! files every Isoloom target generates from it, and one zip to download. Nothing leaves the
//! browser: Isoloom itself produces the Docker, Vagrant and Terraform files.

mod bundle;
mod files;
mod plan;

use wasm_bindgen::prelude::*;

pub use bundle::{bundle_bytes, bundle_target_bytes};
pub use files::own_files;
pub use plan::{File, Plan, plan};

const SPEC_PATH: &str = "isoloom.yml";

/// The download for one target (see [`bundle_target_bytes`]).
#[wasm_bindgen]
pub fn bundle_target(topology_json: &str, target_dir: &str) -> Result<Vec<u8>, JsError> {
    bundle_target_bytes(topology_json, target_dir).map_err(|e| JsError::new(&e))
}

/// JSON for the desktop app: [`own_files`].
#[wasm_bindgen]
pub fn own_files_json(topology_json: &str) -> String {
    serde_json::to_string(&own_files(topology_json)).expect("files serialize")
}

/// JSON for the editor: a [`Plan`].
#[wasm_bindgen]
pub fn plan_json(topology_json: &str) -> String {
    serde_json::to_string(&plan(topology_json)).expect("a plan serializes")
}

/// The download: a zip with `isoloom.yml` and every generated file.
#[wasm_bindgen]
pub fn bundle(topology_json: &str) -> Result<Vec<u8>, JsError> {
    bundle_bytes(topology_json).map_err(|e| JsError::new(&e))
}

#[cfg(test)]
mod tests;
