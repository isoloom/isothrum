//! The topology the editor sends: nodes, the LANs drawn around them, the cables between their
//! ports.

use indexmap::IndexMap;
use isoloom_core::{Service, Tc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Topology {
    /// Kebab-case id of the lab, becomes the spec's `name`.
    pub name: String,
    pub nodes: IndexMap<String, Node>,
    /// The LANs: name -> its block. Machines join one through [`Node::networks`].
    #[serde(default)]
    pub networks: IndexMap<String, Lan>,
    #[serde(default)]
    pub links: Vec<Link>,
}

/// A LAN (or VLAN): one shared segment for every machine on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lan {
    /// An IPv4 block inside 10.0.0.0/8, /24 to /29 (Isoloom's rule), e.g. `10.10.1.0/24`.
    pub cidr: String,
    /// Whether machines on it may reach the internet (Isoloom's default: yes).
    #[serde(default = "yes")]
    pub internet: bool,
    /// A VLAN of a switch: the switch (or the first of the switches a trunk joins) it belongs to,
    /// and its id. In the spec it nests under that switch's LAN (`sw1.vlan10`).
    #[serde(default)]
    pub switch: Option<String>,
    #[serde(default)]
    pub vlan: Option<u16>,
    /// The switch's machine when it runs a Cisco OS (a node of its own, with no networks):
    /// Isoloom's `switch` on the LAN its VLANs nest under.
    #[serde(default)]
    pub device: Option<String>,
    /// How the LAN behaves (a slow or lossy link): Isoloom's `tc`.
    #[serde(default)]
    pub tc: Option<Tc>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub kind: Kind,
    /// The node the user lands on (the runner supplies it, see Isoloom's `access`).
    #[serde(default)]
    pub access: bool,
    #[serde(default)]
    pub services: Vec<Service>,
    /// The LANs this node is on: LAN name -> the last octet of its address there, or `None` for
    /// the next free one.
    #[serde(default)]
    pub networks: IndexMap<String, Option<u8>>,
}

/// How a node is produced. Both kinds are first-class: a Docker container or a VM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Kind {
    Container {
        image: String,
        /// The image runs no service of its own (a stock Linux image): keep it running idle, as a
        /// machine to work from (Isoloom's `docker.idle`).
        #[serde(default)]
        idle: bool,
        /// A network OS Isoloom wires as its image expects (Cisco IOL, as vrnetlab builds it).
        #[serde(default)]
        appliance: Option<isoloom_core::Appliance>,
        /// The appliance's own configuration (IOS commands), written to `configs/<name>.cfg`.
        #[serde(default)]
        config: Option<String>,
    },
    Vm {
        os: String,
        /// Steps run inside the VM, in order (`.sh` scripts, or Ansible `.yml`).
        #[serde(default)]
        provision: Vec<String>,
    },
    /// An automatic router: not a machine. The networks it is on (LANs, VLANs, cables) can all
    /// reach each other, through Isoloom's own router (`reach` rules both ways); nothing to
    /// configure. A router you configure yourself is a container or a VM (FRR, for instance).
    Router {},
}

/// A cable between two ports. A port carries at most one cable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub a: Endpoint,
    pub b: Endpoint,
    /// How the cable behaves (latency, jitter, loss, bandwidth): Isoloom's `tc` on its network.
    #[serde(default)]
    pub tc: Option<Tc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub node: String,
    /// The port's name on the node (`eth0`, `GigabitEthernet0/1`, ...). Free text for now.
    pub port: String,
}
