//! The ad-hoc attacker box: a Docker container the runner stands up on a running lab's network
//! with no Compose service and no manifest machine behind it. Labs no longer declare an access
//! machine, so `attach` makes one here — an idle container from the learner's image (Kali by
//! default), named after the Compose project, on the lab's network(s), its address read back with
//! `docker inspect`. `detach` removes it by name, idempotently.

use isoloom_manifest::access;
use serde_json::Value;

use super::{ATTACH_TIMEOUT, CALL_TIMEOUT, Compose, docker_spec};
use crate::Result;
use crate::driver::Attached;
use crate::executor::{Cx, Echo, call_ok};
use crate::state::Op;

/// The label the ad-hoc box carries, so it is recognisable as one Isothrum stood up.
const LABEL: &str = "isoloom.access";

/// What the box runs: idle forever, so it stays up for the learner to `exec` into.
const IDLE: [&str; 2] = ["sleep", "infinity"];

/// The `docker inspect` template that prints the container's networks as JSON.
const INSPECT_FORMAT: &str = "{{json .NetworkSettings.Networks}}";

/// The shell the box's `exec` opens: `bash` when the image has it (Kali does), else `sh`.
const SHELL: &str = "command -v bash >/dev/null 2>&1 && exec bash || exec sh";

/// The deterministic name of a project's ad-hoc access box.
pub(super) fn container_name(project: &str) -> String {
    format!("{project}-isoloom-access")
}

/// The argv `docker exec`'s into the ad-hoc box with a TTY: a login shell, `bash` or `sh`.
pub(super) fn shell_args(name: &str) -> Vec<String> {
    vec!["exec".to_owned(), "-it".into(), name.to_owned(), "sh".into(), "-c".into(), SHELL.into()]
}

/// A network's non-empty `IPAddress`, as `docker inspect` writes it.
fn ip_of(net: &Value) -> Option<&str> {
    net.get("IPAddress").and_then(Value::as_str).filter(|a| !a.is_empty())
}

/// The box's address under `docker_net` in the inspected networks JSON `json` (the first network's
/// otherwise); `None` when nothing readable is there.
fn address_from_inspect(json: &str, docker_net: Option<&str>) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    let nets = value.as_object()?;
    if let Some(found) = docker_net.and_then(|n| nets.get(n)).and_then(ip_of) {
        return Some(found.to_owned());
    }
    nets.values().find_map(ip_of).map(str::to_owned)
}

/// The lab's networks by spec name, the chosen one (else the first) at the front: the box lands on
/// that one and is plugged into the rest.
fn network_specs(cx: &Cx<'_>, chosen: Option<&str>) -> Vec<String> {
    let mut specs: Vec<String> = cx.run.manifest().networks.iter().map(|n| n.name.to_string()).collect();
    if let Some(chosen) = chosen {
        specs.retain(|s| s != chosen);
        specs.insert(0, chosen.to_owned());
    }
    specs
}

impl Compose {
    /// Stands an ad-hoc attacker box up on the running lab with no Compose service: an idle
    /// container from `image` (Kali by default), named after the project, on `network` (the lab's
    /// first otherwise) and plugged into the lab's other networks best effort. Reports where it is.
    pub(super) async fn attach_adhoc(&self, cx: &Cx<'_>, image: Option<&str>, network: Option<&str>) -> Result<Attached> {
        let project = self.project(cx)?;
        let name = container_name(&project);
        let specs = network_specs(cx, network);
        let docker_net = |spec: &str| format!("{project}_{spec}");
        let primary = specs.first().map(|s| docker_net(s));
        // Idempotent: a box a previous attach left goes first, so `run --name` is free.
        drop(self.remove_container(cx, &name).await);
        self.run_box(cx, &name, primary.as_deref(), image.unwrap_or(access::DEFAULT_IMAGE)).await?;
        for spec in specs.iter().skip(1) {
            self.connect_named(cx, &name, &docker_net(spec)).await;
        }
        let address = self.box_address(cx, &name, primary.as_deref()).await;
        Ok(Attached {
            name: Some(name),
            network: specs.into_iter().next(),
            address,
        })
    }

    /// Runs the detached idle box: `docker run -d --name <name> [--network <net>] <image> sleep
    /// infinity`. The image may still need pulling, so this gets the long boot budget.
    async fn run_box(&self, cx: &Cx<'_>, name: &str, docker_net: Option<&str>, image: &str) -> Result<()> {
        let mut args = vec![
            "run".to_owned(),
            "-d".into(),
            "--name".into(),
            name.to_owned(),
            "--label".into(),
            format!("{LABEL}=true"),
        ];
        if let Some(net) = docker_net {
            args.push("--network".into());
            args.push(net.to_owned());
        }
        args.push(image.to_owned());
        args.extend(IDLE.iter().map(|s| (*s).to_owned()));
        call_ok(cx, Op::Attach.as_str(), &docker_spec(cx, args, Vec::new()), ATTACH_TIMEOUT, Echo::Shown)
            .await
            .map(drop)
    }

    /// Plugs the box into another lab network `docker_net`, best effort: already on it, or a
    /// network it can't join, is left as it is.
    async fn connect_named(&self, cx: &Cx<'_>, name: &str, docker_net: &str) {
        let args = vec!["network".to_owned(), "connect".into(), docker_net.to_owned(), name.to_owned()];
        if let Err(e) = call_ok(cx, Op::Attach.as_str(), &docker_spec(cx, args, Vec::new()), CALL_TIMEOUT, Echo::Quiet).await {
            tracing::debug!(%e, "ad-hoc access box already on the network, or it can't be joined");
        }
    }

    /// The box's address on `docker_net` (the first network's otherwise), read with
    /// `docker inspect`; `None` when it can't be read (best effort, never fails the attach).
    async fn box_address(&self, cx: &Cx<'_>, name: &str, docker_net: Option<&str>) -> Option<String> {
        let args = vec!["inspect".to_owned(), "-f".into(), INSPECT_FORMAT.into(), name.to_owned()];
        let out = call_ok(cx, Op::Attach.as_str(), &docker_spec(cx, args, Vec::new()), CALL_TIMEOUT, Echo::Quiet)
            .await
            .ok()?;
        address_from_inspect(&out, docker_net)
    }

    /// Removes the box by name: `docker rm -f <name>`. The caller ignores its result where it
    /// wants idempotence (a box already gone is no failure to report).
    pub(super) async fn remove_container(&self, cx: &Cx<'_>, name: &str) -> Result<()> {
        let args = vec!["rm".to_owned(), "-f".into(), name.to_owned()];
        call_ok(cx, Op::Detach.as_str(), &docker_spec(cx, args, Vec::new()), CALL_TIMEOUT, Echo::Shown)
            .await
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_box_is_named_after_the_project() {
        assert_eq!(container_name("flag-portal"), "flag-portal-isoloom-access");
    }

    #[test]
    fn the_shell_execs_into_the_box_with_a_bash_to_sh_fallback() {
        let args = shell_args("flag-portal-isoloom-access");
        assert_eq!(args[..4], ["exec", "-it", "flag-portal-isoloom-access", "sh"]);
        assert_eq!(args[4], "-c");
        assert!(args[5].contains("exec bash") && args[5].ends_with("exec sh"), "{}", args[5]);
    }

    #[test]
    fn the_address_comes_from_the_named_network_else_the_first() {
        let json = r#"{"flag-portal_lab":{"IPAddress":"10.30.0.77"},"flag-portal_dmz":{"IPAddress":"10.40.0.5"}}"#;
        assert_eq!(address_from_inspect(json, Some("flag-portal_dmz")).as_deref(), Some("10.40.0.5"));
        // A network that isn't there falls back to the first readable address.
        assert_eq!(address_from_inspect(json, Some("nope")).as_deref(), Some("10.30.0.77"));
        assert_eq!(address_from_inspect(json, None).as_deref(), Some("10.30.0.77"));
    }

    #[test]
    fn an_empty_or_unreadable_inspect_yields_no_address() {
        assert_eq!(address_from_inspect("not json", None), None);
        assert_eq!(address_from_inspect(r#"{"lab":{"IPAddress":""}}"#, Some("lab")), None);
        assert_eq!(address_from_inspect("{}", None), None);
    }
}
