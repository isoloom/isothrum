//! A switch's VLANs, nested under the LAN named after the switch (Isoloom's `vlans`).

use indexmap::IndexMap;
use isoloom_core::{Network, Reach, Vlan};

use crate::draft::{Draft, network};
use crate::{CompileError, Lan, Topology};

/// The name a VLAN LAN has in the spec: `<switch>.vlan<id>` (`None` for a plain LAN).
pub fn vlan_ref(lan: &Lan) -> Option<String> {
    Some(format!("{}.vlan{}", lan.switch.as_ref()?, lan.vlan?))
}

/// Refuses a switch named like a plain LAN: its VLANs nest under a LAN of the switch's name.
pub fn check_switch_names(topo: &Topology) -> Result<(), CompileError> {
    let plain = |name: &String| {
        topo.networks
            .get(name)
            .is_some_and(|l| vlan_ref(l).is_none())
    };
    match topo
        .networks
        .values()
        .find_map(|l| l.switch.as_ref().filter(|s| plain(s)))
    {
        Some(switch) => Err(CompileError::SwitchNameTaken {
            switch: switch.clone(),
        }),
        None => Ok(()),
    }
}

/// The switches' VLANs, nested under each switch's LAN: the LAN's block is the smallest one
/// holding them all, its `internet` the VLANs' when they agree. Machines and `reach` rules name
/// them `<switch>.vlan<id>`.
pub fn nest_vlans(topo: &Topology, draft: &mut Draft) {
    let renamed: IndexMap<&String, String> = topo
        .networks
        .iter()
        .filter_map(|(name, lan)| Some((name, vlan_ref(lan)?)))
        .collect();
    if renamed.is_empty() {
        return;
    }
    draft.networks = nested(topo, std::mem::take(&mut draft.networks));
    for parent in draft.networks.values_mut().filter(|n| !n.vlans.is_empty()) {
        settle_parent(parent);
    }
    rename_refs(draft, &renamed);
}

/// The networks with each VLAN moved under its switch's LAN.
fn nested(topo: &Topology, flat: IndexMap<String, Network>) -> IndexMap<String, Network> {
    let mut networks: IndexMap<String, Network> = IndexMap::new();
    for (name, net) in flat {
        let Some(lan) = topo.networks.get(&name).filter(|l| vlan_ref(l).is_some()) else {
            networks.insert(name, net);
            continue;
        };
        let (switch, id) = (lan.switch.clone().unwrap(), lan.vlan.unwrap());
        let parent = networks
            .entry(switch)
            .or_insert_with(|| network(String::new(), net.internet, None));
        if lan.device.is_some() {
            parent.switch = lan.device.clone();
        }
        let vlan = Vlan {
            cidr: net.cidr,
            internet: Some(net.internet),
        };
        parent.vlans.insert(id, vlan);
    }
    networks
}

/// A switch's LAN: the block covering its VLANs, and their `internet` said once when they agree.
fn settle_parent(parent: &mut Network) {
    parent.cidr = covering(parent.vlans.values().map(|v| v.cidr.as_str()));
    let all: Vec<bool> = parent.vlans.values().filter_map(|v| v.internet).collect();
    if all.iter().all(|i| *i == all[0]) {
        parent.internet = all[0];
        for v in parent.vlans.values_mut() {
            v.internet = None;
        }
    } else {
        parent.internet = true;
    }
}

/// Machines' networks and `reach` rules, with each VLAN LAN under its spec name.
fn rename_refs(draft: &mut Draft, renamed: &IndexMap<&String, String>) {
    let rename = |n: &String| renamed.get(n).cloned().unwrap_or_else(|| n.clone());
    for m in draft.machines.values_mut() {
        m.networks = m.networks.iter().map(|(n, o)| (rename(n), *o)).collect();
    }
    for r in &mut draft.reach {
        *r = Reach {
            from: rename(&r.from),
            to: rename(&r.to),
            ports: std::mem::take(&mut r.ports),
        };
    }
}

/// The smallest IPv4 block holding every one of `cidrs` (at most a /8).
fn covering<'a>(cidrs: impl Iterator<Item = &'a str>) -> String {
    let parse = |c: &str| -> Option<(u32, u8)> {
        let (a, l) = c.split_once('/')?;
        Some((
            u32::from(a.parse::<std::net::Ipv4Addr>().ok()?),
            l.parse().ok()?,
        ))
    };
    let blocks: Vec<(u32, u8)> = cidrs.filter_map(parse).collect();
    let Some(&(first, _)) = blocks.first() else {
        return String::new();
    };
    let mut len = blocks.iter().map(|(_, l)| *l).min().unwrap_or(24);
    let mask = |l: u8| if l == 0 { 0 } else { u32::MAX << (32 - l) };
    while len > 8
        && !blocks
            .iter()
            .all(|(b, _)| b & mask(len) == first & mask(len))
    {
        len -= 1;
    }
    format!("{}/{len}", std::net::Ipv4Addr::from(first & mask(len)))
}
