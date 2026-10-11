//! LANs: addresses, mistakes, automatic routers.

use super::*;

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
