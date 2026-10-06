//! Running a lab on this machine, and capturing its traffic.
//!
//! A lab is the `isoloom.yml` the editor writes. Running it: Isoloom's library generates the
//! Docker Compose files into the lab's folder, and `docker compose` brings them up. Capturing:
//! a throwaway container joins the network namespace of one machine on the LAN or cable, finds
//! the interface on that network, and streams `tcpdump` to Wireshark (or to a `.pcap` file):
//! GNS3's "capture on a link", for a lab of containers.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

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
    Command { what: String, output: String },
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

/// A lab's folder under the app's data directory: `<data>/labs/<name>/`.
pub fn lab_dir(data: &Path, name: &str) -> PathBuf {
    data.join("labs").join(name)
}

/// Checks the spec and writes it with its Docker files into `dir`. Only labs of containers run
/// here for now: a lab with VMs is exported and run with `isoloom run` instead.
pub fn prepare(dir: &Path, yaml: &str) -> Result<Spec, RunError> {
    let spec = isoloom_core::parse(yaml).map_err(|e| RunError::Spec(e.to_string()))?;
    let problems = isoloom_core::validate(&spec);
    if !problems.is_empty() {
        let list: Vec<String> = problems.iter().map(|p| p.to_string()).collect();
        return Err(RunError::Spec(list.join("\n")));
    }
    let files = isoloom_core::generate(&spec, Target::Docker).map_err(|_| {
        RunError::NotDocker(
            "The desktop app runs labs made of containers for now. This lab has VMs: export it and \
             run it with `isoloom run`."
                .into(),
        )
    })?;
    std::fs::create_dir_all(dir).map_err(|e| RunError::Io(e.to_string()))?;
    std::fs::write(dir.join("isoloom.yml"), yaml).map_err(|e| RunError::Io(e.to_string()))?;
    for f in files {
        let path = dir.join(&f.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RunError::Io(e.to_string()))?;
        }
        std::fs::write(&path, f.contents).map_err(|e| RunError::Io(e.to_string()))?;
    }
    Ok(spec)
}

/// `docker compose` on the lab's file, from its folder.
pub fn compose(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("docker");
    c.current_dir(dir).args(["compose", "-f", ".isoloom/docker/compose.yml"]).args(args);
    c
}

/// Runs a command to the end; its output becomes the error when it fails.
pub fn run(mut cmd: Command, what: &str) -> Result<String, RunError> {
    let out = cmd.output().map_err(|e| RunError::Command { what: what.into(), output: e.to_string() })?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(text)
    } else {
        Err(RunError::Command { what: what.into(), output: text })
    }
}

pub fn up(dir: &Path) -> Result<String, RunError> {
    run(compose(dir, &["up", "-d", "--wait"]), "Starting the lab")
}

pub fn down(dir: &Path) -> Result<String, RunError> {
    run(compose(dir, &["down", "--volumes", "--remove-orphans"]), "Stopping the lab")
}

/// The lab's services and their state, as `docker compose ps` reports them (one JSON per line).
pub fn status(dir: &Path) -> Result<String, RunError> {
    run(compose(dir, &["ps", "--all", "--format", "json"]), "Reading the lab's state")
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
    let machine = spec
        .machines
        .iter()
        .find(|(_, m)| m.networks.contains_key(network) && m.docker.is_some())
        .map(|(name, _)| name.clone())
        .ok_or_else(|| RunError::Spec(format!("no container is on `{network}`")))?;
    let id = run(compose(dir, &["ps", "-q", &machine]), "Finding the machine")?;
    let container = id.lines().next().unwrap_or("").trim().to_string();
    if container.is_empty() {
        return Err(RunError::Spec(format!("`{machine}` isn't running: start the lab first")));
    }
    Ok(CapturePlan { container, prefix })
}

/// The capture process: tcpdump in the machine's network namespace, pcap on stdout.
pub fn capture_command(plan: &CapturePlan) -> Command {
    let mut c = Command::new("docker");
    c.args([
        "run",
        "--rm",
        "-i",
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
}

impl Capture {
    pub fn stop(&mut self) {
        for c in &mut self.children {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Whether a program is on the PATH.
pub fn has(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|d| d.join(program).is_file()))
        .unwrap_or(false)
}

/// Starts a capture: live into Wireshark when it is installed, otherwise into `file`.
pub fn start_capture(plan: &CapturePlan, file: &Path) -> Result<Capture, RunError> {
    let mut dump = capture_command(plan);
    if has("wireshark") {
        let mut dump = dump
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| RunError::Io(e.to_string()))?;
        let stdout = dump.stdout.take().ok_or_else(|| RunError::Io("no capture output".into()))?;
        let wireshark = Command::new("wireshark")
            .args(["-k", "-i", "-"])
            .stdin(Stdio::from(stdout))
            .spawn()
            .map_err(|e| RunError::Io(e.to_string()))?;
        return Ok(Capture { children: vec![dump, wireshark], file: None });
    }
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RunError::Io(e.to_string()))?;
    }
    let out = std::fs::File::create(file).map_err(|e| RunError::Io(e.to_string()))?;
    let dump = dump.stdout(out).stderr(Stdio::null()).spawn().map_err(|e| RunError::Io(e.to_string()))?;
    Ok(Capture { children: vec![dump], file: Some(file.to_path_buf()) })
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
        let spec = prepare(&dir, CONTAINERS).unwrap();
        assert_eq!(spec.name, "pair");
        let compose = std::fs::read_to_string(dir.join(".isoloom/docker/compose.yml")).unwrap();
        assert!(compose.contains("name: pair"));
        assert!(dir.join("isoloom.yml").is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_lab_with_vms_says_how_to_run_it() {
        let err = prepare(&temp("vm"), WITH_VM).unwrap_err();
        assert!(matches!(err, RunError::NotDocker(_)));
        assert!(err.to_string().contains("isoloom run"));
    }

    #[test]
    fn an_invalid_spec_is_refused_with_its_problems() {
        let err = prepare(&temp("bad"), "version: 1\nname: x\nnetworks: {}\nmachines: {}\n");
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
        let plan = CapturePlan { container: "abc123".into(), prefix: "10.10.1.".into() };
        let cmd = capture_command(&plan);
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(args.windows(2).any(|w| w == ["--net", "container:abc123"]));
        assert!(args.contains(&CAPTURE_IMAGE.to_string()));
    }
}
