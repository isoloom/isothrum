use super::*;

mod cables;
mod lans;
mod machines;
mod switches;

fn node(kind: Kind) -> Node {
    Node {
        kind,
        access: false,
        services: vec![],
        networks: IndexMap::new(),
        environment: IndexMap::new(),
        files: vec![],
        command: None,
    }
}

fn lan(cidr: &str) -> Lan {
    Lan {
        cidr: cidr.into(),
        internet: true,
        switch: None,
        vlan: None,
        device: None,
        tc: None,
        ipv6: None,
    }
}

/// Two LANs, office and servers, with the router on both (at .254 on office).
fn two_lans() -> Topology {
    let mut topo = three_node();
    topo.links.clear();
    topo.networks.insert("office".into(), lan("10.10.1.0/24"));
    topo.networks.insert("servers".into(), lan("10.10.2.0/24"));
    topo.nodes["client"].networks.insert("office".into(), None);
    topo.nodes["server"].networks.insert("servers".into(), None);
    topo.nodes["router"]
        .networks
        .insert("office".into(), Some(254));
    topo.nodes["router"].networks.insert("servers".into(), None);
    topo
}

fn ep(node: &str, port: &str) -> Endpoint {
    Endpoint {
        node: node.into(),
        port: port.into(),
    }
}

/// client -- router -- server, the router being a container and the server a VM.
fn three_node() -> Topology {
    let mut nodes = IndexMap::new();
    nodes.insert(
        "client".into(),
        Node {
            access: true,
            ..node(Kind::Vm {
                os: "debian-12".into(),
                provision: vec!["provision/x.sh".into()],
            })
        },
    );
    nodes.insert(
        "router".into(),
        node(Kind::Container {
            image: "frrouting/frr:v8.5.2".into(),
            idle: false,
            appliance: None,
            config: None,
        }),
    );
    nodes.insert(
        "server".into(),
        node(Kind::Vm {
            os: "debian-12".into(),
            provision: vec!["provision/x.sh".into()],
        }),
    );
    Topology {
        name: "three-node".into(),
        nodes,
        networks: IndexMap::new(),
        links: vec![
            Link {
                a: ep("client", "eth0"),
                b: ep("router", "eth0"),
                tc: None,
            },
            Link {
                a: ep("router", "eth1"),
                b: ep("server", "eth0"),
                tc: None,
            },
        ],
    }
}

#[test]
fn nodes_keep_their_kind_and_access() {
    let spec = compile(&three_node()).unwrap();
    assert!(spec.machines["client"].access);
    assert_eq!(
        spec.machines["router"]
            .docker
            .as_ref()
            .unwrap()
            .image
            .as_deref(),
        Some("frrouting/frr:v8.5.2")
    );
    assert_eq!(spec.machines["server"].vm.as_ref().unwrap().os, "debian-12");
    assert!(spec.machines["server"].docker.is_none());
}

#[test]
fn the_compiled_spec_is_a_valid_isoloom_spec() {
    let spec = compile(&three_node()).unwrap();
    let yaml = to_yaml(&spec).unwrap();
    let reparsed = isoloom_spec::parse(&yaml).expect("the output parses as an Isoloom spec");
    let problems = isoloom_spec::validate(&reparsed);
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn the_yaml_leaves_defaults_out() {
    let yaml = to_yaml(&compile(&three_node()).unwrap()).unwrap();
    for noise in [
        "null",
        "[]",
        "{}",
        "privileged",
        "read_only",
        "arch",
        "http",
    ] {
        assert!(!yaml.contains(noise), "`{noise}` in:\n{yaml}");
    }
    assert!(yaml.contains("internet: false"));
    assert!(yaml.contains("access: true"));
}

#[test]
fn an_idle_container_is_kept_running() {
    let mut topo = three_node();
    topo.nodes["router"].kind = Kind::Container {
        image: "alpine:3.20".into(),
        idle: true,
        appliance: None,
        config: None,
    };
    let spec = compile(&topo).unwrap();
    assert!(spec.machines["router"].docker.as_ref().unwrap().idle);
    assert!(to_yaml(&spec).unwrap().contains("idle: true"));
}
