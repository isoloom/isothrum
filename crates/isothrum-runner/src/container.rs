//! A tool run from its pinned image instead of the host's binary, where the manifest says so
//! (`tools[].container` on `container_hosts`): Terraform for a Proxmox server on macOS, whose
//! Local Network Privacy keeps third-party binaries (the bpg/proxmox provider) off the LAN while
//! Docker Desktop carries a container's traffic. The decision is the manifest's and the host's
//! only, so every operation of a run takes the same way.
//!
//! The container gets the run's folder, the run's state folder, the runner's SSH folder and the
//! shared plugin cache, each at its own path (so every path in the module, the state and the
//! variables stays valid), and the step's environment by name (`-e NAME`): no value is ever on
//! its command line.

use std::path::{Path, PathBuf};

use isoloom_manifest::Tool;

use crate::Result;
use crate::executor::Cx;
use crate::process::ProcessSpec;

/// The tool of the manifest that runs `program` from an image on this host, if one does.
pub(crate) fn tool_for<'a>(cx: &'a Cx<'_>, program: &str) -> Option<&'a Tool> {
    let host = cx.run.runner.config.host;
    cx.run
        .manifest()
        .tools
        .iter()
        .find(|t| t.container.is_some() && t.container_hosts.contains(&host) && program_of(t).is_some_and(|p| p == program))
}

/// The program `tool`'s probe runs: the tool's own binary.
fn program_of(tool: &Tool) -> Option<&str> {
    match &tool.probe {
        isoloom_manifest::HostArgv::Every(a) => Some(a.program().as_str()),
        isoloom_manifest::HostArgv::PerHost(p) => p.default.as_ref().map(|a| a.program().as_str()),
    }
}

/// `spec`, run from its tool's image when the manifest says so on this host; else as it is.
pub(crate) fn wrap(cx: &Cx<'_>, spec: ProcessSpec) -> Result<ProcessSpec> {
    let Some(image) = tool_for(cx, &spec.program).and_then(|t| t.container.as_ref()) else {
        return Ok(spec);
    };
    let state = cx.run.runner.store.run_dir(cx.run.name())?;
    let home = &cx.run.runner.config.home;
    let mounts = mounts(&[lab_root(cx.run.dir()), &state, &home.join("ssh"), &crate::driver::plugin_cache(home)]);
    Ok(docker_run(image.as_str(), &mounts, spec))
}

/// The run folder a manifest's folder belongs to: the parent of its `.isoloom…` folder (the
/// folder itself when it isn't under one). A module reads the run's files from there.
pub(crate) fn lab_root(dir: &Path) -> &Path {
    dir.ancestors()
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".isoloom")))
        .and_then(Path::parent)
        .unwrap_or(dir)
}

/// `folders`, sorted, each once, none inside another.
fn mounts(folders: &[&Path]) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = folders.iter().map(|p| p.to_path_buf()).collect();
    all.sort();
    all.dedup();
    let outer = all.clone();
    all.retain(|p| !outer.iter().any(|o| o != p && p.starts_with(o)));
    all
}

/// `spec` as `docker run` of `image`: in its folder, with `mounts` at their own paths, its
/// environment by name, its input piped.
fn docker_run(image: &str, mounts: &[PathBuf], spec: ProcessSpec) -> ProcessSpec {
    let mut args: Vec<String> = vec!["run".into(), "--rm".into(), "-i".into()];
    if let Some(cwd) = &spec.cwd {
        args.extend(["-w".into(), cwd.display().to_string()]);
    }
    for m in mounts {
        let m = m.display().to_string();
        args.extend(["-v".into(), format!("{m}:{m}")]);
    }
    // `-e NAME` passes each variable by name only, never its value on the command line. The value
    // still rides the container's environment, so while this ephemeral `--rm` container runs its
    // value is visible via `docker inspect` / `/proc/1/environ` to anyone with Docker-daemon access
    // (already a host compromise). A `-var-file` mount would avoid even that (reviewed, finding S5).
    for (name, _) in &spec.env {
        args.extend(["-e".into(), name.clone()]);
    }
    args.push(image.to_owned());
    args.extend(spec.args);
    ProcessSpec {
        program: "docker".into(),
        args,
        cwd: spec.cwd,
        env: spec.env,
        stdin: spec.stdin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lab_root_is_above_its_isoloom_folder() {
        assert_eq!(lab_root(Path::new("/l/x/.isoloom/proxmox")), Path::new("/l/x"));
        assert_eq!(lab_root(Path::new("/l/x/.isoloom-2/docker-vm/proxmox")), Path::new("/l/x"));
        assert_eq!(lab_root(Path::new("/tmp/m")), Path::new("/tmp/m"));
    }

    #[test]
    fn a_folder_inside_another_mount_isnt_mounted_twice() {
        let m = mounts(&[Path::new("/h/runs/r"), Path::new("/h"), Path::new("/l"), Path::new("/h/ssh"), Path::new("/l")]);
        assert_eq!(m, [PathBuf::from("/h"), PathBuf::from("/l")]);
    }

    #[test]
    fn the_container_gets_the_variables_by_name_never_their_values() {
        let spec = ProcessSpec {
            program: "terraform".into(),
            args: vec!["apply".into(), "-auto-approve".into()],
            cwd: Some("/l/x/.isoloom/proxmox".into()),
            env: vec![("TF_VAR_proxmox_password".into(), "s3cret".into())],
            stdin: None,
        };
        let run = docker_run("hashicorp/terraform:1.9.8@sha256:00", &[PathBuf::from("/l/x"), PathBuf::from("/h")], spec);
        assert_eq!(run.program, "docker");
        assert_eq!(
            run.args,
            [
                "run",
                "--rm",
                "-i",
                "-w",
                "/l/x/.isoloom/proxmox",
                "-v",
                "/l/x:/l/x",
                "-v",
                "/h:/h",
                "-e",
                "TF_VAR_proxmox_password",
                "hashicorp/terraform:1.9.8@sha256:00",
                "apply",
                "-auto-approve"
            ]
        );
        assert!(!run.args.iter().any(|a| a.contains("s3cret")));
        assert_eq!(run.env, [("TF_VAR_proxmox_password".to_owned(), "s3cret".to_owned())]);
    }
}
