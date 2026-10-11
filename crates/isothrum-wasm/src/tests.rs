use std::io::Cursor;

use super::*;

#[test]
fn the_labs_own_files_are_its_starters() {
    let files = own_files(TOPOLOGY);
    assert_eq!(
        files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["provision/client.sh"]
    );
    assert!(files[0].contents.starts_with("#!"));
}

const TOPOLOGY: &str = r#"{
    "name": "pair",
    "nodes": {
        "client": { "kind": { "vm": { "os": "debian-12", "provision": ["provision/client.sh"] } }, "access": true },
        "server": { "kind": { "container": { "image": "nginx:1.27-alpine" } }, "services": [{ "port": 80, "http": true }] }
    },
    "links": [ { "a": { "node": "client", "port": "eth0" }, "b": { "node": "server", "port": "eth0" } } ]
}"#;

#[test]
fn a_topology_becomes_a_spec_and_generated_files() {
    let plan = plan(TOPOLOGY);
    assert!(plan.ok, "{:?}", plan.error);
    let yml = plan.isoloom_yml.unwrap();
    assert!(yml.contains("name: pair"));
    assert!(
        plan.files
            .iter()
            .any(|f| f.path.ends_with("docker/compose.yml")),
        "{:?}",
        plan.files.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
}

#[test]
fn machines_get_addresses_on_their_lans() {
    let t = r#"{"name":"lan","networks":{"office":{"cidr":"10.10.1.0/24"}},"nodes":{
        "a":{"kind":{"container":{"image":"alpine:3.20"}},"networks":{"office":null}},
        "b":{"kind":{"container":{"image":"alpine:3.20"}},"networks":{"office":20}}}}"#;
    let plan = plan(t);
    assert!(plan.ok, "{:?}", plan.error);
    assert_eq!(
        plan.addresses["a"],
        vec![("office".to_string(), "10.10.1.10".to_string())]
    );
    assert_eq!(
        plan.addresses["b"],
        vec![("office".to_string(), "10.10.1.20".to_string())]
    );
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
}

#[test]
fn a_bad_topology_says_why() {
    let plan = plan(
        r#"{"name":"x","nodes":{"a":{"kind":{"vm":{"os":"debian-12"}}}},"links":[{"a":{"node":"a","port":"e"},"b":{"node":"ghost","port":"e"}}]}"#,
    );
    assert!(!plan.ok);
    assert!(plan.error.unwrap().contains("ghost"));
}

#[test]
fn an_invalid_spec_is_reported_not_generated() {
    // A machine without a cable has no network: Isoloom's generators would panic on it.
    let t = r#"{"name":"lab","nodes":{
        "a":{"kind":{"container":{"image":"alpine:3.20"}}},
        "b":{"kind":{"container":{"image":"alpine:3.20"}}},
        "lonely":{"kind":{"vm":{"os":"debian-12"}}}},
        "links":[{"a":{"node":"a","port":"eth0"},"b":{"node":"b","port":"eth0"}}]}"#;
    let plan = plan(t);
    assert!(plan.ok);
    assert!(
        plan.problems.iter().any(|p| p.contains("lonely")),
        "{:?}",
        plan.problems
    );
    assert!(plan.files.is_empty());
    assert!(bundle_bytes(t).is_err());
}

#[test]
fn a_target_bundle_holds_only_that_target() {
    // A target this lab does produce: the folder of its first generated file.
    let first = plan(TOPOLOGY).files[0].path.clone();
    let target = first
        .trim_start_matches(".isoloom/")
        .split('/')
        .next()
        .unwrap()
        .to_string();
    let bytes = bundle_target_bytes(TOPOLOGY, &target).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(names.contains(&SPEC_PATH.to_string()));
    let prefix = format!(".isoloom/{target}/");
    assert!(
        names
            .iter()
            .all(|n| n == SPEC_PATH || n.starts_with(&prefix)),
        "{names:?}"
    );
    assert!(names.len() > 1);
    assert!(bundle_target_bytes(TOPOLOGY, "no-such-target").is_err());
}

#[test]
fn the_bundle_holds_the_spec_and_the_generated_files() {
    let bytes = bundle_bytes(TOPOLOGY).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert!(archive.by_name(SPEC_PATH).is_ok());
    assert!(archive.len() > 2);
}

#[test]
fn a_vm_lab_comes_with_its_provisioning_scripts() {
    let names = |bytes: Vec<u8>| {
        let mut a = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        (0..a.len())
            .map(|i| a.by_index(i).unwrap().name().to_string())
            .collect::<Vec<_>>()
    };
    let vms = r#"{"name":"vms","networks":{"lan":{"cidr":"10.10.1.0/24"}},"nodes":{
        "db":{"kind":{"vm":{"os":"debian-12","provision":["provision/db.sh"]}},"networks":{"lan":null},"services":[{"port":5432}]},
        "app":{"kind":{"vm":{"os":"debian-12","provision":["provision/app.sh"]}},"networks":{"lan":null},"access":true}}}"#;
    let all = names(bundle_bytes(vms).unwrap());
    assert!(all.contains(&"provision/db.sh".to_string()), "{all:?}");
    assert!(
        names(bundle_target_bytes(vms, "vagrant").unwrap())
            .contains(&"provision/db.sh".to_string())
    );
    // The mixed lab's Docker download runs no provisioning: no script in it.
    assert!(
        !names(bundle_target_bytes(TOPOLOGY, "docker").unwrap())
            .contains(&"provision/client.sh".to_string())
    );
}

#[test]
fn vlans_nest_under_their_switch_and_keep_the_editor_names() {
    let t = r#"{"name":"vlans","networks":{
        "vlan10":{"cidr":"10.10.10.0/24","switch":"sw1","vlan":10,"internet":false},
        "vlan20":{"cidr":"10.10.20.0/24","switch":"sw1","vlan":20,"internet":false}},
        "nodes":{
        "pc1":{"kind":{"container":{"image":"alpine:3.20","idle":true}},"networks":{"vlan10":null}},
        "srv":{"kind":{"container":{"image":"nginx:1.27-alpine"}},"networks":{"vlan20":null}},
        "r":{"kind":{"router":{}},"networks":{"vlan10":null,"vlan20":null}}}}"#;
    let plan = plan(t);
    assert!(plan.ok, "{:?}", plan.error);
    assert!(plan.problems.is_empty(), "{:?}", plan.problems);
    let yml = plan.isoloom_yml.unwrap();
    assert!(
        yml.contains("  sw1:\n    cidr: 10.10.0.0/19\n    internet: false\n    vlans:\n"),
        "{yml}"
    );
    assert!(
        yml.contains("sw1.vlan10: 10") && yml.contains("from: sw1.vlan10"),
        "{yml}"
    );
    assert_eq!(
        plan.addresses["pc1"],
        vec![("vlan10".to_string(), "10.10.10.10".to_string())]
    );
    assert!(
        plan.files
            .iter()
            .any(|f| f.path.ends_with("docker/compose.yml") && f.contents.contains("sw1-vlan10"))
    );
    // The download parses it back the same way.
    assert!(bundle_bytes(t).is_ok());
}

#[test]
fn a_cable_with_conditions_carries_them() {
    let t = r#"{"name":"wan","nodes":{
        "a":{"kind":{"container":{"image":"alpine:3.20","idle":true}}},
        "b":{"kind":{"container":{"image":"alpine:3.20","idle":true}}}},
        "links":[{"a":{"node":"a","port":"eth0"},"b":{"node":"b","port":"eth0"},"tc":{"delay":"40ms","loss":0.5}}]}"#;
    let plan = plan(t);
    assert!(
        plan.ok && plan.problems.is_empty(),
        "{:?} {:?}",
        plan.error,
        plan.problems
    );
    let yml = plan.isoloom_yml.unwrap();
    assert!(
        yml.contains("tc:\n      delay: 40ms\n      loss: 0.5"),
        "{yml}"
    );
    assert!(
        plan.files
            .iter()
            .any(|f| f.path.ends_with("docker/compose.yml")
                && f.contents.contains("netem delay 40ms loss 0.5%"))
    );
}

#[test]
fn a_cisco_iol_router_is_its_lans_gateway_with_its_config() {
    let t = r#"{"name":"iol","networks":{"a":{"cidr":"10.30.1.0/24"},"b":{"cidr":"10.30.2.0/24"}},"nodes":{
        "r1":{"kind":{"container":{"image":"vrnetlab/cisco_iol:17.12.01","appliance":"cisco-iol","config":"router ospf 1\n network 10.30.0.0 0.0.255.255 area 0"}},"networks":{"a":null}},
        "r2":{"kind":{"container":{"image":"vrnetlab/cisco_iol:17.12.01","appliance":"cisco-iol"}},"networks":{"b":null}},
        "pc":{"kind":{"container":{"image":"alpine:3.20","idle":true}},"networks":{"a":null}}},
        "links":[{"a":{"node":"r1","port":"eth1"},"b":{"node":"r2","port":"eth1"}}]}"#;
    let plan = plan(t);
    assert!(
        plan.ok && plan.problems.is_empty(),
        "{:?} {:?}",
        plan.error,
        plan.problems
    );
    let yml = plan.isoloom_yml.unwrap();
    assert!(
        yml.contains("gateway: r1") && yml.contains("gateway: r2"),
        "{yml}"
    );
    assert!(
        yml.contains("appliance: cisco-iol") && yml.contains("config: configs/r1.cfg"),
        "{yml}"
    );
    assert_eq!(
        plan.addresses["r1"][0],
        ("a".to_string(), "10.30.1.1".to_string())
    );
    let cfg = plan
        .files
        .iter()
        .find(|f| f.path.ends_with("appliances/r1/base.txt"))
        .unwrap();
    assert!(
        cfg.contents
            .contains("interface Ethernet0/1\n description a\n ip address 10.30.1.1 255.255.255.0"),
        "{}",
        cfg.contents
    );
    let mut zip =
        zip::ZipArchive::new(Cursor::new(bundle_target_bytes(t, "docker").unwrap())).unwrap();
    let mut own = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("configs/r1.cfg").unwrap(), &mut own).unwrap();
    assert!(own.starts_with("router ospf 1"));
}
