//! Running a lab on this machine, and capturing its traffic.
//!
//! A lab is the `isoloom.yml` the editor writes. Running it: Isoloom's library generates the
//! Docker Compose files into the lab's folder, and `docker compose` brings them up. Capturing:
//! a throwaway container joins the network namespace of one machine on the LAN or cable, finds
//! the interface on that network, and streams `tcpdump` to Wireshark (or to a `.pcap` file):
//! GNS3's "capture on a link", for a lab of containers.
//!
//! A lab with VMs runs through Isoloom's Vagrant output, or its hybrid one when it mixes VMs and
//! containers: `vms.rs`. The lab's folder records which (`runtime`).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::console::Console;
use crate::lab_files;
use crate::tools::has;

use isoloom_core::{Spec, Target};

/// The image the capture runs in: tcpdump, ip and a shell, nothing to install.
pub const CAPTURE_IMAGE: &str = "nicolaka/netshoot:v0.13";

#[derive(Debug)]
pub enum RunError {
    /// The YAML isn't an Isoloom spec.
    Spec(String),
    /// The spec can run, but not as containers on this machine (it has VMs, for example).
    NotDocker(String),
    Io(String),
    /// A command failed; its output says why.
    Command {
        what: String,
        output: String,
    },
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Spec(e) => write!(f, "{e}"),
            RunError::NotDocker(why) => write!(f, "{why}"),
            RunError::Io(e) => write!(f, "{e}"),
            RunError::Command { what, output } => write!(f, "{what} failed:\n{output}"),
        }
    }
}

impl std::error::Error for RunError {}

/// How a lab runs: Docker Compose, or Vagrant (only VMs, or VMs and a Docker host VM for the
/// containers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Docker,
    Vagrant,
    Hybrid,
}

impl Runtime {
    /// How the lab's folder records it.
    fn name(self) -> &'static str {
        match self {
            Runtime::Docker => "docker",
            Runtime::Vagrant => "vagrant",
            Runtime::Hybrid => "hybrid",
        }
    }
}

/// Where the lab's folder records its runtime.
const RUNTIME: &str = "runtime";

/// The runtime the lab was prepared for (Docker when nothing says otherwise).
pub fn runtime(dir: &Path) -> Runtime {
    match std::fs::read_to_string(dir.join(RUNTIME))
        .unwrap_or_default()
        .trim()
    {
        "vagrant" => Runtime::Vagrant,
        "hybrid" => Runtime::Hybrid,
        _ => Runtime::Docker,
    }
}

/// Whether the lab has been prepared (its generated files are there).
pub fn prepared(dir: &Path) -> bool {
    match runtime(dir) {
        Runtime::Docker => dir.join(".isoloom/docker/compose.yml").is_file(),
        rt => crate::vms::dir(dir, rt).join("Vagrantfile").is_file(),
    }
}

/// The runtime that fits the lab and Isoloom's files for it: Docker when every machine is a
/// container, else Vagrant (only VMs), else hybrid (both).
pub fn generated(spec: &Spec) -> Result<(Runtime, Vec<isoloom_core::GeneratedFile>), RunError> {
    [
        (Runtime::Docker, Target::Docker),
        (Runtime::Vagrant, Target::Vagrant),
        (Runtime::Hybrid, Target::Hybrid),
    ]
    .into_iter()
    .find_map(|(rt, t)| isoloom_core::generate(spec, t).ok().map(|f| (rt, f)))
    .ok_or_else(|| {
        RunError::NotDocker(
            "This lab runs neither as containers nor as local VMs: export it and run it with \
             `isoloom run` on a target that fits it."
                .into(),
        )
    })
}

/// A lab's folder under the app's data directory: `<data>/labs/<name>/`.
pub fn lab_dir(data: &Path, name: &str) -> PathBuf {
    data.join("labs").join(name)
}

/// Checks the spec and writes it with its generated files into `dir`: Docker's when every machine
/// is a container, else Vagrant's (only VMs) or the hybrid output's (VMs and containers).
/// `own`: the lab's own files (provisioning scripts, appliance configurations), at the paths
/// isoloom.yml gives them (relative, inside the lab's folder).
pub fn prepare(dir: &Path, yaml: &str, own: &[(String, String)]) -> Result<Spec, RunError> {
    if let Some((path, _)) = own.iter().find(|(p, _)| !lab_files::inside_lab(p)) {
        return Err(RunError::Spec(format!(
            "`{path}` isn't a path inside the lab"
        )));
    }
    let spec = valid_spec(yaml)?;
    let (runtime, generated) = generated(&spec)?;
    if runtime != Runtime::Docker && !has("vagrant") {
        return Err(RunError::NotDocker(
            "This lab has VMs: they run with Vagrant (and VirtualBox or libvirt), which isn't \
             installed. Install it from vagrantup.com, or export the lab."
                .into(),
        ));
    }
    lab_files::write(dir, "isoloom.yml", yaml)?;
    lab_files::write(dir, RUNTIME, runtime.name())?;
    let files: Vec<(&str, &str)> = generated
        .iter()
        .map(|f| (f.path.as_str(), f.contents.as_str()))
        .chain(own.iter().map(|(p, c)| (p.as_str(), c.as_str())))
        .collect();
    lab_files::write_all(dir, &files)?;
    Ok(spec)
}

/// The spec in `yaml`, when Isoloom finds no problem in it.
fn valid_spec(yaml: &str) -> Result<Spec, RunError> {
    let spec = isoloom_core::parse(yaml).map_err(|e| RunError::Spec(e.to_string()))?;
    let problems = isoloom_core::validate(&spec);
    if problems.is_empty() {
        return Ok(spec);
    }
    let list: Vec<String> = problems.iter().map(|p| p.to_string()).collect();
    Err(RunError::Spec(list.join("\n")))
}

/// `docker compose` on the lab's file, from its folder.
pub fn compose(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("docker");
    c.current_dir(dir)
        .args(["compose", "-f", ".isoloom/docker/compose.yml"]);
    // Machines brought back from a snapshot run its image (see `restore_snapshot`).
    if dir.join(SNAPSHOT_OVERRIDE).is_file() {
        c.args(["-f", SNAPSHOT_OVERRIDE]);
    }
    c.args(args);
    c
}

/// Runs a command to the end; its output becomes the error when it fails.
pub fn run(mut cmd: Command, what: &str) -> Result<String, RunError> {
    let out = cmd.output().map_err(|e| RunError::Command {
        what: what.into(),
        output: e.to_string(),
    })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if out.status.success() {
        Ok(text)
    } else {
        Err(RunError::Command {
            what: what.into(),
            output: text,
        })
    }
}

pub fn up(dir: &Path) -> Result<String, RunError> {
    if let rt @ (Runtime::Vagrant | Runtime::Hybrid) = runtime(dir) {
        return crate::vms::up(dir, rt);
    }
    run(
        compose(dir, &["up", "-d", "--wait", "--remove-orphans"]),
        "Starting the lab",
    )
}

pub fn down(dir: &Path) -> Result<String, RunError> {
    if let rt @ (Runtime::Vagrant | Runtime::Hybrid) = runtime(dir) {
        return crate::vms::down(dir, rt);
    }
    run(
        compose(dir, &["down", "--volumes", "--remove-orphans"]),
        "Stopping the lab",
    )
}

// One machine of a running lab: its console, and starting or stopping it alone.

/// The machine's container in the running lab.
pub fn container(dir: &Path, machine: &str) -> Result<String, RunError> {
    let id = run(compose(dir, &["ps", "-q", machine]), "Finding the machine")?
        .trim()
        .to_string();
    if id.is_empty() {
        return Err(RunError::Command {
            what: format!("`{machine}`"),
            output: "isn't running".into(),
        });
    }
    Ok(id)
}

/// A container's shell: bash when it has it, else sh.
pub const SHELL: &str = "command -v bash >/dev/null && exec bash || exec sh";

/// The command a console on `machine` runs: a shell in its container, or SSH to a network
/// appliance's OS (its management address; `admin`/`admin`).
pub fn console_command(dir: &Path, spec: &Spec, machine: &str) -> Result<Console, RunError> {
    let m = spec
        .machines
        .get(machine)
        .ok_or_else(|| RunError::Spec(format!("no machine named `{machine}`")))?;
    if let rt @ (Runtime::Vagrant | Runtime::Hybrid) = runtime(dir) {
        return Ok(crate::vms::console_command(dir, spec, rt, machine));
    }
    let id = container(dir, machine)?;
    if m.docker.as_ref().is_some_and(|d| d.appliance.is_some()) {
        let nets = run(
            {
                let mut c = Command::new("docker");
                c.args(["inspect", "-f", "{{range $n, $v := .NetworkSettings.Networks}}{{$n}} {{$v.IPAddress}}{{println}}{{end}}", &id]);
                c
            },
            "Finding its management address",
        )?;
        let ip = nets
            .lines()
            .find_map(|l| {
                l.split_once(' ')
                    .filter(|(n, _)| n.ends_with("isoloom-mgmt"))
                    .map(|(_, ip)| ip.trim().to_string())
            })
            .filter(|ip| !ip.is_empty())
            .ok_or_else(|| RunError::Command {
                what: format!("`{machine}`"),
                output: "has no management address".into(),
            })?;
        let no_file = if cfg!(windows) { "NUL" } else { "/dev/null" };
        return Ok(Console::new(&[
            "ssh",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            &format!("UserKnownHostsFile={no_file}"),
            &format!("admin@{ip}"),
        ]));
    }
    Ok(Console::new(&[
        "docker", "exec", "-it", &id, "sh", "-c", SHELL,
    ]))
}

/// Starts, stops or restarts one machine (and its network sidecar, which sets its routes).
pub fn machine_action(
    dir: &Path,
    spec: &Spec,
    machine: &str,
    action: &str,
) -> Result<String, RunError> {
    if !spec.machines.contains_key(machine) {
        return Err(RunError::Spec(format!("no machine named `{machine}`")));
    }
    if let rt @ (Runtime::Vagrant | Runtime::Hybrid) = runtime(dir) {
        return crate::vms::action(dir, spec, rt, machine, action);
    }
    let sidecar = format!("{machine}-routes");
    let services_out = run(
        compose(dir, &["config", "--services"]),
        "Reading the lab's services",
    )?;
    let mut services = vec![machine.to_string()];
    if services_out.lines().any(|l| l.trim() == sidecar) {
        services.push(sidecar);
    }
    let names: Vec<&str> = services.iter().map(String::as_str).collect();
    match action {
        "start" => run(
            compose(dir, &[&["up", "-d", "--no-deps"][..], &names].concat()),
            "Starting the machine",
        ),
        "stop" => run(
            compose(dir, &[&["stop"][..], &names].concat()),
            "Stopping the machine",
        ),
        "restart" => {
            run(
                compose(dir, &[&["stop"][..], &names].concat()),
                "Stopping the machine",
            )?;
            run(
                compose(dir, &[&["up", "-d", "--no-deps"][..], &names].concat()),
                "Starting the machine",
            )
        }
        other => Err(RunError::Spec(format!(
            "unknown action `{other}`: start, stop or restart"
        ))),
    }
}

// Snapshots: a machine's container saved as an image (`docker commit`), and brought back by
// running the machine from that image. What's in the container is kept (files, a router's saved
// configuration, a QEMU disk); named volumes aren't (`docker commit` leaves them out).

/// The record of a lab's snapshots, in its folder.
const SNAPSHOTS: &str = "snapshots.json";
/// The Compose override that runs restored machines from their snapshot's image.
const SNAPSHOT_OVERRIDE: &str = "snapshots.override.yml";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub machine: String,
    pub image: String,
    pub label: String,
    /// Seconds since the epoch.
    pub created: u64,
    /// The machine runs from this snapshot now.
    #[serde(default)]
    pub in_use: bool,
}

pub fn snapshots(dir: &Path) -> Vec<Snapshot> {
    std::fs::read_to_string(dir.join(SNAPSHOTS))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_snapshots(dir: &Path, list: &[Snapshot]) -> Result<(), RunError> {
    let text = serde_json::to_string_pretty(list).map_err(|e| RunError::Io(e.to_string()))?;
    std::fs::write(dir.join(SNAPSHOTS), text).map_err(|e| RunError::Io(e.to_string()))?;
    // The override: each machine in use from a snapshot, on that snapshot's image.
    let used: Vec<&Snapshot> = list.iter().filter(|s| s.in_use).collect();
    let path = dir.join(SNAPSHOT_OVERRIDE);
    if used.is_empty() {
        let _ = std::fs::remove_file(path);
        return Ok(());
    }
    let mut yaml = String::from("# Machines restored from a snapshot (Isothrum).\nservices:\n");
    for s in used {
        yaml.push_str(&format!("  {}:\n    image: {}\n", s.machine, s.image));
    }
    std::fs::write(path, yaml).map_err(|e| RunError::Io(e.to_string()))
}

/// The machine and its network sidecar, as Compose services.
fn with_sidecar(dir: &Path, machine: &str) -> Result<Vec<String>, RunError> {
    let services = run(
        compose(dir, &["config", "--services"]),
        "Reading the lab's services",
    )?;
    let sidecar = format!("{machine}-routes");
    let mut out = vec![machine.to_string()];
    if services.lines().any(|l| l.trim() == sidecar) {
        out.push(sidecar);
    }
    Ok(out)
}

/// Saves a machine's container as an image (the machine pauses for the copy).
pub fn take_snapshot(
    dir: &Path,
    lab: &str,
    machine: &str,
    label: &str,
) -> Result<Snapshot, RunError> {
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut list = snapshots(dir);
    let stamp = unique_stamp(&list, machine, created);
    let image = match runtime(dir) {
        Runtime::Docker => commit_container(dir, lab, machine, label, &stamp)?,
        rt => save_vm(dir, rt, machine, &stamp)?,
    };
    let snap = Snapshot {
        id: format!("{machine}-{stamp}"),
        machine: machine.into(),
        image,
        label: match label.trim() {
            "" => "snapshot".into(),
            l => l.into(),
        },
        created,
        in_use: false,
    };
    list.push(snap.clone());
    save_snapshots(dir, &list)?;
    Ok(snap)
}

/// What tells a machine's snapshot apart: the second it was taken, plus `-2`, `-3`… when the
/// machine already has one from that second.
fn unique_stamp(list: &[Snapshot], machine: &str, created: u64) -> String {
    let taken = |stamp: &str| list.iter().any(|s| s.id == format!("{machine}-{stamp}"));
    std::iter::once(created.to_string())
        .chain((2..).map(|n| format!("{created}-{n}")))
        .find(|stamp| !taken(stamp))
        .expect("an unused stamp")
}

/// Saves a machine's container as an image; returns the image.
fn commit_container(
    dir: &Path,
    lab: &str,
    machine: &str,
    label: &str,
    stamp: &str,
) -> Result<String, RunError> {
    let id = container(dir, machine)?;
    let image = format!("isothrum-snapshots/{lab}-{machine}:{stamp}");
    let mut c = Command::new("docker");
    c.args(["commit", "--message", label, &id, &image]);
    run(c, "Saving the machine")?;
    Ok(image)
}

/// Saves a VM with Vagrant's own snapshot (the provider's), named like the record.
fn save_vm(dir: &Path, rt: Runtime, machine: &str, stamp: &str) -> Result<String, RunError> {
    let spec = lab_spec(dir)?;
    if crate::vms::in_host(&spec, rt, machine) {
        return Err(RunError::Spec(
            "snapshots of a container in a lab with VMs come later: snapshot its VMs".into(),
        ));
    }
    let name = format!("{machine}-{stamp}");
    crate::vms::snapshot_save(dir, rt, machine, &name)?;
    Ok(format!("vagrant:{name}"))
}

/// Runs the machine from a snapshot (or, with `None`, from its own image again), recreating it
/// with its sidecar.
pub fn use_snapshot(dir: &Path, machine: &str, id: Option<&str>) -> Result<String, RunError> {
    let mut list = snapshots(dir);
    if let Some(id) = id
        && !list.iter().any(|s| s.id == id && s.machine == machine)
    {
        return Err(RunError::Spec(format!("no snapshot `{id}` of `{machine}`")));
    }
    // A VM goes back to the snapshot's state and carries on from there: nothing to undo later.
    if let rt @ (Runtime::Vagrant | Runtime::Hybrid) = runtime(dir) {
        return match id {
            Some(id) => crate::vms::snapshot_restore(dir, rt, machine, id),
            None => Ok(String::new()),
        };
    }
    for s in list.iter_mut().filter(|s| s.machine == machine) {
        s.in_use = Some(s.id.as_str()) == id;
    }
    save_snapshots(dir, &list)?;
    let services = with_sidecar(dir, machine)?;
    let names: Vec<&str> = services.iter().map(String::as_str).collect();
    run(
        compose(
            dir,
            &[&["up", "-d", "--no-deps", "--force-recreate"][..], &names].concat(),
        ),
        "Bringing the machine back",
    )
}

/// Forgets a snapshot and removes its image (not while the machine runs from it).
pub fn delete_snapshot(dir: &Path, id: &str) -> Result<(), RunError> {
    let mut list = snapshots(dir);
    let Some(i) = list.iter().position(|s| s.id == id) else {
        return Err(RunError::Spec(format!("no snapshot `{id}`")));
    };
    if list[i].in_use {
        return Err(RunError::Spec(format!(
            "`{}` runs from this snapshot: bring it back to its own image first",
            list[i].machine
        )));
    }
    let snap = list.remove(i);
    if let rt @ (Runtime::Vagrant | Runtime::Hybrid) = runtime(dir) {
        crate::vms::snapshot_delete(dir, rt, &snap.machine, &snap.id)?;
        return save_snapshots(dir, &list);
    }
    let _ = run(
        {
            let mut c = Command::new("docker");
            c.args(["rmi", &snap.image]);
            c
        },
        "Removing the snapshot",
    );
    save_snapshots(dir, &list)
}

/// The lab's spec as last started (its folder's isoloom.yml).
pub fn lab_spec(dir: &Path) -> Result<Spec, RunError> {
    let yaml = std::fs::read_to_string(dir.join("isoloom.yml"))
        .map_err(|_| RunError::Spec("start the lab first".into()))?;
    isoloom_core::parse(&yaml).map_err(|e| RunError::Spec(e.to_string()))
}

/// The lab's services and their state, as `docker compose ps` reports them (one JSON per line).
pub fn status(dir: &Path) -> Result<String, RunError> {
    if let rt @ (Runtime::Vagrant | Runtime::Hybrid) = runtime(dir) {
        return crate::vms::status(dir, rt);
    }
    run(
        compose(dir, &["ps", "--all", "--format", "json"]),
        "Reading the lab's state",
    )
}

/// The first three octets of a /24 to /29 block, with the trailing dot: what every address on
/// that network starts with (`10.10.1.0/24` -> `10.10.1.`).
pub fn prefix_of(cidr: &str) -> Option<String> {
    let addr = cidr.split('/').next()?;
    let (head, _) = addr.rsplit_once('.')?;
    Some(format!("{head}."))
}

/// The shell run inside the capture container: find the interface whose address starts with
/// `prefix`, then stream its packets as pcap on stdout. The prefix is checked by the caller to
/// be digits and dots only, so it can't break out of the quotes.
pub fn capture_script(prefix: &str) -> String {
    format!(
        "IF=$(ip -o -4 addr show | awk -v p='{prefix}' 'index($4, p) == 1 {{ print $2; exit }}'); \
         if [ -z \"$IF\" ]; then echo 'no interface on {prefix}0' >&2; exit 1; fi; \
         exec tcpdump -i \"$IF\" -U -w -"
    )
}

/// Where to capture: a machine on the network, by its container, and the network's block.
pub struct CapturePlan {
    pub container: String,
    pub prefix: String,
    /// In a lab with VMs: the lab's folder, its runtime, the machine, and whether it's a
    /// container in the Docker host VM.
    pub vm: Option<(PathBuf, Runtime, String, bool)>,
}

/// Picks the machine to capture from: on `network`, the first one Isoloom attached to it.
/// Every machine on a LAN or cable sees its traffic on that segment, so any member works.
pub fn plan_capture(dir: &Path, spec: &Spec, network: &str) -> Result<CapturePlan, RunError> {
    let net = spec
        .networks
        .get(network)
        .ok_or_else(|| RunError::Spec(format!("the lab has no network `{network}`")))?;
    let prefix = prefix_of(&net.cidr)
        .filter(|p| p.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .ok_or_else(|| RunError::Spec(format!("`{}` isn't an IPv4 block", net.cidr)))?;
    if let rt @ (Runtime::Vagrant | Runtime::Hybrid) = runtime(dir) {
        let machine = spec
            .machines
            .iter()
            .find(|(_, m)| m.networks.contains_key(network))
            .map(|(name, _)| name.clone())
            .ok_or_else(|| RunError::Spec(format!("no machine is on `{network}`")))?;
        let in_host = crate::vms::in_host(spec, rt, &machine);
        return Ok(CapturePlan {
            container: String::new(),
            prefix,
            vm: Some((dir.to_path_buf(), rt, machine, in_host)),
        });
    }
    let machine = spec
        .machines
        .iter()
        .find(|(_, m)| m.networks.contains_key(network) && m.docker.is_some())
        .map(|(name, _)| name.clone())
        .ok_or_else(|| RunError::Spec(format!("no container is on `{network}`")))?;
    let id = run(compose(dir, &["ps", "-q", &machine]), "Finding the machine")?;
    let container = id.lines().next().unwrap_or("").trim().to_string();
    if container.is_empty() {
        return Err(RunError::Spec(format!(
            "`{machine}` isn't running: start the lab first"
        )));
    }
    Ok(CapturePlan {
        container,
        prefix,
        vm: None,
    })
}

/// The capture process: tcpdump in the machine's network namespace, pcap on stdout. On Docker,
/// its container is called `name`, so that stopping the capture can remove it.
pub fn capture_command(plan: &CapturePlan, name: &str) -> Command {
    if let Some((dir, rt, machine, in_host)) = &plan.vm {
        return crate::vms::capture_command(
            dir,
            *rt,
            machine,
            *in_host,
            &capture_script(&plan.prefix),
        );
    }
    let mut c = Command::new("docker");
    c.args([
        "run",
        "--rm",
        "-i",
        "--name",
        name,
        "--net",
        &format!("container:{}", plan.container),
        "--cap-add",
        "NET_RAW",
        "--cap-add",
        "NET_ADMIN",
        CAPTURE_IMAGE,
        "sh",
        "-c",
        &capture_script(&plan.prefix),
    ]);
    c
}

/// A running capture: the tcpdump container, and Wireshark when it reads from it.
pub struct Capture {
    pub children: Vec<Child>,
    /// Where the packets go when Wireshark isn't there.
    pub file: Option<PathBuf>,
    /// The capture's container (Docker labs): killing its `docker run` client may leave it.
    pub container: Option<String>,
}

impl Capture {
    pub fn stop(&mut self) {
        for c in &mut self.children {
            stop_child(c);
        }
        if let Some(name) = &self.container {
            let _ = remove_container(name).output();
        }
    }
}

fn stop_child(c: &mut Child) {
    let _ = c.kill();
    let _ = c.wait();
}

/// `docker rm -f <name>`.
fn remove_container(name: &str) -> Command {
    let mut c = Command::new("docker");
    c.args(["rm", "-f", name]);
    c
}

/// A name no other capture has: `isothrum-capture-<pid>-<n>`.
fn capture_name() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("isothrum-capture-{}-{n}", std::process::id())
}

/// Whether Wireshark can show a capture live: it hands even a piped capture to `dumpcap`, which
/// Linux distributions restrict to the `wireshark` group. Without it, Wireshark opens only to
/// fail with "Couldn't run dumpcap in child process: Permission denied".
pub fn live_capture_possible() -> bool {
    has("wireshark")
        && Command::new("dumpcap")
            .arg("-v")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
}

/// Starts a capture: live into Wireshark when it can show it, otherwise into `file`.
pub fn start_capture(plan: &CapturePlan, file: &Path) -> Result<Capture, RunError> {
    let name = capture_name();
    let dump = capture_command(plan, &name);
    let container = plan.vm.is_none().then_some(name);
    let (children, file) = if live_capture_possible() {
        (into_wireshark(dump)?, None)
    } else {
        (vec![into_file(dump, file)?], Some(file.to_path_buf()))
    };
    Ok(Capture {
        children,
        file,
        container,
    })
}

/// Runs the capture into Wireshark; stops the capture when Wireshark doesn't start.
fn into_wireshark(mut dump: Command) -> Result<Vec<Child>, RunError> {
    let mut dump = dump
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| RunError::Io(e.to_string()))?;
    let wireshark = dump
        .stdout
        .take()
        .ok_or_else(|| RunError::Io("no capture output".into()))
        .and_then(|out| {
            Command::new("wireshark")
                .args(["-k", "-i", "-"])
                .stdin(Stdio::from(out))
                .spawn()
                .map_err(|e| RunError::Io(format!("couldn't start Wireshark: {e}")))
        });
    match wireshark {
        Ok(w) => Ok(vec![dump, w]),
        Err(e) => {
            stop_child(&mut dump);
            Err(e)
        }
    }
}

/// Runs the capture into a `.pcap` file.
fn into_file(mut dump: Command, file: &Path) -> Result<Child, RunError> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RunError::Io(e.to_string()))?;
    }
    let out = std::fs::File::create(file).map_err(|e| RunError::Io(e.to_string()))?;
    dump.stdout(out)
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| RunError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTAINERS: &str = r#"
version: 1
name: pair
networks:
  office: { cidr: 10.10.1.0/24 }
machines:
  a:
    networks: { office: 10 }
    docker: { image: "alpine:3.20" }
  b:
    networks: { office: 11 }
    docker: { image: "alpine:3.20" }
"#;

    const WITH_VM: &str = r#"
version: 1
name: mixed
networks:
  office: { cidr: 10.10.1.0/24 }
machines:
  a:
    networks: { office: 10 }
    docker: { image: "alpine:3.20" }
  b:
    networks: { office: 11 }
    vm: { os: debian-12, provision: [provision/b.sh] }
"#;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("isothrum-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_lab_of_containers_gets_its_compose_file() {
        let dir = temp("prepare");
        let spec = prepare(&dir, CONTAINERS, &[]).unwrap();
        assert_eq!(spec.name, "pair");
        let compose = std::fs::read_to_string(dir.join(".isoloom/docker/compose.yml")).unwrap();
        assert!(compose.contains("name: pair"));
        assert!(dir.join("isoloom.yml").is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_lab_with_vms_runs_with_vagrant() {
        let spec = isoloom_core::parse(WITH_VM).unwrap();
        let (rt, files) = generated(&spec).unwrap();
        assert_eq!(rt, Runtime::Hybrid);
        assert!(
            files
                .iter()
                .any(|f| f.path == ".isoloom/hybrid/Vagrantfile")
        );
        let only_vm = WITH_VM.replace(
            "    docker: { image: \"alpine:3.20\" }\n",
            "    vm: { os: debian-12 }\n",
        );
        let (rt, _) = generated(&isoloom_core::parse(&only_vm).unwrap()).unwrap();
        assert_eq!(rt, Runtime::Vagrant);
        // Prepared: the runtime is recorded and the Vagrantfile found (when Vagrant is installed).
        let dir = temp("vm");
        match prepare(&dir, WITH_VM, &[]) {
            Ok(_) => {
                assert_eq!(runtime(&dir), Runtime::Hybrid);
                assert!(prepared(&dir));
            }
            Err(e) => assert!(!has("vagrant") && e.to_string().contains("Vagrant"), "{e}"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_labs_own_files_stay_inside_it() {
        let dir = temp("own");
        prepare(
            &dir,
            CONTAINERS,
            &[("configs/r1.cfg".into(), "hostname r1\n".into())],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("configs/r1.cfg")).unwrap(),
            "hostname r1\n"
        );
        for bad in ["../escape.sh", "/etc/passwd", "a/../../b"] {
            assert!(
                prepare(&dir, CONTAINERS, &[(bad.into(), String::new())]).is_err(),
                "{bad}"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_invalid_spec_is_refused_with_its_problems() {
        let err = prepare(
            &temp("bad"),
            "version: 1\nname: x\nnetworks: {}\nmachines: {}\n",
            &[],
        );
        assert!(matches!(err, Err(RunError::Spec(_))));
    }

    #[test]
    fn the_capture_finds_the_interface_by_its_network() {
        assert_eq!(prefix_of("10.10.1.0/24").as_deref(), Some("10.10.1."));
        assert_eq!(prefix_of("10.200.0.8/29").as_deref(), Some("10.200.0."));
        let script = capture_script("10.10.1.");
        assert!(script.contains("p='10.10.1.'"));
        assert!(script.contains("exec tcpdump -i \"$IF\" -U -w -"));
    }

    #[test]
    fn the_capture_runs_in_the_machine_namespace() {
        let plan = CapturePlan {
            container: "abc123".into(),
            prefix: "10.10.1.".into(),
            vm: None,
        };
        let cmd = capture_command(&plan, "isothrum-capture-1-0");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|w| w == ["--net", "container:abc123"]));
        assert!(args.contains(&CAPTURE_IMAGE.to_string()));
        assert!(
            args.windows(2)
                .any(|w| w == ["--name", "isothrum-capture-1-0"])
        );
    }

    #[test]
    fn captures_have_their_own_names() {
        assert_ne!(capture_name(), capture_name());
        let rm = remove_container("isothrum-capture-1-0");
        let args: Vec<&std::ffi::OsStr> = rm.get_args().collect();
        assert_eq!(args, ["rm", "-f", "isothrum-capture-1-0"]);
    }

    #[test]
    fn two_snapshots_in_one_second_get_their_own_ids() {
        let snap = |id: &str| Snapshot {
            id: id.into(),
            machine: "pc".into(),
            image: String::new(),
            label: String::new(),
            created: 100,
            in_use: false,
        };
        assert_eq!(unique_stamp(&[], "pc", 100), "100");
        assert_eq!(unique_stamp(&[snap("pc-100")], "pc", 100), "100-2");
        assert_eq!(
            unique_stamp(&[snap("pc-100"), snap("pc-100-2")], "pc", 100),
            "100-3"
        );
        assert_eq!(unique_stamp(&[snap("pc-100")], "srv", 100), "100");
    }

    /// Needs Docker (run with `--ignored`): a machine's console and starting/stopping it alone.
    #[test]
    #[ignore]
    fn one_machine_stops_starts_and_has_a_console() {
        let dir = std::env::temp_dir().join(format!("isothrum-machine-{}", std::process::id()));
        let yaml = "version: 1\nname: onem\nnetworks:\n  a: { cidr: 10.61.1.0/24 }\n  b: { cidr: 10.61.2.0/24 }\nreach: [{ from: a, to: b }]\nmachines:\n  pc: { networks: { a: 10 }, docker: { image: \"alpine:3.20\", idle: true } }\n  srv: { networks: { b: 10 }, docker: { image: \"alpine:3.20\", idle: true } }\n";
        let spec = prepare(&dir, yaml, &[]).unwrap();
        up(&dir).unwrap();
        let running = |m: &str| {
            !run(compose(&dir, &["ps", "-q", m]), "ps")
                .unwrap()
                .trim()
                .is_empty()
        };
        let cmd = console_command(&dir, &spec, "pc").unwrap();
        assert_eq!(cmd.argv[..3], ["docker", "exec", "-it"]);
        assert_eq!(cmd.argv.last().map(String::as_str), Some(SHELL));
        machine_action(&dir, &spec, "pc", "stop").unwrap();
        assert!(
            !running("pc") && !running("pc-routes"),
            "the machine and its sidecar stop"
        );
        assert!(running("srv"), "the others keep running");
        machine_action(&dir, &spec, "pc", "start").unwrap();
        assert!(running("pc") && running("pc-routes"));
        // Its routes came back with the sidecar.
        let routes = run(
            compose(&dir, &["exec", "-T", "pc", "ip", "route"]),
            "routes",
        )
        .unwrap();
        assert!(routes.contains("10.61.2.0/24 via"), "{routes}");
        assert!(machine_action(&dir, &spec, "nobody", "stop").is_err());
        down(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Needs Docker (run with `--ignored`): a snapshot keeps what was in the machine.
    #[test]
    #[ignore]
    fn a_snapshot_brings_a_machine_back_as_it_was() {
        let dir = std::env::temp_dir().join(format!("isothrum-snap-{}", std::process::id()));
        let yaml = "version: 1\nname: snap\nnetworks:\n  a: { cidr: 10.62.1.0/24 }\nmachines:\n  pc: { networks: { a: 10 }, docker: { image: \"alpine:3.20\", idle: true } }\n";
        prepare(&dir, yaml, &[]).unwrap();
        up(&dir).unwrap();
        let sh = |cmd: &str| {
            run(
                compose(&dir, &["exec", "-T", "pc", "sh", "-c", cmd]),
                "exec",
            )
        };
        sh("echo kept > /root/note").unwrap();
        let snap = take_snapshot(&dir, "snap", "pc", "before the change").unwrap();
        sh("rm /root/note").unwrap();
        use_snapshot(&dir, "pc", Some(&snap.id)).unwrap();
        assert_eq!(sh("cat /root/note").unwrap().trim(), "kept", "restored");
        assert!(snapshots(&dir)[0].in_use);
        assert!(delete_snapshot(&dir, &snap.id).is_err(), "not while in use");
        use_snapshot(&dir, "pc", None).unwrap();
        assert!(sh("cat /root/note").is_err(), "its own image again");
        assert!(!dir.join(SNAPSHOT_OVERRIDE).exists());
        delete_snapshot(&dir, &snap.id).unwrap();
        assert!(snapshots(&dir).is_empty());
        let gone = Command::new("docker")
            .args(["image", "inspect", &snap.image])
            .output()
            .unwrap();
        assert!(!gone.status.success(), "its image is removed");
        down(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
