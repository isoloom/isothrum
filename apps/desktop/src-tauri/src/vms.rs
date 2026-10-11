//! Labs with VMs: run through Isoloom's Vagrant output (only VMs) or its hybrid output (VMs, and
//! the containers in a Docker host VM), with `vagrant` instead of `docker compose`. Machines are
//! Vagrant's (`config.vm.define "<name>"`): a console is `vagrant ssh`, starting and stopping
//! one is `vagrant up` and `halt`, a snapshot is `vagrant snapshot`, a capture is `tcpdump` over
//! `vagrant ssh`. A hybrid lab's containers are reached through their host VM.

use std::path::{Path, PathBuf};
use std::process::Command;

use isoloom_core::Spec;

use crate::runner::{RunError, Runtime, run};

/// The hybrid output's Docker host VM, and where it keeps the containers' Compose file.
pub const DOCKER_HOST: &str = "isoloom-docker";
const HOST_COMPOSE: &str = "/opt/isoloom/.isoloom/hybrid/compose.yml";

/// The folder Isoloom wrote the Vagrantfile into.
pub fn dir(lab: &Path, runtime: Runtime) -> PathBuf {
    lab.join(".isoloom").join(match runtime {
        Runtime::Hybrid => "hybrid",
        _ => "vagrant",
    })
}

/// `vagrant` in the lab's Vagrant folder.
pub fn vagrant(lab: &Path, runtime: Runtime, args: &[&str]) -> Command {
    let mut c = Command::new("vagrant");
    c.current_dir(dir(lab, runtime)).args(args);
    c
}

/// Whether `machine` is one of a hybrid lab's containers (those run in the Docker host VM).
pub fn in_host(spec: &Spec, runtime: Runtime, machine: &str) -> bool {
    runtime == Runtime::Hybrid
        && spec
            .machines
            .get(machine)
            .is_some_and(|m| m.docker.is_some())
}

/// A shell command run as root in a VM, base64-encoded so that its quotes survive `ssh -c`.
pub fn ssh_root(lab: &Path, runtime: Runtime, vm: &str, script: &str) -> Command {
    vagrant(
        lab,
        runtime,
        &[
            "ssh",
            vm,
            "-c",
            &format!("echo {} | base64 -d | sudo sh", base64(script.as_bytes())),
        ],
    )
}

pub fn up(lab: &Path, runtime: Runtime) -> Result<String, RunError> {
    run(vagrant(lab, runtime, &["up"]), "Starting the lab's VMs")
}

pub fn down(lab: &Path, runtime: Runtime) -> Result<String, RunError> {
    run(
        vagrant(lab, runtime, &["destroy", "-f"]),
        "Removing the lab's VMs",
    )
}

/// The machines' states, in the shape the editor reads (`docker compose ps` JSON lines):
/// `{"Service": <machine>, "State": "running" | "exited"}`. A hybrid lab adds its containers'.
pub fn status(lab: &Path, runtime: Runtime) -> Result<String, RunError> {
    let out = run(
        vagrant(lab, runtime, &["status", "--machine-readable"]),
        "Reading the lab's state",
    )?;
    let mut lines = states(&out);
    if runtime == Runtime::Hybrid
        && lines.contains(&format!(
            "\"Service\":\"{DOCKER_HOST}\",\"State\":\"running\""
        ))
    {
        let script = format!("docker compose -f {HOST_COMPOSE} ps --all --format json");
        if let Ok(containers) = run(
            ssh_root(lab, runtime, DOCKER_HOST, &script),
            "Reading the containers' state",
        ) {
            lines.push('\n');
            lines.push_str(containers.trim());
        }
    }
    Ok(lines)
}

/// `vagrant status --machine-readable` (`time,machine,state,<state>` lines) as JSON lines.
pub fn states(machine_readable: &str) -> String {
    machine_readable
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            (f.len() >= 4 && f[2] == "state" && !f[1].is_empty()).then(|| {
                let state = if f[3] == "running" {
                    "running"
                } else {
                    "exited"
                };
                serde_json::json!({ "Service": f[1], "State": state }).to_string()
            })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A console: `vagrant ssh` to a VM, or a shell in a hybrid lab's container through its host.
pub fn console_command(lab: &Path, spec: &Spec, runtime: Runtime, machine: &str) -> String {
    let d = dir(lab, runtime)
        .display()
        .to_string()
        .replace('\'', "'\\''");
    if in_host(spec, runtime, machine) {
        format!(
            "cd '{d}' && vagrant ssh {DOCKER_HOST} -c \"sudo docker compose -f {HOST_COMPOSE} exec {machine} sh -c 'command -v bash >/dev/null && exec bash || exec sh'\""
        )
    } else {
        format!("cd '{d}' && vagrant ssh {machine}")
    }
}

/// Starts, stops or restarts one machine.
pub fn action(
    lab: &Path,
    spec: &Spec,
    runtime: Runtime,
    machine: &str,
    action: &str,
) -> Result<String, RunError> {
    if in_host(spec, runtime, machine) {
        let verb = match action {
            "start" => "start",
            "stop" => "stop",
            "restart" => "restart",
            other => {
                return Err(RunError::Spec(format!(
                    "unknown action `{other}`: start, stop or restart"
                )));
            }
        };
        let script = format!("docker compose -f {HOST_COMPOSE} {verb} {machine}");
        return run(
            ssh_root(lab, runtime, DOCKER_HOST, &script),
            "Changing the machine's state",
        );
    }
    let (args, what): (&[&str], &str) = match action {
        "start" => (&["up", machine], "Starting the machine"),
        "stop" => (&["halt", machine], "Stopping the machine"),
        "restart" => (&["reload", machine], "Restarting the machine"),
        other => {
            return Err(RunError::Spec(format!(
                "unknown action `{other}`: start, stop or restart"
            )));
        }
    };
    run(vagrant(lab, runtime, args), what)
}

// Snapshots of a VM: Vagrant's (the provider's own: VirtualBox, libvirt...), named by the id.

pub fn snapshot_save(
    lab: &Path,
    runtime: Runtime,
    machine: &str,
    id: &str,
) -> Result<String, RunError> {
    run(
        vagrant(lab, runtime, &["snapshot", "save", machine, id]),
        "Saving the machine",
    )
}

pub fn snapshot_restore(
    lab: &Path,
    runtime: Runtime,
    machine: &str,
    id: &str,
) -> Result<String, RunError> {
    run(
        vagrant(
            lab,
            runtime,
            &["snapshot", "restore", machine, id, "--no-provision"],
        ),
        "Restoring the machine",
    )
}

pub fn snapshot_delete(
    lab: &Path,
    runtime: Runtime,
    machine: &str,
    id: &str,
) -> Result<String, RunError> {
    run(
        vagrant(lab, runtime, &["snapshot", "delete", machine, id]),
        "Removing the snapshot",
    )
}

/// A capture from `machine`: the container capture's script (find the interface on the network,
/// pcap on stdout), in the VM with tcpdump installed first if missing, or for a hybrid lab's
/// container, in the capture image on its host joined to the container's namespace.
pub fn capture_command(
    lab: &Path,
    runtime: Runtime,
    machine: &str,
    in_host: bool,
    script: &str,
) -> Command {
    if in_host {
        let inner = script.replace('\'', "'\\''");
        let host = format!(
            "docker run --rm -i --net container:$(docker compose -f {HOST_COMPOSE} ps -q {machine}) --cap-add NET_RAW --cap-add NET_ADMIN {} sh -c '{inner}'",
            crate::runner::CAPTURE_IMAGE
        );
        return ssh_root(lab, runtime, DOCKER_HOST, &host);
    }
    let full = format!(
        "command -v tcpdump >/dev/null || {{ (apt-get install -y -qq tcpdump || dnf install -y -q tcpdump) >/dev/null 2>&1; }}; {script}"
    );
    ssh_root(lab, runtime, machine, &full)
}

/// Standard, padded base64 (one small use: no dependency for it).
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let v = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            out.push(if i <= chunk.len() {
                A[((v >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vagrant_states_become_the_editors_json_lines() {
        let out = "1700000000,,metadata,provider,virtualbox\n1700000000,web,state,running\n1700000000,db,state,poweroff\n1700000000,web,state-human-short,running\n";
        assert_eq!(
            states(out),
            "{\"Service\":\"web\",\"State\":\"running\"}\n{\"Service\":\"db\",\"State\":\"exited\"}"
        );
    }

    /// GNU `base64` (`-w0`): Linux only.
    #[test]
    #[cfg(target_os = "linux")]
    fn base64_matches_the_systems() {
        for s in ["", "a", "ab", "abc", "echo 'a b' && awk '{print $2}'\n"] {
            let sys = Command::new("sh")
                .args(["-c", "printf %s \"$0\" | base64 -w0", s])
                .output()
                .unwrap()
                .stdout;
            assert_eq!(
                base64(s.as_bytes()),
                String::from_utf8(sys).unwrap(),
                "{s:?}"
            );
        }
    }

    #[test]
    fn a_hybrid_labs_containers_go_through_their_host() {
        let spec = isoloom_core::parse(
            "version: 1\nname: mixed\nnetworks:\n  lan: {cidr: 10.0.1.0/24}\nmachines:\n  web:\n    networks: {lan: 10}\n    docker: {image: nginx}\n  pc:\n    networks: {lan: 20}\n    vm: {os: debian-12}\n",
        )
        .unwrap();
        assert!(in_host(&spec, Runtime::Hybrid, "web"));
        assert!(!in_host(&spec, Runtime::Hybrid, "pc"));
        let c = console_command(Path::new("/lab"), &spec, Runtime::Hybrid, "web");
        assert!(
            c.contains("vagrant ssh isoloom-docker -c") && c.contains("exec web"),
            "{c}"
        );
        assert!(
            console_command(Path::new("/lab"), &spec, Runtime::Hybrid, "pc")
                .ends_with("vagrant ssh pc")
        );
        let cap = capture_command(
            Path::new("/lab"),
            Runtime::Vagrant,
            "pc",
            false,
            "tcpdump -w -",
        );
        let args: Vec<String> = cap
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(&args[..3], ["ssh", "pc", "-c"]);
    }
}

#[cfg(test)]
mod vagrant_tests {
    use crate::runner::{Runtime, prepare, runtime};

    /// Vagrant accepts the Vagrantfiles the app prepares (needs `vagrant`; `ISOTHRUM_VM_LAB`
    /// keeps the folder).
    #[test]
    #[ignore]
    fn vagrant_validates_the_prepared_labs() {
        let base = std::env::var("ISOTHRUM_VM_LAB")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        let mixed = "version: 1\nname: mixed\nnetworks:\n  office: {cidr: 10.10.1.0/24}\nmachines:\n  web:\n    networks: {office: 10}\n    docker: {image: \"nginx:1.27\"}\n  pc:\n    networks: {office: 20}\n    vm: {os: debian-12, provision: [provision/pc.sh]}\n";
        let vms = "version: 1\nname: vms\nnetworks:\n  office: {cidr: 10.10.1.0/24}\nmachines:\n  a:\n    networks: {office: 10}\n    vm: {os: debian-12, provision: [provision/a.sh]}\n  b:\n    networks: {office: 11}\n    vm: {os: ubuntu-24.04, provision: [provision/b.sh]}\n";
        let script = || "#!/bin/sh\nset -e\n".to_string();
        for (name, yaml, rt) in [
            ("mixed", mixed, Runtime::Hybrid),
            ("vms", vms, Runtime::Vagrant),
        ] {
            let dir = base.join(name);
            let own: Vec<(String, String)> = ["pc", "a", "b"]
                .iter()
                .map(|m| (format!("provision/{m}.sh"), script()))
                .collect();
            prepare(&dir, yaml, &own).unwrap();
            assert_eq!(runtime(&dir), rt);
            let out = super::vagrant(&dir, rt, &["validate"]).output().unwrap();
            assert!(
                out.status.success(),
                "{name}: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let st = super::status(&dir, rt).unwrap();
            assert!(st.contains("\"State\":\"exited\""), "{name}: {st}");
        }
    }
}
