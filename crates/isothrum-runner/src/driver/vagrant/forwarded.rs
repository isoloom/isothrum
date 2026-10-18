//! `vagrant.check_forwarded_ports` (run-manifest-v0.md §11): a VM forwards each published port
//! from this computer at its fixed number, so two labs forwarding one port collide, and Vagrant
//! says so only once the VM was imported. Unless the run's VMs exist (their forwards are this
//! run's own), each such port must be free over its protocol, and not reserved by another run of
//! this runner starting meanwhile.

use std::collections::BTreeSet;

use isoloom_manifest::{Manifest, Protocol, PublishMode, Step};

use super::status;
use crate::Result;
use crate::error::Error;
use crate::executor::Cx;
use crate::status::MachineState;

/// The host ports the VMs forward: every published port `fixed` at a number, each once.
pub(super) fn forwarded(manifest: &Manifest) -> BTreeSet<(u16, Protocol)> {
    manifest
        .machines
        .iter()
        .flat_map(|m| &m.ports)
        .filter_map(|p| {
            let publish = p.publish.as_ref()?;
            (publish.mode == PublishMode::Fixed).then_some((publish.preferred?.get(), p.kind.transport()))
        })
        .collect()
}

/// Runs the action for step `step`.
pub(super) async fn check(cx: &Cx<'_>, step: &Step) -> Result<()> {
    let read = status::parse(&crate::driver::status_report(cx).await?);
    if read.states.values().any(|s| status::machine_state(s) != MachineState::Absent) {
        tracing::debug!(step = %step.id, "the run's VMs exist: their forwards are its own");
        return Ok(());
    }
    let runner = cx.run.runner;
    for (port, protocol) in forwarded(cx.run.manifest()) {
        if runner.reserved_elsewhere(cx.run.name(), port, protocol) || runner.config.ports.in_use(port, protocol) {
            let protocol = if protocol == Protocol::Udp { "udp" } else { "tcp" };
            return Err(Error::PortInUse { port, protocol });
        }
    }
    Ok(())
}
