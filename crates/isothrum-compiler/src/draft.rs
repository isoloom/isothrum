//! The spec while `compile` builds it, step by step.

use indexmap::IndexMap;

use crate::controller::ControllerPlaybook;
use crate::document::{MachineDoc, NetworkDoc, ReachDoc};

/// The spec's networks, machines, `reach` rules and controller playbooks so far, and the automatic
/// routers' networks.
#[derive(Debug, Default)]
pub(crate) struct Draft {
    pub(crate) networks: IndexMap<String, NetworkDoc>,
    pub(crate) machines: IndexMap<String, MachineDoc>,
    /// Automatic routers aren't machines: only the networks they join count.
    pub(crate) routers: IndexMap<String, Vec<String>>,
    pub(crate) reach: Vec<ReachDoc>,
    /// The playbooks Isoloom's controller runs, in the order the machines name them.
    pub(crate) controller: Vec<ControllerPlaybook>,
}

impl Draft {
    /// Machine `node` at `octet` on network `net`; an automatic router only joins the network.
    pub(crate) fn attach(&mut self, node: &str, net: &str, octet: u8) {
        if let Some(nets) = self.routers.get_mut(node) {
            nets.push(net.to_owned());
        } else if let Some(m) = self.machines.get_mut(node) {
            m.networks.insert(net.to_owned(), octet);
        }
    }

    /// Whether `node` is an automatic router.
    pub(crate) fn is_router(&self, node: &str) -> bool {
        self.routers.contains_key(node)
    }
}
