//! A lab's life: its folder prepared from the spec, started, read, stopped.

use std::path::Path;

use isoloom_core::{Spec, Target};

use super::{RUNTIME, RunError, Runtime, compose, run, runtime};
use crate::lab_files;
use crate::tools::has;

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

/// Starts the lab; the links suspended before it was started again stay cut.
pub fn up(dir: &Path) -> Result<String, RunError> {
    let mut out = if let rt @ (Runtime::Vagrant | Runtime::Hybrid) = runtime(dir) {
        crate::vms::up(dir, rt)?
    } else {
        run(
            compose(dir, &["up", "-d", "--wait", "--remove-orphans"]),
            "Starting the lab",
        )?
    };
    let notes = super::links::reapply_all_links(dir, &super::lab_spec(dir)?);
    if !notes.is_empty() {
        out.push('\n');
        out.push_str(&notes);
    }
    Ok(out)
}

/// Stops the lab: its machines are gone, and its suspended links with them.
pub fn down(dir: &Path) -> Result<String, RunError> {
    let out = if let rt @ (Runtime::Vagrant | Runtime::Hybrid) = runtime(dir) {
        crate::vms::down(dir, rt)?
    } else {
        run(
            compose(dir, &["down", "--volumes", "--remove-orphans"]),
            "Stopping the lab",
        )?
    };
    super::links::clear_links(dir);
    super::display::remove_forwarders(dir);
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::prepared;
    use std::path::PathBuf;

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
}
