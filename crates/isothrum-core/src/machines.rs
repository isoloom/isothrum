//! The topology's nodes as Isoloom machines, and Cisco routers as their LANs' gateways.

use indexmap::IndexMap;
use isoloom_core::model::{DockerImpl, VmImpl};
use isoloom_core::{Appliance, Machine};

use crate::draft::Draft;
use crate::vlans::vlan_ref;
use crate::{Kind, Node, Topology};

/// Every node: a machine, or an automatic router (which only records its networks).
pub fn add_machines(topo: &Topology, draft: &mut Draft) {
    for (name, node) in &topo.nodes {
        match &node.kind {
            Kind::Router {} => {
                let nets = node.networks.keys().cloned().collect();
                draft.routers.insert(name.clone(), nets);
            }
            _ => {
                draft.machines.insert(name.clone(), machine(name, node));
            }
        }
    }
}

/// A container or VM node as a machine, without its networks yet.
fn machine(name: &str, node: &Node) -> Machine {
    let mut m = Machine {
        access: node.access,
        services: node.services.clone(),
        ..Machine::default()
    };
    match &node.kind {
        Kind::Container {
            image,
            idle,
            appliance,
            config,
        } => m.docker = Some(docker(name, image, *idle, *appliance, config.as_deref())),
        Kind::Vm { os, provision } => {
            m.vm = Some(VmImpl {
                os: os.clone(),
                provision: provision.clone(),
                image: None,
            });
        }
        Kind::Router {} => unreachable!("automatic routers aren't machines"),
    }
    m
}

/// A container node's Docker side.
fn docker(
    name: &str,
    image: &str,
    idle: bool,
    appliance: Option<Appliance>,
    config: Option<&str>,
) -> DockerImpl {
    // A Dynamips router has no image: Isoloom builds the emulator, the user's IOS image goes in
    // the download's `images/` folder.
    let dynamips = appliance == Some(Appliance::CiscoDynamips);
    DockerImpl {
        image: (!dynamips).then(|| image.to_string()),
        build: None,
        init: vec![],
        idle: idle && appliance.is_none(),
        appliance,
        dockerfile: None,
        args: IndexMap::new(),
        config: config
            .filter(|c| !c.trim().is_empty())
            .map(|_| config_path(name)),
        firmware: dynamips.then(|| firmware_path(name)),
    }
}

/// A Cisco router on a LAN is that LAN's gateway (at its gateway address): the machines on it
/// route through IOS, as behind a real router. Not on a VLAN: Isoloom's VLANs have no gateway,
/// so there the router stays a member, at the address it already has.
pub fn cisco_gateways(topo: &Topology, draft: &mut Draft) {
    let plain_lan = |n: &String| topo.networks.get(n).is_some_and(|l| vlan_ref(l).is_none());
    for (lan, net) in draft.networks.iter_mut().filter(|(n, _)| plain_lan(n)) {
        let router = topo.nodes.iter().find(|(name, n)| {
            is_cisco_router(&n.kind)
                && draft
                    .machines
                    .get(*name)
                    .is_some_and(|m| m.networks.contains_key(lan))
        });
        if let Some((name, _)) = router
            && let Some(c) = isoloom_core::validate::Cidr::parse(&net.cidr)
        {
            net.gateway = Some(name.clone());
            draft.machines[name]
                .networks
                .insert(lan.clone(), c.gateway_octet());
        }
    }
}

/// A Cisco router appliance: IOL, IOSv, CSR 1000v, Catalyst 8000v or Dynamips.
fn is_cisco_router(kind: &Kind) -> bool {
    matches!(
        kind,
        Kind::Container {
            appliance: Some(
                Appliance::CiscoIol
                    | Appliance::CiscoVios
                    | Appliance::CiscoCsr1000v
                    | Appliance::CiscoC8000v
                    | Appliance::CiscoDynamips
            ),
            ..
        }
    )
}

/// Where a Dynamips router's IOS image goes in the project (the user's to add).
pub fn firmware_path(machine: &str) -> String {
    format!("images/{machine}.bin")
}

/// Where an appliance's own configuration goes in the project.
pub fn config_path(machine: &str) -> String {
    format!("configs/{machine}.cfg")
}

/// The appliances' own configurations, as files of the project (path, contents).
pub fn config_files(topo: &Topology) -> Vec<(String, String)> {
    topo.nodes
        .iter()
        .filter_map(|(name, n)| match &n.kind {
            Kind::Container {
                appliance: Some(_),
                config: Some(c),
                ..
            } if !c.trim().is_empty() => Some((
                config_path(name),
                if c.ends_with('\n') {
                    c.clone()
                } else {
                    format!("{c}\n")
                },
            )),
            _ => None,
        })
        .collect()
}
