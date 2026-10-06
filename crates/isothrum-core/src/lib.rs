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

use indexmap::IndexMap;
use isoloom_core::model::{DockerImpl, VmImpl};
use isoloom_core::{Machine, Network, Reach, Service, Spec};
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
    /// A node is on a LAN the lab doesn't have.
    UnknownLan { node: String, lan: String },
    /// A LAN's name is one the cables use (`link-<n>`).
    ReservedLanName(String),
    /// A LAN's CIDR isn't `a.b.c.d/n`.
    BadCidr { lan: String, cidr: String },
    /// Two nodes ask for the same address on a LAN.
    AddressTaken { lan: String, octet: u8, nodes: (String, String) },
    /// A LAN has more machines than free addresses.
    LanFull { lan: String, cidr: String },
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
            CompileError::UnknownLan { node, lan } => {
                write!(f, "`{node}` is on LAN `{lan}`, which the lab doesn't have")
            }
            CompileError::ReservedLanName(lan) => {
                write!(f, "`{lan}` is a name the cables use; call the LAN something else")
            }
            CompileError::BadCidr { lan, cidr } => {
                write!(f, "LAN `{lan}`: `{cidr}` isn't a block like 10.10.1.0/24")
            }
            CompileError::AddressTaken { lan, octet, nodes: (a, b) } => {
                write!(f, "on LAN `{lan}`, `{a}` and `{b}` both ask for .{octet}")
            }
            CompileError::LanFull { lan, cidr } => {
                write!(f, "LAN `{lan}` ({cidr}) has no free address left; use a larger block")
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

/// Compiles a topology into an Isoloom spec. The result is not validated yet: run
/// `isoloom_core::validate` on it, which catches what the topology can't express on its own.
pub fn compile(topo: &Topology) -> Result<Spec, CompileError> {
    if topo.links.len() > MAX_LINKS {
        return Err(CompileError::TooManyLinks(topo.links.len()));
    }

    let mut machines: IndexMap<String, Machine> = IndexMap::new();
    // Automatic routers aren't machines: only the networks they join count.
    let mut routers: IndexMap<String, Vec<String>> = IndexMap::new();
    for (name, node) in &topo.nodes {
        if matches!(node.kind, Kind::Router {}) {
            routers.insert(name.clone(), node.networks.keys().cloned().collect());
            continue;
        }
        let mut m = Machine {
            access: node.access,
            services: node.services.clone(),
            ..Machine::default()
        };
        match &node.kind {
            Kind::Container { image, idle } => {
                m.docker = Some(DockerImpl { image: Some(image.clone()), build: None, init: vec![], idle: *idle });
            }
            Kind::Router {} => unreachable!("routers are skipped above"),
            Kind::Vm { os, provision } => {
                m.vm = Some(VmImpl { os: os.clone(), provision: provision.clone(), image: None });
            }
        }
        machines.insert(name.clone(), m);
    }

    let mut networks: IndexMap<String, Network> = IndexMap::new();
    for (name, lan) in &topo.networks {
        if name.starts_with("link-") {
            return Err(CompileError::ReservedLanName(name.clone()));
        }
        let prefix = prefix_of(&lan.cidr)
            .ok_or_else(|| CompileError::BadCidr { lan: name.clone(), cidr: lan.cidr.clone() })?;
        networks.insert(
            name.clone(),
            Network { cidr: lan.cidr.clone(), internet: lan.internet, gateway: None, docker: None },
        );

        // Asked-for addresses first, then the others take the free ones in order.
        let mut taken: IndexMap<u8, String> = IndexMap::new();
        for (node, n) in topo.nodes.iter().filter(|(node, _)| !routers.contains_key(*node)) {
            if let Some(Some(octet)) = n.networks.get(name) {
                if let Some(other) = taken.insert(*octet, node.clone()) {
                    return Err(CompileError::AddressTaken {
                        lan: name.clone(),
                        octet: *octet,
                        nodes: (other, node.clone()),
                    });
                }
                machines[node].networks.insert(name.clone(), *octet);
            }
        }
        let mut free = usable(prefix).filter(|o| !taken.contains_key(o));
        for (node, n) in topo.nodes.iter().filter(|(node, _)| !routers.contains_key(*node)) {
            if let Some(None) = n.networks.get(name) {
                let octet = free
                    .next()
                    .ok_or_else(|| CompileError::LanFull { lan: name.clone(), cidr: lan.cidr.clone() })?;
                machines[node].networks.insert(name.clone(), octet);
            }
        }
    }
    for (node, n) in &topo.nodes {
        if let Some(lan) = n.networks.keys().find(|l| !topo.networks.contains_key(*l)) {
            return Err(CompileError::UnknownLan { node: node.clone(), lan: lan.clone() });
        }
    }

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
        // A cable between two automatic routers carries nothing of its own.
        if routers.contains_key(&link.a.node) && routers.contains_key(&link.b.node) {
            continue;
        }
        networks.insert(
            net.clone(),
            Network { cidr, internet: false, gateway: None, docker: None },
        );
        for (end, octet) in [(&link.a.node, a_octet), (&link.b.node, b_octet)] {
            match routers.get_mut(end) {
                Some(nets) => nets.push(net.clone()),
                None => {
                    machines[end].networks.insert(net.clone(), octet);
                }
            }
        }
    }

    // Each automatic router lets every pair of its networks reach each other, both ways.
    let mut reach: Vec<Reach> = Vec::new();
    for nets in routers.values() {
        for from in nets {
            for to in nets {
                if from != to && !reach.iter().any(|r| &r.from == from && &r.to == to) {
                    reach.push(Reach { from: from.clone(), to: to.clone(), ports: vec![] });
                }
            }
        }
    }

    Ok(Spec {
        version: 1,
        name: topo.name.clone(),
        networks,
        reach,
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
const DEFAULT_FALSE: &[&str] = &["privileged", "read_only", "access", "http", "idle"];

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
        Node { kind, access: false, services: vec![], networks: IndexMap::new() }
    }

    fn lan(cidr: &str) -> Lan {
        Lan { cidr: cidr.into(), internet: true }
    }

    /// Two LANs, office and servers, with the router on both (at .254 on office).
    fn two_lans() -> Topology {
        let mut topo = three_node();
        topo.links.clear();
        topo.networks.insert("office".into(), lan("10.10.1.0/24"));
        topo.networks.insert("servers".into(), lan("10.10.2.0/24"));
        topo.nodes["client"].networks.insert("office".into(), None);
        topo.nodes["server"].networks.insert("servers".into(), None);
        topo.nodes["router"].networks.insert("office".into(), Some(254));
        topo.nodes["router"].networks.insert("servers".into(), None);
        topo
    }

    fn ep(node: &str, port: &str) -> Endpoint {
        Endpoint { node: node.into(), port: port.into() }
    }

    /// client -- router -- server, the router being a container and the server a VM.
    fn three_node() -> Topology {
        let mut nodes = IndexMap::new();
        nodes.insert("client".into(), Node { access: true, ..node(Kind::Vm { os: "debian-12".into(), provision: vec!["provision/x.sh".into()] }) });
        nodes.insert("router".into(), node(Kind::Container { image: "frrouting/frr:v8.5.2".into(), idle: false }));
        nodes.insert("server".into(), node(Kind::Vm { os: "debian-12".into(), provision: vec!["provision/x.sh".into()] }));
        Topology {
            name: "three-node".into(),
            nodes,
            networks: IndexMap::new(),
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
    fn machines_on_a_lan_take_its_free_addresses() {
        let mut topo = two_lans();
        topo.nodes["router"].networks.insert("office".into(), Some(10));
        let spec = compile(&topo).unwrap();
        assert_eq!(spec.networks["office"].cidr, "10.10.1.0/24");
        // The router asked for .10 on office, so the client takes the next one.
        assert_eq!(spec.machines["router"].networks["office"], 10);
        assert_eq!(spec.machines["client"].networks["office"], 11);
        assert_eq!(spec.machines["router"].networks["servers"], 10);
        assert_eq!(spec.machines["server"].networks["servers"], 11);
        let problems = isoloom_core::validate(&spec);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_small_lan_starts_low_and_fills_up() {
        let mut topo = two_lans();
        topo.nodes["router"].networks.shift_remove("office");
        topo.networks["office"].cidr = "10.10.1.0/29".into();
        topo.nodes["server"].networks.insert("office".into(), None);
        topo.nodes["router"].networks.insert("office".into(), None);
        let spec = compile(&topo).unwrap();
        // A /29 has .2 to .4 for machines.
        assert_eq!(spec.machines["client"].networks["office"], 2);
        assert_eq!(spec.machines["router"].networks["office"], 3);
        assert_eq!(spec.machines["server"].networks["office"], 4);
        let problems = isoloom_core::validate(&spec);
        assert!(problems.is_empty(), "{problems:?}");

        topo.nodes.insert("extra".into(), node(Kind::Container { image: "alpine:3.20".into(), idle: true }));
        topo.nodes["extra"].networks.insert("office".into(), None);
        assert!(matches!(compile(&topo), Err(CompileError::LanFull { .. })));
    }

    #[test]
    fn lan_mistakes_say_what_to_fix() {
        let mut topo = two_lans();
        topo.nodes["client"].networks.insert("dmz".into(), None);
        assert_eq!(compile(&topo), Err(CompileError::UnknownLan { node: "client".into(), lan: "dmz".into() }));

        let mut topo = two_lans();
        topo.nodes["client"].networks.insert("office".into(), Some(254));
        assert!(matches!(compile(&topo), Err(CompileError::AddressTaken { octet: 254, .. })));

        let mut topo = two_lans();
        topo.networks.insert("link-0".into(), lan("10.10.9.0/24"));
        assert_eq!(compile(&topo), Err(CompileError::ReservedLanName("link-0".into())));

        let mut topo = two_lans();
        topo.networks["office"].cidr = "office".into();
        assert!(matches!(compile(&topo), Err(CompileError::BadCidr { .. })));
    }

    #[test]
    fn lans_and_cables_mix() {
        let mut topo = two_lans();
        topo.nodes["router"].networks.insert("office".into(), None);
        topo.links.push(Link { a: ep("client", "eth1"), b: ep("server", "eth1") });
        let spec = compile(&topo).unwrap();
        assert_eq!(spec.networks.len(), 3);
        assert_eq!(spec.machines["client"].networks["link-0"], 2);
        let yaml = to_yaml(&spec).unwrap();
        let problems = isoloom_core::validate(&isoloom_core::parse(&yaml).unwrap());
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn an_idle_container_is_kept_running() {
        let mut topo = three_node();
        topo.nodes["router"].kind = Kind::Container { image: "alpine:3.20".into(), idle: true };
        let spec = compile(&topo).unwrap();
        assert!(spec.machines["router"].docker.as_ref().unwrap().idle);
        assert!(to_yaml(&spec).unwrap().contains("idle: true"));
    }

    #[test]
    fn an_automatic_router_lets_its_networks_reach_each_other() {
        let mut topo = two_lans();
        topo.nodes["router"].kind = Kind::Router {};
        topo.nodes["router"].networks.insert("office".into(), None);
        let spec = compile(&topo).unwrap();
        // Not a machine, and no address taken on its LANs.
        assert!(!spec.machines.contains_key("router"));
        assert_eq!(spec.machines["client"].networks["office"], 10);
        let pairs: Vec<(&str, &str)> = spec.reach.iter().map(|r| (r.from.as_str(), r.to.as_str())).collect();
        assert_eq!(pairs, vec![("office", "servers"), ("servers", "office")]);
        let problems = isoloom_core::validate(&spec);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn an_automatic_router_routes_its_cables_too() {
        let mut topo = two_lans();
        topo.nodes["router"].kind = Kind::Router {};
        topo.nodes["router"].networks.shift_remove("office");
        topo.links.push(Link { a: ep("router", "eth1"), b: ep("client", "eth1") });
        let spec = compile(&topo).unwrap();
        assert_eq!(spec.networks["link-0"].cidr, "10.200.0.0/29");
        assert_eq!(spec.machines["client"].networks["link-0"], 3);
        assert!(spec.reach.iter().any(|r| r.from == "link-0" && r.to == "servers"));
        let problems = isoloom_core::validate(&spec);
        assert!(problems.is_empty(), "{problems:?}");
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
