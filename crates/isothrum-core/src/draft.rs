//! The spec while `compile` builds it, step by step.

use indexmap::IndexMap;
use isoloom_spec::{Machine, Network, Reach, Tc};

/// The spec's networks, machines and `reach` rules so far, and the automatic routers' networks.
#[derive(Default)]
pub struct Draft {
    pub networks: IndexMap<String, Network>,
    pub machines: IndexMap<String, Machine>,
    /// Automatic routers aren't machines: only the networks they join count.
    pub routers: IndexMap<String, Vec<String>>,
    pub reach: Vec<Reach>,
}

/// A plain network: a block, whether it reaches the internet, how its link behaves.
pub fn network(cidr: String, internet: bool, tc: Option<Tc>) -> Network {
    Network {
        cidr,
        internet,
        gateway: None,
        docker: None,
        vlans: IndexMap::new(),
        tc,
        vlan: None,
        switch: None,
        ipv6: None,
        external: None,
        bridged: None,
        mtu: None,
    }
}
