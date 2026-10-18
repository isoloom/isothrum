//! Hot-adding the on-demand access machine (the learner's attacker box) into a running run, and
//! hot-removing it. The box is a manifest machine marked `on_demand` that the initial `up` left
//! down (a Compose `access` profile, a Vagrant `autostart: false`); `attach` brings it up on a
//! chosen network with the learner's picked image, `detach` takes it back out.

use isoloom_manifest::{Machine, Native};
use serde::Serialize;
use serde_json::json;

use super::Running;
use super::operation::Done;
use crate::Result;
use crate::error::Error;
use crate::events::{Emitter, Events};
use crate::options::RunOptions;
use crate::secrets::sensitive_values;
use crate::state::Op;
use crate::status::{ShellCommand, any_target_running};

/// What a hot-add produced: the access box brought up, and where to reach it.
#[derive(Debug, Clone, Serialize)]
pub struct AttachReport {
    /// The access machine's manifest name.
    pub machine: String,
    /// Its native name (the Compose service, the Vagrant define).
    pub native: String,
    /// The lab network it is on (the chosen one, else its first).
    pub network: Option<String>,
    /// Its static address on that network, for the learner to reach it.
    pub address: Option<String>,
    /// The image or box it was brought up with, when the learner picked one.
    pub image: Option<String>,
}

impl Running<'_> {
    /// Hot-adds the on-demand access machine into the running run: brings its box up with the
    /// learner-picked `image` (its baked default otherwise), on `network` when given. `machine`
    /// names which on-demand machine, needed only when the lab has more than one.
    ///
    /// # Errors
    ///
    /// Fails with `machine_unknown` for a named machine the lab has no on-demand machine for,
    /// `unsupported` when the lab has no on-demand machine (or more than one and none is named, or
    /// the target is a cloud one), `not_running` when the run isn't up, and as the other
    /// operations do for a step or the state.
    pub async fn attach(&self, machine: Option<&str>, network: Option<&str>, image: Option<&str>, events: &dyn Events) -> Result<AttachReport> {
        let opts = self.connected(&RunOptions::default());
        let mask = sensitive_values(self.manifest(), &opts.secrets);
        self.operate(Op::Attach, events, mask, async |e: &Emitter<'_>| {
            let access = self.access_machine(machine)?.clone();
            let native = native_name(&access).to_owned();
            let (_, mut cx) = self.existing(&opts, e).await?;
            if !any_target_running(&self.observe(&mut cx).await?.machines) {
                return Err(Error::NotRunning { run: self.name().to_string() });
            }
            self.driver.attach(&cx, &native, image, network).await?;
            let (network, address) = address_on(&access, network);
            let report = AttachReport {
                machine: access.name.as_str().to_owned(),
                native,
                network,
                address,
                image: image.map(str::to_owned),
            };
            let data = json!({ "machine": report.machine, "native": report.native, "network": report.network, "address": report.address });
            Ok(Done {
                value: report,
                ok: true,
                data,
                state: None,
            })
        })
        .await
    }

    /// Hot-removes the on-demand access machine from the running run, leaving the rest up.
    ///
    /// # Errors
    ///
    /// Fails as [`Running::attach`] does for the machine choice and the target, and as the other
    /// operations do for a step or the state.
    pub async fn detach(&self, machine: Option<&str>, events: &dyn Events) -> Result<()> {
        let opts = self.connected(&RunOptions::default());
        let mask = sensitive_values(self.manifest(), &opts.secrets);
        self.operate(Op::Detach, events, mask, async |e: &Emitter<'_>| {
            let access = self.access_machine(machine)?.clone();
            let native = native_name(&access).to_owned();
            let (_, cx) = self.existing(&opts, e).await?;
            self.driver.detach(&cx, &native).await?;
            Ok(Done {
                value: (),
                ok: true,
                data: json!({ "machine": access.name.as_str(), "native": native }),
                state: None,
            })
        })
        .await
    }

    /// The command that opens an interactive shell into the on-demand access machine: the runner
    /// builds it from the run's Compose project / Vagrant define, so the embedder opens the box by
    /// service or define name and never reconstructs a container name. A pure query — it runs
    /// nothing, emits nothing, and doesn't check the box is up (the embedder runs what it returns
    /// in its own terminal once the box is attached).
    ///
    /// # Errors
    ///
    /// Fails as [`Running::attach`] does for the machine choice, and `unsupported` on a cloud
    /// target (its attacker runs locally, not in the cloud).
    pub fn shell_command(&self, machine: Option<&str>) -> Result<ShellCommand> {
        let access = self.access_machine(machine)?;
        let native = native_name(access).to_owned();
        let (_, provider, params) = self.query_prelude()?;
        let silent = |_: &crate::events::Event| {};
        let emit = Emitter::new(self.name().as_str(), Op::Check, self.runner.config.clock.as_ref(), &silent);
        let cx = self.cx(params, &self.connection, &emit, provider)?;
        self.driver.shell_command(&cx, &native)
    }

    /// The manifest's on-demand access machine: the one `machine` names, or the only one.
    fn access_machine(&self, machine: Option<&str>) -> Result<&Machine> {
        let mut on_demand = self.manifest().machines.iter().filter(|m| m.on_demand);
        if let Some(name) = machine {
            return on_demand.find(|m| m.name.as_str() == name).ok_or_else(|| Error::MachineUnknown {
                run: self.name().to_string(),
                machine: name.to_owned(),
            });
        }
        match (on_demand.next(), on_demand.next()) {
            (Some(one), None) => Ok(one),
            (None, _) => Err(Error::Unsupported {
                what: "hot-adding an access machine: this lab has no on-demand access machine".into(),
            }),
            (Some(_), Some(_)) => {
                let names = self
                    .manifest()
                    .machines
                    .iter()
                    .filter(|m| m.on_demand)
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                Err(Error::Unsupported {
                    what: format!("hot-adding an access machine: this lab has several ({names}); name which"),
                })
            }
        }
    }
}

/// The native name the driver addresses the box by.
fn native_name(m: &Machine) -> &str {
    match &m.native {
        Some(Native::Compose { service }) => service.as_str(),
        Some(Native::Vagrant { vagrant, .. }) => vagrant.as_str(),
        _ => m.name.as_str(),
    }
}

/// The box's static address on `network` (its first when none is named), from the manifest.
fn address_on(m: &Machine, network: Option<&str>) -> (Option<String>, Option<String>) {
    match network {
        Some(n) => (
            Some(n.to_owned()),
            m.addresses.iter().find(|(k, _)| k.as_str() == n).map(|(_, a)| a.to_string()),
        ),
        None => m
            .addresses
            .iter()
            .next()
            .map_or((None, None), |(k, a)| (Some(k.as_str().to_owned()), Some(a.to_string()))),
    }
}
