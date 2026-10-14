//! The plan the editor shows: the spec, its problems, the files every target generates, and
//! each machine's addresses.

use indexmap::IndexMap;
use isoloom_spec::Spec;
use isoloom_targets::GeneratedFile;
use isothrum_core::Topology;
use serde::Serialize;

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
    let (topo, yaml, spec) = match compiled(topology_json) {
        Ok(parts) => parts,
        Err(e) => return Plan::failed(e),
    };
    let problems: Vec<String> = isoloom_spec::validate(&spec)
        .iter()
        .map(|p| p.to_string())
        .collect();
    // Isoloom's generators assume a valid spec (an unattached machine has no address to give,
    // for example), so nothing is generated until validation passes.
    let (generated, skipped) = if problems.is_empty() {
        isoloom_targets::generate_all(&spec)
    } else {
        (vec![], vec![])
    };
    Plan {
        addresses: addresses(&spec, &topo),
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

/// The topology, its isoloom.yml, and the spec as the rest of Isoloom reads it (isoloom.yml keeps
/// the VLANs under their switch; the rest of Isoloom reads them as networks).
fn compiled(topology_json: &str) -> Result<(Topology, String, Spec), String> {
    let topo: Topology = serde_json::from_str(topology_json)
        .map_err(|e| format!("the topology is malformed: {e}"))?;
    let spec = isothrum_core::compile(&topo).map_err(|e| e.to_string())?;
    let yaml = isothrum_core::to_yaml(&spec).map_err(|e| e.to_string())?;
    let spec = isoloom_spec::vlans::flatten(spec)?;
    Ok((topo, yaml, spec))
}

/// Every machine's IPv4 addresses: the network's first three octets and the machine's last one
/// (Isoloom's networks are /24 to /29, so the last octet is all that varies). Networks go by the
/// editor's names: a VLAN that became `sw1-vlan10` is the editor's `vlan10` again.
fn addresses(spec: &Spec, topo: &Topology) -> IndexMap<String, Vec<(String, String)>> {
    let editor_name: IndexMap<String, &String> = topo
        .networks
        .iter()
        .filter_map(|(name, lan)| {
            Some((format!("{}-vlan{}", lan.switch.as_ref()?, lan.vlan?), name))
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
