//! Starter provisioning scripts, so a downloaded lab with VMs runs as is.
//!
//! The editor lists a VM's provisioning steps (`provision/db.sh`), but the files aren't written
//! by anyone. Each `.sh` step gets a script that installs what the machine's service ports call
//! for and makes it listen on the network (PostgreSQL, MariaDB and Redis listen on localhost by
//! default, so the lab's checks couldn't reach them); a `.ps1` step gets a commented template; an
//! Ansible `.yml` step a minimal playbook. They are starting points to edit, not a configuration
//! management system.

use isoloom_core::{Machine, Spec};

/// A file to put in the download: its path in the project, and its contents.
pub struct Starter {
    pub path: String,
    pub contents: String,
}

/// What a service port installs on Debian-family (`apt`) and Red Hat-family (`dnf`) systems, and
/// the shell lines that make it answer on the network.
struct Package {
    port: u16,
    apt: &'static str,
    dnf: &'static str,
    /// The systemd unit to enable and restart after configuring.
    unit_apt: &'static str,
    unit_dnf: &'static str,
    /// Lines run after installing (both families), to listen beyond localhost.
    configure: &'static str,
}

const PACKAGES: &[Package] = &[
    Package {
        port: 22,
        apt: "openssh-server",
        dnf: "openssh-server",
        unit_apt: "ssh",
        unit_dnf: "sshd",
        configure: "",
    },
    Package {
        port: 80,
        apt: "nginx",
        dnf: "nginx",
        unit_apt: "nginx",
        unit_dnf: "nginx",
        configure: "",
    },
    Package {
        port: 443,
        apt: "nginx",
        dnf: "nginx",
        unit_apt: "nginx",
        unit_dnf: "nginx",
        configure: "",
    },
    Package {
        port: 5432,
        apt: "postgresql",
        dnf: "postgresql-server",
        unit_apt: "postgresql",
        unit_dnf: "postgresql",
        configure: r#"# Red Hat family: create the database cluster first (Debian's package does it itself).
if command -v postgresql-setup >/dev/null && [ ! -f /var/lib/pgsql/data/PG_VERSION ]; then
  postgresql-setup --initdb || runuser -u postgres -- initdb -D /var/lib/pgsql/data
fi
conf=$(find /etc/postgresql /var/lib/pgsql -name postgresql.conf 2>/dev/null | head -1 || true)
hba=$(find /etc/postgresql /var/lib/pgsql -name pg_hba.conf 2>/dev/null | head -1 || true)
if [ -n "$conf" ]; then sed -i "s/^#\?listen_addresses.*/listen_addresses = '*'/" "$conf"; fi
if [ -n "$hba" ] && ! grep -q "0.0.0.0/0" "$hba"; then echo "host all all 0.0.0.0/0 scram-sha-256" >> "$hba"; fi"#,
    },
    Package {
        port: 3306,
        apt: "mariadb-server",
        dnf: "mariadb-server",
        unit_apt: "mariadb",
        unit_dnf: "mariadb",
        configure: r#"for f in /etc/mysql/mariadb.conf.d/50-server.cnf /etc/my.cnf.d/mariadb-server.cnf; do
  if [ -f "$f" ]; then sed -i "s/^bind-address.*/bind-address = 0.0.0.0/" "$f"; fi
done"#,
    },
    Package {
        port: 6379,
        apt: "redis-server",
        dnf: "redis",
        unit_apt: "redis-server",
        unit_dnf: "redis",
        configure: r#"for f in /etc/redis/redis.conf /etc/redis.conf; do
  if [ -f "$f" ]; then sed -i -e "s/^bind .*/bind 0.0.0.0/" -e "s/^protected-mode yes/protected-mode no/" "$f"; fi
done"#,
    },
    Package {
        port: 53,
        apt: "bind9",
        dnf: "bind",
        unit_apt: "named",
        unit_dnf: "named",
        configure: "",
    },
    Package {
        port: 21,
        apt: "vsftpd",
        dnf: "vsftpd",
        unit_apt: "vsftpd",
        unit_dnf: "vsftpd",
        configure: "",
    },
    Package {
        port: 25,
        apt: "postfix",
        dnf: "postfix",
        unit_apt: "postfix",
        unit_dnf: "postfix",
        configure: "",
    },
];

/// Whether an OS name from Isoloom's list uses `dnf` (otherwise `apt`).
fn uses_dnf(os: &str) -> bool {
    ["rocky", "almalinux", "centos", "fedora"]
        .iter()
        .any(|p| os.starts_with(p))
}

fn addresses(spec: &Spec, m: &Machine) -> Vec<String> {
    m.networks
        .iter()
        .filter_map(|(net, octet)| {
            let cidr = &spec.networks.get(net)?.cidr;
            let base = cidr.split('/').next()?.rsplit_once('.')?.0;
            Some(format!("{base}.{octet} ({net})"))
        })
        .collect()
}

impl Package {
    fn name(&self, dnf: bool) -> &'static str {
        if dnf { self.dnf } else { self.apt }
    }

    fn unit(&self, dnf: bool) -> &'static str {
        if dnf { self.unit_dnf } else { self.unit_apt }
    }
}

fn shell(spec: &Spec, name: &str, m: &Machine, os: &str) -> String {
    let dnf = uses_dnf(os);
    let ports: Vec<u16> = m.services.iter().map(|s| s.port).collect();
    let wanted = wanted(&ports, dnf);
    let mut s = shell_header(spec, name, m, os, &ports);
    if wanted.is_empty() {
        s.push_str("# Nothing to install for this machine's ports yet: add your own steps here.\n");
    } else {
        s.push_str(&install(&wanted, dnf));
    }
    let unknown: Vec<String> = ports
        .iter()
        .filter(|port| !PACKAGES.iter().any(|p| p.port == **port))
        .map(|p| p.to_string())
        .collect();
    if !unknown.is_empty() {
        s.push_str(&format!(
            "\n# TODO: install what answers on port(s) {}.\n",
            unknown.join(", ")
        ));
    }
    s
}

/// The packages answering on `ports`, each package once.
fn wanted(ports: &[u16], dnf: bool) -> Vec<&'static Package> {
    let mut wanted: Vec<&Package> = Vec::new();
    for p in PACKAGES.iter().filter(|p| ports.contains(&p.port)) {
        if !wanted.iter().any(|w| w.name(dnf) == p.name(dnf)) {
            wanted.push(p);
        }
    }
    wanted
}

/// The script's first lines: what it is, the machine's addresses and expected services.
fn shell_header(spec: &Spec, name: &str, m: &Machine, os: &str, ports: &[u16]) -> String {
    let mut s = String::from("#!/usr/bin/env bash\n");
    s.push_str(&format!("# Starter provisioning for `{name}` ({os}), written by Isothrum: edit it to build your lab.\n"));
    for a in addresses(spec, m) {
        s.push_str(&format!("# Address: {a}\n"));
    }
    if !ports.is_empty() {
        let list: Vec<String> = ports.iter().map(|p| p.to_string()).collect();
        s.push_str(&format!(
            "# Services the lab expects to answer: {}\n",
            list.join(", ")
        ));
    }
    s.push_str("set -euo pipefail\n\n");
    s
}

/// Installs, configures, enables and restarts the packages.
fn install(wanted: &[&Package], dnf: bool) -> String {
    let pkgs: Vec<&str> = wanted.iter().map(|p| p.name(dnf)).collect();
    let mut s = if dnf {
        format!("dnf install -y {}\n", pkgs.join(" "))
    } else {
        format!(
            "export DEBIAN_FRONTEND=noninteractive\napt-get update\napt-get install -y {}\n",
            pkgs.join(" ")
        )
    };
    for p in wanted.iter().filter(|p| !p.configure.is_empty()) {
        s.push_str(&format!(
            "\n# Port {}: listen on the lab's networks, not only on localhost.\n{}\n",
            p.port, p.configure
        ));
    }
    s.push('\n');
    for p in wanted {
        let unit = p.unit(dnf);
        s.push_str(&format!(
            "systemctl enable {unit}\nsystemctl restart {unit}\n"
        ));
    }
    s
}

fn powershell(spec: &Spec, name: &str, m: &Machine, os: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!("# Starter provisioning for `{name}` ({os}), written by Isothrum: edit it to build your lab.\n"));
    for a in addresses(spec, m) {
        s.push_str(&format!("# Address: {a}\n"));
    }
    s.push_str("$ErrorActionPreference = 'Stop'\n\n");
    let ports: Vec<u16> = m.services.iter().map(|s| s.port).collect();
    if ports.contains(&80) || ports.contains(&443) {
        s.push_str("# A web server (IIS) for port 80.\nInstall-WindowsFeature -Name Web-Server -IncludeManagementTools\n");
    }
    for p in ports.iter().filter(|p| **p != 80 && **p != 443) {
        s.push_str(&format!(
            "# TODO: install what answers on port {p}, then open it:\n"
        ));
        s.push_str(&format!(
            "New-NetFirewallRule -DisplayName 'lab-{p}' -Direction Inbound -Protocol TCP -LocalPort {p} -Action Allow\n"
        ));
    }
    if ports.is_empty() {
        s.push_str("# Nothing to install for this machine's ports yet: add your own steps here.\n");
    }
    s
}

fn playbook(name: &str) -> String {
    format!(
        "# Starter playbook for `{name}`, written by Isothrum: edit it to build your lab.\n- hosts: all\n  become: true\n  tasks:\n    - name: The machine answers\n      ansible.builtin.ping:\n"
    )
}

/// One starter per VM provisioning step (relative paths in the project only).
pub fn starters(spec: &Spec) -> Vec<Starter> {
    let mut out: Vec<Starter> = Vec::new();
    for (name, m) in &spec.machines {
        let Some(vm) = &m.vm else { continue };
        for step in &vm.provision {
            if step.starts_with('/') || step.contains("..") || out.iter().any(|s| &s.path == step) {
                continue;
            }
            let contents = if step.ends_with(".ps1") {
                powershell(spec, name, m, &vm.os)
            } else if step.ends_with(".yml") || step.ends_with(".yaml") {
                playbook(name)
            } else {
                shell(spec, name, m, &vm.os)
            };
            out.push(Starter {
                path: step.clone(),
                contents,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(yaml: &str) -> Spec {
        isoloom_core::parse(yaml).unwrap()
    }

    #[test]
    fn a_database_vm_gets_postgresql_listening_on_the_lab() {
        let s = spec(
            "version: 1\nname: x\nnetworks:\n  lan: { cidr: 10.10.1.0/24 }\nmachines:\n  db:\n    networks: { lan: 12 }\n    services: [{ port: 5432 }]\n    vm: { os: debian-12, provision: [provision/db.sh] }\n",
        );
        let files = starters(&s);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "provision/db.sh");
        let sh = &files[0].contents;
        assert!(sh.starts_with("#!/usr/bin/env bash\n"));
        assert!(sh.contains("# Address: 10.10.1.12 (lan)"));
        assert!(sh.contains("apt-get install -y postgresql"));
        assert!(sh.contains("listen_addresses = '*'"));
        assert!(sh.contains("systemctl enable postgresql"));
    }

    #[test]
    fn red_hat_family_uses_dnf_and_unknown_ports_are_left_to_do() {
        let s = spec(
            "version: 1\nname: x\nnetworks:\n  lan: { cidr: 10.10.1.0/24 }\nmachines:\n  web:\n    networks: { lan: 10 }\n    services: [{ port: 80 }, { port: 9000 }]\n    vm: { os: rocky-9, provision: [provision/web.sh] }\n",
        );
        let sh = &starters(&s)[0].contents;
        assert!(sh.contains("dnf install -y nginx"));
        assert!(sh.contains("TODO: install what answers on port(s) 9000"));
    }

    #[test]
    fn windows_and_ansible_get_their_own_templates() {
        let s = spec(
            "version: 1\nname: x\nnetworks:\n  lan: { cidr: 10.10.1.0/24 }\nmachines:\n  win:\n    networks: { lan: 10 }\n    services: [{ port: 80 }]\n    vm: { os: windows-server-2022, provision: [provision/win.ps1, provision/win.yml] }\n",
        );
        let files = starters(&s);
        assert!(
            files[0]
                .contents
                .contains("Install-WindowsFeature -Name Web-Server")
        );
        assert!(files[1].contents.contains("ansible.builtin.ping"));
    }
}
