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

mod capture;
mod lifecycle;
mod machines;
mod snapshots;

use std::path::{Path, PathBuf};
use std::process::Command;

use isoloom_core::Spec;

pub use capture::{CAPTURE_IMAGE, Capture, live_capture_possible, plan_capture, start_capture};
pub use lifecycle::{down, prepare, status, up};
pub use machines::{SHELL, console_command, container, machine_action};
pub use snapshots::{Snapshot, delete_snapshot, snapshots, take_snapshot, use_snapshot};

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

/// A lab's folder under the app's data directory: `<data>/labs/<name>/`.
pub fn lab_dir(data: &Path, name: &str) -> PathBuf {
    data.join("labs").join(name)
}

/// `docker compose` on the lab's file, from its folder.
pub fn compose(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("docker");
    c.current_dir(dir)
        .args(["compose", "-f", ".isoloom/docker/compose.yml"]);
    // Machines brought back from a snapshot run its image (see `restore_snapshot`).
    if dir.join(snapshots::SNAPSHOT_OVERRIDE).is_file() {
        c.args(["-f", snapshots::SNAPSHOT_OVERRIDE]);
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

/// The lab's spec as last started (its folder's isoloom.yml).
pub fn lab_spec(dir: &Path) -> Result<Spec, RunError> {
    let yaml = std::fs::read_to_string(dir.join("isoloom.yml"))
        .map_err(|_| RunError::Spec("start the lab first".into()))?;
    isoloom_core::parse(&yaml).map_err(|e| RunError::Spec(e.to_string()))
}
