//! A lab as drawn in the editor: nodes (containers or VMs) and the cables between their ports.
//!
//! [`compile`] turns a [`Topology`] into an Isoloom spec. Every cable becomes its own
//! point-to-point network (a /30 with the two ends on .1 and .2), so the environment runs on any
//! target Isoloom can produce. Routing is not decided here: Isoloom routes between its networks
//! through its own router, and `reach` rules are the topology's to add.

use indexmap::IndexMap;
use isoloom_core::model::{DockerImpl, VmImpl};
use isoloom_core::{Machine, Network, Service, Spec};
use serde::{Deserialize, Serialize};

/// The block the cables are cut from: `10.200.0.0/16`, one /29 per cable. Isoloom takes a /24 to
/// /29 per network, so the smallest one is the /29, with two usable ends (.2 and .3).
const LINK_BASE: [u8; 2] = [10, 200];
const LINKS_PER_THIRD: usize = 32;
/// A /16 holds 32 /29 blocks in each /24.
const MAX_LINKS: usize = 256 * LINKS_PER_THIRD;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Topology {
    /// Kebab-case id of the lab, becomes the spec's `name`.
    pub name: String,
    pub nodes: IndexMap<String, Node>,
    #[serde(default)]
    pub links: Vec<Link>,
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
}

/// How a node is produced. Both kinds are first-class: a Docker container or a VM.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Kind {
    Container { image: String },
    Vm {
        os: String,
        /// Steps run inside the VM, in order (`.sh` scripts, or Ansible `.yml`).
        #[serde(default)]
        provision: Vec<String>,
    },
}

/// A cable between two ports. A port carries at most one cable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub a: Endpoint,
    pub b: Endpoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub node: String,
    /// The port's name on the node (`eth0`, `GigabitEthernet0/1`, ...). Free text for now.
    pub port: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    UnknownNode { link: usize, node: String },
    SelfLink { link: usize, node: String },
    PortInUse { node: String, port: String },
    TooManyLinks(usize),
    Spec(String),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::UnknownNode { link, node } => {
                write!(f, "link {link} refers to node `{node}`, which the lab doesn't have")
            }
            CompileError::SelfLink { link, node } => {
                write!(f, "link {link} cables `{node}` to itself")
            }
            CompileError::PortInUse { node, port } => {
                write!(f, "port `{port}` of `{node}` already has a cable")
            }
            CompileError::TooManyLinks(n) => {
                write!(f, "{n} cables don't fit in {}.{}.0.0/16", LINK_BASE[0], LINK_BASE[1])
            }
            CompileError::Spec(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for CompileError {}

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

/// Compiles a topology into an Isoloom spec. The result is not validated yet: run
/// `isoloom_core::validate` on it, which catches what the topology can't express on its own.
pub fn compile(topo: &Topology) -> Result<Spec, CompileError> {
    if topo.links.len() > MAX_LINKS {
        return Err(CompileError::TooManyLinks(topo.links.len()));
    }

    let mut machines: IndexMap<String, Machine> = IndexMap::new();
    for (name, node) in &topo.nodes {
        let mut m = Machine {
            access: node.access,
            services: node.services.clone(),
            ..Machine::default()
        };
        match &node.kind {
            Kind::Container { image } => {
                m.docker = Some(DockerImpl { image: Some(image.clone()), build: None, init: vec![] });
            }
            Kind::Vm { os, provision } => {
                m.vm = Some(VmImpl { os: os.clone(), provision: provision.clone(), image: None });
            }
        }
        machines.insert(name.clone(), m);
    }

    let mut networks: IndexMap<String, Network> = IndexMap::new();
    let mut used_ports: Vec<(&str, &str)> = Vec::new();
    for (i, link) in topo.links.iter().enumerate() {
        for end in [&link.a, &link.b] {
            if !topo.nodes.contains_key(&end.node) {
                return Err(CompileError::UnknownNode { link: i, node: end.node.clone() });
            }
            if used_ports.contains(&(end.node.as_str(), end.port.as_str())) {
                return Err(CompileError::PortInUse { node: end.node.clone(), port: end.port.clone() });
            }
            used_ports.push((end.node.as_str(), end.port.as_str()));
        }
        if link.a.node == link.b.node {
            return Err(CompileError::SelfLink { link: i, node: link.a.node.clone() });
        }

        let net = link_network(i);
        let (cidr, a_octet, b_octet) = link_block(i);
        networks.insert(
            net.clone(),
            Network { cidr, internet: false, gateway: None, docker: None },
        );
        machines[&link.a.node].networks.insert(net.clone(), a_octet);
        machines[&link.b.node].networks.insert(net, b_octet);
    }

    Ok(Spec {
        version: 1,
        name: topo.name.clone(),
        networks,
        reach: vec![],
        inputs: vec![],
        machines,
        provision: vec![],
        checks: vec![],
        targets: None,
    })
}

/// The spec as YAML, ready to write to `isoloom.yml`: only what differs from Isoloom's defaults,
/// as a person would write it.
pub fn to_yaml(spec: &Spec) -> Result<String, CompileError> {
    let mut value = serde_yaml_ng::to_value(spec).map_err(|e| CompileError::Spec(e.to_string()))?;
    strip_defaults(&mut value);
    serde_yaml_ng::to_string(&value).map_err(|e| CompileError::Spec(e.to_string()))
}

/// Fields whose value is Isoloom's default, so leaving them out means the same thing.
/// `internet` is not here: it defaults to true, and every cable sets it to false.
const DEFAULT_FALSE: &[&str] = &["privileged", "read_only", "access", "http"];

fn strip_defaults(value: &mut serde_yaml_ng::Value) {
    use serde_yaml_ng::Value;
    match value {
        Value::Mapping(map) => {
            map.retain(|k, v| {
                let key = k.as_str().unwrap_or_default();
                !(v.is_null()
                    || v.as_sequence().is_some_and(|s| s.is_empty())
                    || v.as_mapping().is_some_and(|m| m.is_empty())
                    || (DEFAULT_FALSE.contains(&key) && v.as_bool() == Some(false))
                    || (key == "arch" && v.as_str() == Some("amd64")))
            });
            for v in map.values_mut() {
                strip_defaults(v);
            }
        }
        Value::Sequence(seq) => seq.iter_mut().for_each(strip_defaults),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(kind: Kind) -> Node {
        Node { kind, access: false, services: vec![] }
    }

    fn ep(node: &str, port: &str) -> Endpoint {
        Endpoint { node: node.into(), port: port.into() }
    }

    /// client -- router -- server, the router being a container and the server a VM.
    fn three_node() -> Topology {
        let mut nodes = IndexMap::new();
        nodes.insert("client".into(), Node { access: true, ..node(Kind::Vm { os: "debian-12".into(), provision: vec!["provision/x.sh".into()] }) });
        nodes.insert("router".into(), node(Kind::Container { image: "frrouting/frr:v8.5.2".into() }));
        nodes.insert("server".into(), node(Kind::Vm { os: "debian-12".into(), provision: vec!["provision/x.sh".into()] }));
        Topology {
            name: "three-node".into(),
            nodes,
            links: vec![
                Link { a: ep("client", "eth0"), b: ep("router", "eth0") },
                Link { a: ep("router", "eth1"), b: ep("server", "eth0") },
            ],
        }
    }

    #[test]
    fn each_cable_is_a_point_to_point_network() {
        let spec = compile(&three_node()).unwrap();
        assert_eq!(spec.networks.len(), 2);
        assert_eq!(spec.networks["link-0"].cidr, "10.200.0.0/29");
        assert_eq!(spec.networks["link-1"].cidr, "10.200.0.8/29");
        assert!(!spec.networks["link-0"].internet);
    }

    #[test]
    fn the_router_gets_one_address_per_cable() {
        let spec = compile(&three_node()).unwrap();
        let router = &spec.machines["router"];
        assert_eq!(router.networks["link-0"], 3);
        assert_eq!(router.networks["link-1"], 10);
        assert_eq!(spec.machines["client"].networks["link-0"], 2);
        assert_eq!(spec.machines["server"].networks["link-1"], 11);
    }

    #[test]
    fn nodes_keep_their_kind_and_access() {
        let spec = compile(&three_node()).unwrap();
        assert_eq!(spec.machines["client"].access, true);
        assert_eq!(spec.machines["router"].docker.as_ref().unwrap().image.as_deref(), Some("frrouting/frr:v8.5.2"));
        assert_eq!(spec.machines["server"].vm.as_ref().unwrap().os, "debian-12");
        assert!(spec.machines["server"].docker.is_none());
    }

    #[test]
    fn the_compiled_spec_is_a_valid_isoloom_spec() {
        let spec = compile(&three_node()).unwrap();
        let yaml = to_yaml(&spec).unwrap();
        let reparsed = isoloom_core::parse(&yaml).expect("the output parses as an Isoloom spec");
        let problems = isoloom_core::validate(&reparsed);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn the_yaml_leaves_defaults_out() {
        let yaml = to_yaml(&compile(&three_node()).unwrap()).unwrap();
        for noise in ["null", "[]", "{}", "privileged", "read_only", "arch", "http"] {
            assert!(!yaml.contains(noise), "`{noise}` in:\n{yaml}");
        }
        assert!(yaml.contains("internet: false"));
        assert!(yaml.contains("access: true"));
    }

    #[test]
    fn a_port_carries_one_cable() {
        let mut topo = three_node();
        topo.links.push(Link { a: ep("client", "eth0"), b: ep("server", "eth1") });
        assert_eq!(
            compile(&topo),
            Err(CompileError::PortInUse { node: "client".into(), port: "eth0".into() })
        );
    }

    #[test]
    fn a_cable_needs_two_known_nodes() {
        let mut topo = three_node();
        topo.links.push(Link { a: ep("client", "eth1"), b: ep("ghost", "eth0") });
        assert_eq!(compile(&topo), Err(CompileError::UnknownNode { link: 2, node: "ghost".into() }));
    }

    #[test]
    fn a_node_can_not_be_cabled_to_itself() {
        let mut topo = three_node();
        topo.links = vec![Link { a: ep("router", "eth0"), b: ep("router", "eth1") }];
        assert_eq!(compile(&topo), Err(CompileError::SelfLink { link: 0, node: "router".into() }));
    }

    #[test]
    fn many_cables_get_distinct_blocks() {
        let mut topo = three_node();
        topo.nodes.shift_remove("router");
        topo.links.clear();
        for i in 0..100 {
            topo.links.push(Link {
                a: ep("client", &format!("p{i}")),
                b: ep("server", &format!("p{i}")),
            });
        }
        let spec = compile(&topo).unwrap();
        assert_eq!(spec.networks["link-99"].cidr, "10.200.3.24/29");
        let problems = isoloom_core::validate(&spec);
        assert!(problems.is_empty(), "{problems:?}");
    }
}
