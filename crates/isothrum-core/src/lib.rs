//! A lab as drawn in the editor: nodes (containers or VMs), the LANs drawn around them, and the
//! cables between their ports.
//!
//! [`compile`] turns a [`Topology`] into an Isoloom spec:
//! - a LAN (an area drawn on the canvas, a VLAN in practice) is an Isoloom network with its own
//!   CIDR; each machine on it gets an address there, the next free one unless it asks for one;
//! - a cable between two machines is its own point-to-point network (a /29 cut from
//!   `10.200.0.0/16`, the two ends on its first two usable addresses).
//!
//! Routing is not decided here: Isoloom routes between its networks through its own router, and
//! `reach` rules are the topology's to add.

mod addressing;
mod draft;
mod error;
mod machines;
mod model;
mod routers;
pub mod starters;
mod vlans;
mod yaml;

use indexmap::IndexMap;
use isoloom_core::Spec;

use draft::Draft;
pub use error::CompileError;
pub use machines::{config_files, config_path, firmware_path};
pub use model::{Endpoint, Kind, Lan, Link, Node, Topology};
pub use vlans::vlan_ref;
pub use yaml::to_yaml;

/// Compiles a topology into an Isoloom spec. The result is not validated yet: run
/// `isoloom_core::validate` on it, which catches what the topology can't express on its own.
pub fn compile(topo: &Topology) -> Result<Spec, CompileError> {
    addressing::check_link_count(topo)?;
    vlans::check_switch_names(topo)?;
    let mut draft = Draft::default();
    machines::add_machines(topo, &mut draft);
    addressing::address_lans(topo, &mut draft)?;
    addressing::allocate_cables(topo, &mut draft)?;
    routers::add_reach_rules(&mut draft);
    machines::cisco_gateways(topo, &mut draft);
    vlans::nest_vlans(topo, &mut draft);
    Ok(spec(topo, draft))
}

/// The spec of the finished draft.
fn spec(topo: &Topology, draft: Draft) -> Spec {
    Spec {
        version: 1,
        name: topo.name.clone(),
        networks: draft.networks,
        reach: draft.reach,
        inputs: vec![],
        machines: draft.machines,
        common: None,
        groups: IndexMap::new(),
        provision: vec![],
        checks: vec![],
        targets: None,
        message: None,
        tools: IndexMap::new(),
        clones: IndexMap::new(),
    }
}

#[cfg(test)]
mod tests;
