//! Cables: point-to-point networks and their mistakes.

use super::*;

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
    let problems = isoloom_spec::validate(&spec);
    assert!(problems.is_empty(), "{problems:?}");
}
