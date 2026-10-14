//! The lab's own files: appliances' configurations and VMs' starter provisioning scripts.

use crate::{File, plan};

/// The lab's own files, next to isoloom.yml where its paths point: the appliances'
/// configurations and the starter scripts of the VMs' provisioning steps. What the desktop app
/// writes with the spec to run it.
pub fn own_files(topology_json: &str) -> Vec<File> {
    let Some(yaml) = plan(topology_json).isoloom_yml else {
        return vec![];
    };
    let mut out = config_files(topology_json);
    out.extend(starter_files(&yaml).unwrap_or_default());
    out
}

/// The appliances' own configurations (IOS commands), where isoloom.yml points.
pub(crate) fn config_files(topology_json: &str) -> Vec<File> {
    serde_json::from_str::<isothrum_core::Topology>(topology_json)
        .map(|topo| isothrum_core::config_files(&topo))
        .unwrap_or_default()
        .into_iter()
        .map(|(path, contents)| File { path, contents })
        .collect()
}

/// Starter scripts for the VMs' provisioning steps, where isoloom.yml points.
pub(crate) fn starter_files(yaml: &str) -> Result<Vec<File>, String> {
    let spec = isoloom_spec::parse(yaml).map_err(|e| e.to_string())?;
    Ok(isothrum_core::starters::starters(&spec)
        .into_iter()
        .map(|s| File {
            path: s.path,
            contents: s.contents,
        })
        .collect())
}
