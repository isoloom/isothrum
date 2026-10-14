//! What Isoloom v1 lets a machine and a LAN carry: IPv6, environment, files, a command.

use super::*;

/// Two LANs, the router taking its addresses like the others (Isoloom v1 keeps a block's last
/// two addresses for its controller and router).
fn lab() -> Topology {
    let mut topo = two_lans();
    topo.nodes["router"].networks.insert("office".into(), None);
    topo
}

#[test]
fn a_lan_with_an_ipv6_block_is_dual_stack() {
    let mut topo = lab();
    // All containers, so the lab runs on Docker.
    for node in topo.nodes.values_mut() {
        if !matches!(node.kind, Kind::Router {}) {
            node.kind = Kind::Container {
                image: "alpine:3.20".into(),
                idle: true,
                appliance: None,
                config: None,
            };
        }
    }
    topo.networks["office"].ipv6 = Some("fd00:1::/64".into());
    let spec = compile(&topo).unwrap();
    let v6 = spec.networks["office"].ipv6.as_ref().unwrap();
    assert_eq!(v6.cidr, "fd00:1::/64");
    assert!(!v6.only);
    assert!(spec.networks["servers"].ipv6.is_none());
    let problems = isoloom_spec::validate(&spec);
    assert!(problems.is_empty(), "{problems:?}");
    // It reaches Docker Compose as an IPv6 subnet beside the IPv4 one.
    let files = isoloom_targets::generate(&spec, isoloom_spec::Target::Docker).unwrap();
    let compose = files
        .iter()
        .find(|f| f.path.ends_with("compose.yml"))
        .unwrap();
    assert!(
        compose.contents.contains("fd00:1::/64"),
        "{}",
        compose.contents
    );
}

#[test]
fn a_blank_ipv6_block_leaves_the_lan_ipv4() {
    let mut topo = two_lans();
    topo.networks["office"].ipv6 = Some("  ".into());
    assert!(compile(&topo).unwrap().networks["office"].ipv6.is_none());
}

#[test]
fn a_machine_carries_its_environment_files_and_command() {
    let mut topo = lab();
    let server = &mut topo.nodes["server"];
    server.environment.insert("APP_MODE".into(), "lab".into());
    server.files.push(NodeFile {
        path: " /etc/app/app.ini ".into(),
        content: "[app]\nmode = lab\n".into(),
    });
    server.command = Some("  sleep infinity  ".into());
    let spec = compile(&topo).unwrap();
    let m = &spec.machines["server"];
    assert_eq!(m.environment["APP_MODE"], "lab");
    assert_eq!(m.files[0].path, "/etc/app/app.ini");
    assert_eq!(m.files[0].content.as_deref(), Some("[app]\nmode = lab\n"));
    assert_eq!(
        m.command,
        Some(isoloom_spec::model::CommandLine::Shell(
            "sleep infinity".into()
        ))
    );
    let problems = isoloom_spec::validate(&spec);
    assert!(problems.is_empty(), "{problems:?}");
    // A blank command is no command.
    topo.nodes["server"].command = Some(" ".into());
    assert!(compile(&topo).unwrap().machines["server"].command.is_none());
}
