//! Hot-adding the on-demand access box into a running run, and taking it back out, through the
//! fake engines: the Compose `attacker` service in its `access` profile, and the Vagrant
//! `attacker` define. No real tool ever runs.

use super::*;
use crate::ErrorCode;
use crate::testing::{FLAG_PORTAL_ACCESS, FLAG_VM_ACCESS};

#[tokio::test]
async fn attach_brings_the_access_service_up_in_its_profile_with_the_picked_image() {
    let f = fixture_with(FLAG_PORTAL_ACCESS);
    f.set(Containers::Up); // the lab is already up; the access box was left out of it
    let run = f.runner.open(&f.manifest, None).unwrap();

    let events = Recorder::new();
    let report = run.attach(None, Some("lab"), Some("myimage"), &events).await.unwrap();

    assert_eq!((report.machine.as_str(), report.native.as_str()), ("attacker", "attacker"));
    assert_eq!(report.network.as_deref(), Some("lab"));
    assert_eq!(report.address.as_deref(), Some("10.30.0.99"), "its static address, for the launcher to show");
    assert_eq!(report.image.as_deref(), Some("myimage"));

    let up = f.fake.last("--profile access up").expect("the access profile is brought up");
    assert!(up.line().contains("--no-deps attacker"), "{}", up.line());
    assert_eq!(up.env(isoloom_manifest::access::IMAGE_ENV), Some("myimage"), "the picked image is delivered");
    assert!(matches!(kinds(&events).last(), Some(EventKind::Result { ok: true, .. })));
}

#[tokio::test]
async fn attach_without_an_image_lets_the_services_default_stand() {
    let f = fixture_with(FLAG_PORTAL_ACCESS);
    f.set(Containers::Up);
    let run = f.runner.open(&f.manifest, None).unwrap();
    run.attach(None, None, None, &Recorder::new()).await.unwrap();
    let up = f.fake.last("--profile access up").unwrap();
    assert_eq!(
        up.env(isoloom_manifest::access::IMAGE_ENV),
        None,
        "no override: the service's baked default stands"
    );
}

#[tokio::test]
async fn detach_removes_just_the_access_service() {
    let f = fixture_with(FLAG_PORTAL_ACCESS);
    f.set(Containers::Up);
    let run = f.runner.open(&f.manifest, None).unwrap();
    let events = Recorder::new();
    run.detach(None, &events).await.unwrap();
    let rm = f.fake.last("--profile access rm").expect("the access service is removed");
    assert!(rm.line().contains("rm -sf attacker"), "{}", rm.line());
    assert!(matches!(kinds(&events).last(), Some(EventKind::Result { ok: true, .. })));
}

#[tokio::test]
async fn attach_refuses_a_lab_with_no_on_demand_access_box() {
    let f = fixture(); // flag-portal: no access machine
    f.set(Containers::Up);
    let run = f.runner.open(&f.manifest, None).unwrap();
    let err = run.attach(None, None, None, &Recorder::new()).await.unwrap_err();
    assert_eq!(err.code(), ErrorCode::Unsupported);
}

#[tokio::test]
async fn attach_refuses_an_unknown_named_machine() {
    let f = fixture_with(FLAG_PORTAL_ACCESS);
    f.set(Containers::Up);
    let run = f.runner.open(&f.manifest, None).unwrap();
    let err = run.attach(Some("nope"), None, None, &Recorder::new()).await.unwrap_err();
    assert_eq!(err.code(), ErrorCode::MachineUnknown);
}

#[tokio::test]
async fn attach_refuses_a_lab_that_is_not_running() {
    let f = fixture_with(FLAG_PORTAL_ACCESS);
    // containers left at None: nothing of the run is up
    let run = f.runner.open(&f.manifest, None).unwrap();
    let err = run.attach(None, None, None, &Recorder::new()).await.unwrap_err();
    assert_eq!(err.code(), ErrorCode::NotRunning);
}

/// A runner over the vagrant access fixture (`FLAG_VM_ACCESS`), its Vagrant answered by `answer`.
fn vagrant_access(answer: impl Fn(&Call) -> Reply + Send + Sync + 'static) -> (LabFolder, Arc<FakeProcesses>, Runner) {
    let folder = LabFolder::new("vagrant", FLAG_VM_ACCESS);
    let fake = Arc::new(FakeProcesses::new(answer));
    let ports: Arc<dyn crate::HostPorts> = Arc::new(FakePorts::new(std::iter::empty::<u16>()));
    let runner = Runner::new(runner_config(folder.home.path(), processes(&fake), ports, crate::this_host())).unwrap();
    (folder, fake, runner)
}

#[tokio::test]
async fn vagrant_attach_boots_the_access_vm_with_the_picked_box() {
    // A fake Vagrant that reports the lab's web VM up, and succeeds at everything else.
    let (folder, fake, runner) = vagrant_access(|call| {
        let line = call.line();
        if line.contains("plugin list") {
            return Reply::ok("");
        }
        if line.contains("status") {
            return Reply::ok("1,web,state,running\n1,isoloom-controller,state,running\n1,attacker,state,not_created");
        }
        Reply::ok("")
    });
    let run = runner.open(&folder.manifest, None).unwrap();

    let report = run.attach(None, None, Some("mybox"), &Recorder::new()).await.unwrap();
    assert_eq!((report.machine.as_str(), report.native.as_str()), ("attacker", "attacker"));

    let up = fake.last("up attacker").expect("the access VM is booted");
    assert_eq!(up.env(isoloom_manifest::access::BOX_ENV), Some("mybox"), "the picked box is delivered");

    run.detach(None, &Recorder::new()).await.unwrap();
    assert!(fake.last("destroy -f attacker").is_some(), "the access VM is destroyed");
}

#[tokio::test]
async fn shell_command_opens_the_compose_service_by_name() {
    let f = fixture_with(FLAG_PORTAL_ACCESS);
    let run = f.runner.open(&f.manifest, None).unwrap();
    let cmd = run.shell_command(None).unwrap();
    assert_eq!(cmd.program, "docker");
    let line = cmd.args.join(" ");
    assert!(line.starts_with("compose --project-name"), "{line}");
    assert!(line.ends_with("exec attacker bash"), "addresses the service, no container guessing: {line}");
    assert!(cmd.cwd.is_some(), "runs in the run's folder");
}

#[tokio::test]
async fn shell_command_opens_the_vagrant_define_by_name() {
    let (folder, _fake, runner) = vagrant_access(|_| Reply::ok(""));
    let run = runner.open(&folder.manifest, None).unwrap();
    let cmd = run.shell_command(None).unwrap();
    assert_eq!(cmd.program, "vagrant");
    assert_eq!(cmd.args, vec!["ssh".to_owned(), "attacker".to_owned()]);
}

#[tokio::test]
async fn shell_command_reuses_the_access_machine_selection() {
    let f = fixture_with(FLAG_PORTAL_ACCESS);
    let run = f.runner.open(&f.manifest, None).unwrap();
    assert_eq!(run.shell_command(Some("nope")).unwrap_err().code(), ErrorCode::MachineUnknown);

    let none = fixture();
    let bare = none.runner.open(&none.manifest, None).unwrap();
    assert_eq!(bare.shell_command(None).unwrap_err().code(), ErrorCode::Unsupported);
}
