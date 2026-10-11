use super::*;

fn node(kind: Kind) -> Node {
    Node {
        kind,
        access: false,
        services: vec![],
        networks: IndexMap::new(),
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
    let reparsed = isoloom_core::parse(&yaml).expect("the output parses as an Isoloom spec");
    let problems = isoloom_core::validate(&reparsed);
    assert!(problems.is_empty(), "{problems:?}");
}

/// A switch running a Cisco OS, as the editor sends it: a node of its own with no networks,
/// named as the `device` of its VLANs; machines on its ports join the VLANs.
#[test]
fn a_cisco_switch_switches_its_vlans() {
    let topo: Topology = serde_json::from_str(
        r#"{
          "name": "cisco-sw",
          "nodes": {
            "pc1": { "kind": { "container": { "image": "alpine:3.20", "idle": true } }, "access": false, "services": [], "networks": { "vlan10": null } },
            "pc2": { "kind": { "container": { "image": "alpine:3.20", "idle": true } }, "access": false, "services": [], "networks": { "vlan20": null } },
            "sw1": { "kind": { "container": { "image": "vrnetlab/cisco_iol:L2-17.12.01", "appliance": "cisco-iol-l2", "config": "spanning-tree mode rapid-pvst" } }, "access": false, "services": [], "networks": {} }
          },
          "networks": {
            "vlan10": { "cidr": "10.10.10.0/24", "internet": true, "switch": "sw1", "vlan": 10, "device": "sw1" },
            "vlan20": { "cidr": "10.10.20.0/24", "internet": true, "switch": "sw1", "vlan": 20, "device": "sw1" }
          },
          "links": []
        }"#,
    )
    .unwrap();
    let spec = compile(&topo).unwrap();
    assert_eq!(spec.networks["sw1"].switch.as_deref(), Some("sw1"));
    assert!(spec.machines["sw1"].networks.is_empty());
    assert_eq!(
        spec.machines["pc1"].networks.keys().collect::<Vec<_>>(),
        ["sw1.vlan10"]
    );
    let reparsed = isoloom_core::parse(&to_yaml(&spec).unwrap()).unwrap();
    let problems = isoloom_core::validate(&reparsed);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(
        config_files(&topo),
        [(
            config_path("sw1"),
            "spanning-tree mode rapid-pvst\n".to_string()
        )]
    );
}

/// A Cisco router on a switch's VLAN: Isoloom's VLANs have no gateway, so the router joins it
/// at an ordinary address, and the lab still builds.
#[test]
fn a_cisco_router_on_a_vlan_is_a_member_of_it() {
    let topo: Topology = serde_json::from_str(
        r#"{
          "name": "router-on-vlan",
          "nodes": {
            "pc": { "kind": { "container": { "image": "alpine:3.20", "idle": true } }, "networks": { "vlan10": null } },
            "r": { "kind": { "container": { "image": "vrnetlab/cisco_iol:17.12.01", "appliance": "cisco-iol" } }, "networks": { "vlan10": null, "office": null } }
          },
          "networks": {
            "office": { "cidr": "10.10.1.0/24" },
            "vlan10": { "cidr": "10.10.10.0/24", "switch": "sw1", "vlan": 10 }
          }
        }"#,
    )
    .unwrap();
    let spec = compile(&topo).unwrap();
    assert_eq!(spec.machines["r"].networks["sw1.vlan10"], 11);
    // On a plain LAN it is still the gateway.
    assert_eq!(spec.networks["office"].gateway.as_deref(), Some("r"));
    assert_eq!(spec.machines["r"].networks["office"], 1);
    let reparsed = isoloom_core::parse(&to_yaml(&spec).unwrap()).unwrap();
    let problems = isoloom_core::validate(&reparsed);
    assert!(problems.is_empty(), "{problems:?}");
}

/// A switch's VLANs nest under a LAN named after the switch: a LAN of the lab can't have
/// that name too, in either order.
#[test]
fn a_switch_can_not_share_a_lans_name() {
    let mut topo = two_lans();
    let vlan = Lan {
        switch: Some("office".into()),
        vlan: Some(10),
        ..lan("10.10.10.0/24")
    };
    topo.networks.insert("vlan10".into(), vlan.clone());
    topo.nodes["server"].networks.insert("vlan10".into(), None);
    let clash = Err(CompileError::SwitchNameTaken {
        switch: "office".into(),
    });
    assert_eq!(compile(&topo), clash);
    topo.networks.shift_remove("vlan10");
    topo.networks.shift_insert(0, "vlan10".into(), vlan);
    assert_eq!(compile(&topo), clash);
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
fn machines_on_a_lan_take_its_free_addresses() {
    let mut topo = two_lans();
    topo.nodes["router"]
        .networks
        .insert("office".into(), Some(10));
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

    topo.nodes.insert(
        "extra".into(),
        node(Kind::Container {
            image: "alpine:3.20".into(),
            idle: true,
            appliance: None,
            config: None,
        }),
    );
    topo.nodes["extra"].networks.insert("office".into(), None);
    assert!(matches!(compile(&topo), Err(CompileError::LanFull { .. })));
}

#[test]
fn lan_mistakes_say_what_to_fix() {
    let mut topo = two_lans();
    topo.nodes["client"].networks.insert("dmz".into(), None);
    assert_eq!(
        compile(&topo),
        Err(CompileError::UnknownLan {
            node: "client".into(),
            lan: "dmz".into()
        })
    );

    let mut topo = two_lans();
    topo.nodes["client"]
        .networks
        .insert("office".into(), Some(254));
    assert!(matches!(
        compile(&topo),
        Err(CompileError::AddressTaken { octet: 254, .. })
    ));

    let mut topo = two_lans();
    topo.networks.insert("link-0".into(), lan("10.10.9.0/24"));
    assert_eq!(
        compile(&topo),
        Err(CompileError::ReservedLanName("link-0".into()))
    );

    let mut topo = two_lans();
    topo.networks["office"].cidr = "office".into();
    assert!(matches!(compile(&topo), Err(CompileError::BadCidr { .. })));
}

#[test]
fn lans_and_cables_mix() {
    let mut topo = two_lans();
    topo.nodes["router"].networks.insert("office".into(), None);
    topo.links.push(Link {
        a: ep("client", "eth1"),
        b: ep("server", "eth1"),
        tc: None,
    });
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

#[test]
fn an_automatic_router_lets_its_networks_reach_each_other() {
    let mut topo = two_lans();
    topo.nodes["router"].kind = Kind::Router {};
    topo.nodes["router"].networks.insert("office".into(), None);
    let spec = compile(&topo).unwrap();
    // Not a machine, and no address taken on its LANs.
    assert!(!spec.machines.contains_key("router"));
    assert_eq!(spec.machines["client"].networks["office"], 10);
    let pairs: Vec<(&str, &str)> = spec
        .reach
        .iter()
        .map(|r| (r.from.as_str(), r.to.as_str()))
        .collect();
    assert_eq!(pairs, vec![("office", "servers"), ("servers", "office")]);
    let problems = isoloom_core::validate(&spec);
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn an_automatic_router_routes_its_cables_too() {
    let mut topo = two_lans();
    topo.nodes["router"].kind = Kind::Router {};
    topo.nodes["router"].networks.shift_remove("office");
    topo.links.push(Link {
        a: ep("router", "eth1"),
        b: ep("client", "eth1"),
        tc: None,
    });
    let spec = compile(&topo).unwrap();
    assert_eq!(spec.networks["link-0"].cidr, "10.200.0.0/29");
    assert_eq!(spec.machines["client"].networks["link-0"], 3);
    assert!(
        spec.reach
            .iter()
            .any(|r| r.from == "link-0" && r.to == "servers")
    );
    let problems = isoloom_core::validate(&spec);
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn a_port_carries_one_cable() {
    let mut topo = three_node();
    topo.links.push(Link {
        a: ep("client", "eth0"),
        b: ep("server", "eth1"),
        tc: None,
    });
    assert_eq!(
        compile(&topo),
        Err(CompileError::PortInUse {
            node: "client".into(),
            port: "eth0".into()
        })
    );
}

#[test]
fn a_cable_needs_two_known_nodes() {
    let mut topo = three_node();
    topo.links.push(Link {
        a: ep("client", "eth1"),
        b: ep("ghost", "eth0"),
        tc: None,
    });
    assert_eq!(
        compile(&topo),
        Err(CompileError::UnknownNode {
            link: 2,
            node: "ghost".into()
        })
    );
}

#[test]
fn a_node_can_not_be_cabled_to_itself() {
    let mut topo = three_node();
    topo.links = vec![Link {
        a: ep("router", "eth0"),
        b: ep("router", "eth1"),
        tc: None,
    }];
    assert_eq!(
        compile(&topo),
        Err(CompileError::SelfLink {
            link: 0,
            node: "router".into()
        })
    );
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
            tc: None,
        });
    }
    let spec = compile(&topo).unwrap();
    assert_eq!(spec.networks["link-99"].cidr, "10.200.3.24/29");
    let problems = isoloom_core::validate(&spec);
    assert!(problems.is_empty(), "{problems:?}");
}
