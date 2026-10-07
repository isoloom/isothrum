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
    // isoloom.yml keeps the VLANs under their switch; the rest of Isoloom reads them as networks.
    let spec = match isoloom_core::vlans::flatten(spec) {
        Ok(s) => s,
        Err(e) => return Plan::failed(e),
    };

    let problems: Vec<String> = isoloom_core::validate(&spec)
        .iter()
        .map(|p| p.to_string())
        .collect();
    // Isoloom's generators assume a valid spec (an unattached machine has no address to give,
    // for example), so nothing is generated until validation passes.
    let (generated, skipped) = if problems.is_empty() {
        isoloom_core::generate_all(&spec)
    } else {
        (vec![], vec![])
    };

    let addresses = addresses(&spec, &topo);
    Plan {
        addresses,
        ok: true,
        error: None,
        isoloom_yml: Some(yaml),
        problems,
        files: generated
            .into_iter()
            .map(|GeneratedFile { path, contents }| File { path, contents })
            .collect(),
        skipped: skipped.iter().map(|e| format!("{e:?}")).collect(),
    }
}

/// Every machine's IPv4 addresses: the network's first three octets and the machine's last one
/// (Isoloom's networks are /24 to /29, so the last octet is all that varies). Networks go by the
/// editor's names: a VLAN that became `sw1-vlan10` is the editor's `vlan10` again.
fn addresses(
    spec: &Spec,
    topo: &isothrum_core::Topology,
) -> IndexMap<String, Vec<(String, String)>> {
    let editor_name: IndexMap<String, &String> = topo
        .networks
        .iter()
        .filter_map(|(name, lan)| {
            Some((
                isoloom_core::vlans::vlan_network(lan.switch.as_ref()?, lan.vlan?),
                name,
            ))
        })
        .collect();
    spec.machines
        .iter()
        .map(|(name, m)| {
            let ips = m
                .networks
                .iter()
                .filter_map(|(net, octet)| {
                    let cidr = &spec.networks.get(net)?.cidr;
                    let base = cidr.split('/').next()?.rsplit_once('.')?.0;
                    let shown = editor_name
                        .get(net)
                        .map(|n| (*n).clone())
                        .unwrap_or_else(|| net.clone());
                    Some((shown, format!("{base}.{octet}")))
                })
                .collect();
            (name.clone(), ips)
        })
        .collect()
}

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

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut add = |path: &str, contents: &str| -> Result<(), String> {
        zip.start_file(path, options).map_err(|e| e.to_string())?;
        zip.write_all(contents.as_bytes())
            .map_err(|e| e.to_string())
    };
    let prefix = target_dir.map(|d| format!(".isoloom/{}/", d.trim_matches('/')));
    let files: Vec<&File> = plan
        .files
        .iter()
        .filter(|f| prefix.as_ref().is_none_or(|p| f.path.starts_with(p)))
        .collect();
    if files.is_empty() {
        return Err(format!(
            "this lab has no files for `{}`",
            target_dir.unwrap_or("any target")
        ));
    }
    add(SPEC_PATH, &yaml)?;
    // Starter scripts for the VMs' provisioning steps, next to isoloom.yml where its paths point;
    // a one-target download only gets those its files use (none for Docker).
    let spec = isoloom_core::parse(&yaml).map_err(|e| e.to_string())?;
    // Appliances' own configurations (IOS commands), where isoloom.yml points.
    if let Ok(topo) = serde_json::from_str::<isothrum_core::Topology>(topology_json) {
        for (path, contents) in isothrum_core::config_files(&topo) {
            if target_dir.is_none() || files.iter().any(|f| f.contents.contains(&path)) {
                add(&path, &contents)?;
            }
        }
    }
    for s in isothrum_core::starters::starters(&spec) {
        if target_dir.is_none() || files.iter().any(|f| f.contents.contains(&s.path)) {
            add(&s.path, &s.contents)?;
        }
    }
    for f in files {
        add(&f.path, &f.contents)?;
    }
    let cursor = zip.finish().map_err(|e| e.to_string())?;
    Ok(cursor.into_inner())
}

/// The download for one target (see [`bundle_target_bytes`]).
#[wasm_bindgen]
pub fn bundle_target(topology_json: &str, target_dir: &str) -> Result<Vec<u8>, JsError> {
    bundle_target_bytes(topology_json, target_dir).map_err(|e| JsError::new(&e))
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
        assert!(
            plan.files
                .iter()
                .any(|f| f.path.ends_with("docker/compose.yml")),
            "{:?}",
            plan.files.iter().map(|f| &f.path).collect::<Vec<_>>()
        );
    }

    #[test]
    fn machines_get_addresses_on_their_lans() {
        let t = r#"{"name":"lan","networks":{"office":{"cidr":"10.10.1.0/24"}},"nodes":{
            "a":{"kind":{"container":{"image":"alpine:3.20"}},"networks":{"office":null}},
            "b":{"kind":{"container":{"image":"alpine:3.20"}},"networks":{"office":20}}}}"#;
        let plan = plan(t);
        assert!(plan.ok, "{:?}", plan.error);
        assert_eq!(
            plan.addresses["a"],
            vec![("office".to_string(), "10.10.1.10".to_string())]
        );
        assert_eq!(
            plan.addresses["b"],
            vec![("office".to_string(), "10.10.1.20".to_string())]
        );
        assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    }

    #[test]
    fn a_bad_topology_says_why() {
        let plan = plan(
            r#"{"name":"x","nodes":{"a":{"kind":{"vm":{"os":"debian-12"}}}},"links":[{"a":{"node":"a","port":"e"},"b":{"node":"ghost","port":"e"}}]}"#,
        );
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
        assert!(
            plan.problems.iter().any(|p| p.contains("lonely")),
            "{:?}",
            plan.problems
        );
        assert!(plan.files.is_empty());
        assert!(bundle_bytes(t).is_err());
    }

    #[test]
    fn a_target_bundle_holds_only_that_target() {
        // A target this lab does produce: the folder of its first generated file.
        let first = plan(TOPOLOGY).files[0].path.clone();
        let target = first
            .trim_start_matches(".isoloom/")
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let bytes = bundle_target_bytes(TOPOLOGY, &target).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&SPEC_PATH.to_string()));
        let prefix = format!(".isoloom/{target}/");
        assert!(
            names
                .iter()
                .all(|n| n == SPEC_PATH || n.starts_with(&prefix)),
            "{names:?}"
        );
        assert!(names.len() > 1);
        assert!(bundle_target_bytes(TOPOLOGY, "no-such-target").is_err());
    }

    #[test]
    fn the_bundle_holds_the_spec_and_the_generated_files() {
        let bytes = bundle_bytes(TOPOLOGY).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(archive.by_name(SPEC_PATH).is_ok());
        assert!(archive.len() > 2);
    }

    #[test]
    fn a_vm_lab_comes_with_its_provisioning_scripts() {
        let names = |bytes: Vec<u8>| {
            let mut a = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
            (0..a.len())
                .map(|i| a.by_index(i).unwrap().name().to_string())
                .collect::<Vec<_>>()
        };
        let vms = r#"{"name":"vms","networks":{"lan":{"cidr":"10.10.1.0/24"}},"nodes":{
            "db":{"kind":{"vm":{"os":"debian-12","provision":["provision/db.sh"]}},"networks":{"lan":null},"services":[{"port":5432}]},
            "app":{"kind":{"vm":{"os":"debian-12","provision":["provision/app.sh"]}},"networks":{"lan":null},"access":true}}}"#;
        let all = names(bundle_bytes(vms).unwrap());
        assert!(all.contains(&"provision/db.sh".to_string()), "{all:?}");
        assert!(
            names(bundle_target_bytes(vms, "vagrant").unwrap())
                .contains(&"provision/db.sh".to_string())
        );
        // The mixed lab's Docker download runs no provisioning: no script in it.
        assert!(
            !names(bundle_target_bytes(TOPOLOGY, "docker").unwrap())
                .contains(&"provision/client.sh".to_string())
        );
    }

    #[test]
    fn vlans_nest_under_their_switch_and_keep_the_editor_names() {
        let t = r#"{"name":"vlans","networks":{
            "vlan10":{"cidr":"10.10.10.0/24","switch":"sw1","vlan":10,"internet":false},
            "vlan20":{"cidr":"10.10.20.0/24","switch":"sw1","vlan":20,"internet":false}},
            "nodes":{
            "pc1":{"kind":{"container":{"image":"alpine:3.20","idle":true}},"networks":{"vlan10":null}},
            "srv":{"kind":{"container":{"image":"nginx:1.27-alpine"}},"networks":{"vlan20":null}},
            "r":{"kind":{"router":{}},"networks":{"vlan10":null,"vlan20":null}}}}"#;
        let plan = plan(t);
        assert!(plan.ok, "{:?}", plan.error);
        assert!(plan.problems.is_empty(), "{:?}", plan.problems);
        let yml = plan.isoloom_yml.unwrap();
        assert!(
            yml.contains("  sw1:\n    cidr: 10.10.0.0/19\n    internet: false\n    vlans:\n"),
            "{yml}"
        );
        assert!(
            yml.contains("sw1.vlan10: 10") && yml.contains("from: sw1.vlan10"),
            "{yml}"
        );
        assert_eq!(
            plan.addresses["pc1"],
            vec![("vlan10".to_string(), "10.10.10.10".to_string())]
        );
        assert!(
            plan.files.iter().any(
                |f| f.path.ends_with("docker/compose.yml") && f.contents.contains("sw1-vlan10")
            )
        );
        // The download parses it back the same way.
        assert!(bundle_bytes(t).is_ok());
    }

    #[test]
    fn a_cable_with_conditions_carries_them() {
        let t = r#"{"name":"wan","nodes":{
            "a":{"kind":{"container":{"image":"alpine:3.20","idle":true}}},
            "b":{"kind":{"container":{"image":"alpine:3.20","idle":true}}}},
            "links":[{"a":{"node":"a","port":"eth0"},"b":{"node":"b","port":"eth0"},"tc":{"delay":"40ms","loss":0.5}}]}"#;
        let plan = plan(t);
        assert!(
            plan.ok && plan.problems.is_empty(),
            "{:?} {:?}",
            plan.error,
            plan.problems
        );
        let yml = plan.isoloom_yml.unwrap();
        assert!(
            yml.contains("tc:\n      delay: 40ms\n      loss: 0.5"),
            "{yml}"
        );
        assert!(
            plan.files
                .iter()
                .any(|f| f.path.ends_with("docker/compose.yml")
                    && f.contents.contains("netem delay 40ms loss 0.5%"))
        );
    }

    #[test]
    fn a_cisco_iol_router_is_its_lans_gateway_with_its_config() {
        let t = r#"{"name":"iol","networks":{"a":{"cidr":"10.30.1.0/24"},"b":{"cidr":"10.30.2.0/24"}},"nodes":{
            "r1":{"kind":{"container":{"image":"vrnetlab/cisco_iol:17.12.01","appliance":"cisco-iol","config":"router ospf 1\n network 10.30.0.0 0.0.255.255 area 0"}},"networks":{"a":null}},
            "r2":{"kind":{"container":{"image":"vrnetlab/cisco_iol:17.12.01","appliance":"cisco-iol"}},"networks":{"b":null}},
            "pc":{"kind":{"container":{"image":"alpine:3.20","idle":true}},"networks":{"a":null}}},
            "links":[{"a":{"node":"r1","port":"eth1"},"b":{"node":"r2","port":"eth1"}}]}"#;
        let plan = plan(t);
        assert!(
            plan.ok && plan.problems.is_empty(),
            "{:?} {:?}",
            plan.error,
            plan.problems
        );
        let yml = plan.isoloom_yml.unwrap();
        assert!(
            yml.contains("gateway: r1") && yml.contains("gateway: r2"),
            "{yml}"
        );
        assert!(
            yml.contains("appliance: cisco-iol") && yml.contains("config: configs/r1.cfg"),
            "{yml}"
        );
        assert_eq!(
            plan.addresses["r1"][0],
            ("a".to_string(), "10.30.1.1".to_string())
        );
        let cfg = plan
            .files
            .iter()
            .find(|f| f.path.ends_with("appliances/r1/base.txt"))
            .unwrap();
        assert!(
            cfg.contents.contains(
                "interface Ethernet0/1\n description a\n ip address 10.30.1.1 255.255.255.0"
            ),
            "{}",
            cfg.contents
        );
        let mut zip =
            zip::ZipArchive::new(Cursor::new(bundle_target_bytes(t, "docker").unwrap())).unwrap();
        let mut own = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("configs/r1.cfg").unwrap(), &mut own)
            .unwrap();
        assert!(own.starts_with("router ospf 1"));
    }
}
