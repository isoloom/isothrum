//! Addresses: each machine's on its LANs, and the cables' point-to-point blocks.

use indexmap::IndexMap;

use crate::draft::{Draft, network};
use crate::{CompileError, Lan, Link, Topology};

/// The block the cables are cut from: `10.200.0.0/16`, one /29 per cable. Isoloom takes a /24 to
/// /29 per network, so the smallest one is the /29, with two usable ends (.2 and .3).
pub const LINK_BASE: [u8; 2] = [10, 200];
const LINKS_PER_THIRD: usize = 32;
/// A /16 holds 32 /29 blocks in each /24.
const MAX_LINKS: usize = 256 * LINKS_PER_THIRD;

/// The network name of the `i`th cable.
fn link_network(i: usize) -> String {
    format!("link-{i}")
}

/// The /29 of the `i`th cable: its network address and the last octets of its two ends.
fn link_block(i: usize) -> (String, u8, u8) {
    let third = (i / LINKS_PER_THIRD) as u8;
    let fourth = ((i % LINKS_PER_THIRD) * 8) as u8;
    (
        format!("{}.{}.{third}.{fourth}/29", LINK_BASE[0], LINK_BASE[1]),
        fourth + 2,
        fourth + 3,
    )
}

/// The addresses a machine may take on a block of `prefix` bits, lowest first: Isoloom keeps the
/// network address and .1 (the gateway), the two last ones (its controller and router) and the
/// broadcast. A /24 or larger starts at .10, leaving room for the infrastructure people expect
/// low in a LAN.
fn usable(prefix: u8) -> std::ops::RangeInclusive<u8> {
    let size: u32 = 1 << (32 - u32::from(prefix.clamp(24, 30)));
    let first = if size >= 256 { 10 } else { 2 };
    let last = (size - 4) as u8;
    first..=last
}

/// The prefix length of `a.b.c.d/n`.
fn prefix_of(cidr: &str) -> Option<u8> {
    let (addr, len) = cidr.split_once('/')?;
    let ok = addr.split('.').filter_map(|o| o.parse::<u8>().ok()).count() == 4;
    let len = len.parse::<u8>().ok()?;
    (ok && len <= 32).then_some(len)
}

/// Refuses more cables than the cables' block holds.
pub fn check_link_count(topo: &Topology) -> Result<(), CompileError> {
    match topo.links.len() {
        n if n > MAX_LINKS => Err(CompileError::TooManyLinks(n)),
        _ => Ok(()),
    }
}

/// Each LAN becomes a network, and each machine on it gets its address there.
pub fn address_lans(topo: &Topology, draft: &mut Draft) -> Result<(), CompileError> {
    for (name, lan) in &topo.networks {
        address_lan(topo, draft, name, lan)?;
    }
    check_lans_known(topo)
}

fn address_lan(
    topo: &Topology,
    draft: &mut Draft,
    name: &str,
    lan: &Lan,
) -> Result<(), CompileError> {
    if name.starts_with("link-") {
        return Err(CompileError::ReservedLanName(name.into()));
    }
    let prefix = prefix_of(&lan.cidr).ok_or_else(|| CompileError::BadCidr {
        lan: name.into(),
        cidr: lan.cidr.clone(),
    })?;
    draft.networks.insert(
        name.into(),
        network(lan.cidr.clone(), lan.internet, lan.tc.clone()),
    );
    let taken = asked_addresses(topo, draft, name)?;
    let mut free = usable(prefix).filter(|o| !taken.contains_key(o));
    for node in members(topo, draft, name, |octet| octet.is_none()) {
        let octet = free.next().ok_or_else(|| CompileError::LanFull {
            lan: name.into(),
            cidr: lan.cidr.clone(),
        })?;
        draft.machines[&node].networks.insert(name.into(), octet);
    }
    Ok(())
}

/// The machines on `lan` whose asked-for octet passes `keep`, in the topology's order.
fn members(
    topo: &Topology,
    draft: &Draft,
    lan: &str,
    keep: impl Fn(Option<u8>) -> bool,
) -> Vec<String> {
    topo.nodes
        .iter()
        .filter(|(node, _)| !draft.routers.contains_key(*node))
        .filter(|(_, n)| n.networks.get(lan).is_some_and(|o| keep(*o)))
        .map(|(node, _)| node.clone())
        .collect()
}

/// The addresses machines ask for on `lan`, given to them first: octet -> machine.
fn asked_addresses(
    topo: &Topology,
    draft: &mut Draft,
    lan: &str,
) -> Result<IndexMap<u8, String>, CompileError> {
    let mut taken: IndexMap<u8, String> = IndexMap::new();
    for node in members(topo, draft, lan, |octet| octet.is_some()) {
        let octet = topo.nodes[&node].networks[lan].expect("asked for");
        if let Some(other) = taken.insert(octet, node.clone()) {
            return Err(CompileError::AddressTaken {
                lan: lan.into(),
                octet,
                nodes: (other, node),
            });
        }
        draft.machines[&node].networks.insert(lan.into(), octet);
    }
    Ok(taken)
}

/// Refuses a node on a LAN the lab doesn't have.
fn check_lans_known(topo: &Topology) -> Result<(), CompileError> {
    for (node, n) in &topo.nodes {
        if let Some(lan) = n.networks.keys().find(|l| !topo.networks.contains_key(*l)) {
            return Err(CompileError::UnknownLan {
                node: node.clone(),
                lan: lan.clone(),
            });
        }
    }
    Ok(())
}

/// Each cable becomes its own /29 network, its two ends on it.
pub fn allocate_cables(topo: &Topology, draft: &mut Draft) -> Result<(), CompileError> {
    let mut used_ports: Vec<(&str, &str)> = Vec::new();
    for (i, link) in topo.links.iter().enumerate() {
        check_cable(topo, i, link, &mut used_ports)?;
        cable(draft, i, link);
    }
    Ok(())
}

/// Refuses a cable to an unknown node, on a port already cabled, or from a node to itself.
fn check_cable<'a>(
    topo: &Topology,
    i: usize,
    link: &'a Link,
    used_ports: &mut Vec<(&'a str, &'a str)>,
) -> Result<(), CompileError> {
    for end in [&link.a, &link.b] {
        if !topo.nodes.contains_key(&end.node) {
            return Err(CompileError::UnknownNode {
                link: i,
                node: end.node.clone(),
            });
        }
        if used_ports.contains(&(end.node.as_str(), end.port.as_str())) {
            return Err(CompileError::PortInUse {
                node: end.node.clone(),
                port: end.port.clone(),
            });
        }
        used_ports.push((end.node.as_str(), end.port.as_str()));
    }
    if link.a.node == link.b.node {
        return Err(CompileError::SelfLink {
            link: i,
            node: link.a.node.clone(),
        });
    }
    Ok(())
}

/// The `i`th cable's network; an automatic router at an end joins it, a machine gets its address.
fn cable(draft: &mut Draft, i: usize, link: &Link) {
    // A cable between two automatic routers carries nothing of its own.
    if draft.routers.contains_key(&link.a.node) && draft.routers.contains_key(&link.b.node) {
        return;
    }
    let net = link_network(i);
    let (cidr, a_octet, b_octet) = link_block(i);
    draft
        .networks
        .insert(net.clone(), network(cidr, false, link.tc.clone()));
    for (end, octet) in [(&link.a.node, a_octet), (&link.b.node, b_octet)] {
        match draft.routers.get_mut(end) {
            Some(nets) => nets.push(net.clone()),
            None => {
                draft.machines[end].networks.insert(net.clone(), octet);
            }
        }
    }
}
