//! Switches: VLANs, Cisco switches and routers on them.

use super::*;

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
    let reparsed = isoloom_spec::parse(&to_yaml(&spec).unwrap()).unwrap();
    let problems = isoloom_spec::validate(&reparsed);
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
    let reparsed = isoloom_spec::parse(&to_yaml(&spec).unwrap()).unwrap();
    let problems = isoloom_spec::validate(&reparsed);
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
