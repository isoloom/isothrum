//! The editor's engine, compiled to WebAssembly: a topology (JSON) becomes an `isoloom.yml`, the
//! files every Isoloom target generates from it, and one zip to download. Nothing leaves the
//! browser: Isoloom itself produces the Docker, Vagrant and Terraform files.

use std::io::{Cursor, Write};

use indexmap::IndexMap;
use isoloom_core::{GeneratedFile, Spec};
use serde::Serialize;
use wasm_bindgen::prelude::*;
use zip::write::SimpleFileOptions;

/// Where the spec itself goes in the bundle.
const SPEC_PATH: &str = "isoloom.yml";

#[derive(Debug, Serialize, PartialEq)]
pub struct Plan {
    /// `false` when the topology can't be compiled; `error` says why.
    pub ok: bool,
    pub error: Option<String>,
    /// The spec the editor shows and the user keeps.
    pub isoloom_yml: Option<String>,
    /// What the spec's validation reports, in the words of `isoloom validate`.
    pub problems: Vec<String>,
    /// Every generated file, with the target's own paths (for example `.isoloom/docker/compose.yml`).
    pub files: Vec<File>,
    /// Targets the spec allows but no generator covers yet, with the reason.
    pub skipped: Vec<String>,
    /// Each machine's addresses, for the canvas: machine -> [(network, IPv4)].
    pub addresses: IndexMap<String, Vec<(String, String)>>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct File {
    pub path: String,
    pub contents: String,
}

impl Plan {
    fn failed(error: String) -> Plan {
        Plan {
            ok: false,
            error: Some(error),
            isoloom_yml: None,
            problems: vec![],
            files: vec![],
            skipped: vec![],
            addresses: IndexMap::new(),
        }
    }
}

/// Turns a topology (the JSON the editor produces) into the spec and everything generated from it.
pub fn plan(topology_json: &str) -> Plan {
    let topo: isothrum_core::Topology = match serde_json::from_str(topology_json) {
        Ok(t) => t,
        Err(e) => return Plan::failed(format!("the topology is malformed: {e}")),
    };
    let spec = match isothrum_core::compile(&topo) {
        Ok(s) => s,
        Err(e) => return Plan::failed(e.to_string()),
    };
    let yaml = match isothrum_core::to_yaml(&spec) {
        Ok(y) => y,
        Err(e) => return Plan::failed(e.to_string()),
    };

    let problems: Vec<String> = isoloom_core::validate(&spec).iter().map(|p| p.to_string()).collect();
    // Isoloom's generators assume a valid spec (an unattached machine has no address to give,
    // for example), so nothing is generated until validation passes.
    let (generated, skipped) =
        if problems.is_empty() { isoloom_core::generate_all(&spec) } else { (vec![], vec![]) };

    let addresses = addresses(&spec);
    Plan {
        addresses,
        ok: true,
        error: None,
        isoloom_yml: Some(yaml),
        problems,
        files: generated.into_iter().map(|GeneratedFile { path, contents }| File { path, contents }).collect(),
        skipped: skipped.iter().map(|e| format!("{e:?}")).collect(),
    }
}

/// Every machine's IPv4 addresses: the network's first three octets and the machine's last one
/// (Isoloom's networks are /24 to /29, so the last octet is all that varies).
fn addresses(spec: &Spec) -> IndexMap<String, Vec<(String, String)>> {
    spec.machines
        .iter()
        .map(|(name, m)| {
            let ips = m
                .networks
                .iter()
                .filter_map(|(net, octet)| {
                    let cidr = &spec.networks.get(net)?.cidr;
                    let base = cidr.split('/').next()?.rsplit_once('.')?.0;
                    Some((net.clone(), format!("{base}.{octet}")))
                })
                .collect();
            (name.clone(), ips)
        })
        .collect()
}

/// The zip the user downloads: `isoloom.yml` at the root, and each generated file at its path.
pub fn bundle_bytes(topology_json: &str) -> Result<Vec<u8>, String> {
    let plan = plan(topology_json);
    let yaml = plan.isoloom_yml.ok_or_else(|| plan.error.unwrap_or_default())?;
    if !plan.problems.is_empty() {
        return Err(format!("fix the spec first: {}", plan.problems.join("; ")));
    }

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut add = |path: &str, contents: &str| -> Result<(), String> {
        zip.start_file(path, options).map_err(|e| e.to_string())?;
        zip.write_all(contents.as_bytes()).map_err(|e| e.to_string())
    };
    add(SPEC_PATH, &yaml)?;
    for f in &plan.files {
        add(&f.path, &f.contents)?;
    }
    let cursor = zip.finish().map_err(|e| e.to_string())?;
    Ok(cursor.into_inner())
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
mod tests {
    use super::*;

    const TOPOLOGY: &str = r#"{
        "name": "pair",
        "nodes": {
            "client": { "kind": { "vm": { "os": "debian-12", "provision": ["provision/client.sh"] } }, "access": true },
            "server": { "kind": { "container": { "image": "nginx:1.27-alpine" } }, "services": [{ "port": 80, "http": true }] }
        },
        "links": [ { "a": { "node": "client", "port": "eth0" }, "b": { "node": "server", "port": "eth0" } } ]
    }"#;

    #[test]
    fn a_topology_becomes_a_spec_and_generated_files() {
        let plan = plan(TOPOLOGY);
        assert!(plan.ok, "{:?}", plan.error);
        let yml = plan.isoloom_yml.unwrap();
        assert!(yml.contains("name: pair"));
        assert!(plan.files.iter().any(|f| f.path.ends_with("docker/compose.yml")), "{:?}", plan.files.iter().map(|f| &f.path).collect::<Vec<_>>());
    }

    #[test]
    fn machines_get_addresses_on_their_lans() {
        let t = r#"{"name":"lan","networks":{"office":{"cidr":"10.10.1.0/24"}},"nodes":{
            "a":{"kind":{"container":{"image":"alpine:3.20"}},"networks":{"office":null}},
            "b":{"kind":{"container":{"image":"alpine:3.20"}},"networks":{"office":20}}}}"#;
        let plan = plan(t);
        assert!(plan.ok, "{:?}", plan.error);
        assert_eq!(plan.addresses["a"], vec![("office".to_string(), "10.10.1.10".to_string())]);
        assert_eq!(plan.addresses["b"], vec![("office".to_string(), "10.10.1.20".to_string())]);
        assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    }

    #[test]
    fn a_bad_topology_says_why() {
        let plan = plan(r#"{"name":"x","nodes":{"a":{"kind":{"vm":{"os":"debian-12"}}}},"links":[{"a":{"node":"a","port":"e"},"b":{"node":"ghost","port":"e"}}]}"#);
        assert!(!plan.ok);
        assert!(plan.error.unwrap().contains("ghost"));
    }

    #[test]
    fn an_invalid_spec_is_reported_not_generated() {
        // A machine without a cable has no network: Isoloom's generators would panic on it.
        let t = r#"{"name":"lab","nodes":{
            "a":{"kind":{"container":{"image":"alpine:3.20"}}},
            "b":{"kind":{"container":{"image":"alpine:3.20"}}},
            "lonely":{"kind":{"vm":{"os":"debian-12"}}}},
            "links":[{"a":{"node":"a","port":"eth0"},"b":{"node":"b","port":"eth0"}}]}"#;
        let plan = plan(t);
        assert!(plan.ok);
        assert!(plan.problems.iter().any(|p| p.contains("lonely")), "{:?}", plan.problems);
        assert!(plan.files.is_empty());
        assert!(bundle_bytes(t).is_err());
    }

    #[test]
    fn the_bundle_holds_the_spec_and_the_generated_files() {
        let bytes = bundle_bytes(TOPOLOGY).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(archive.by_name(SPEC_PATH).is_ok());
        assert!(archive.len() > 2);
    }
}

