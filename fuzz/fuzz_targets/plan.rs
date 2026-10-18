//! The runner's step planning on any manifest: every step of a manifest that parses is planned on
//! every host and provider (elsewhere, a command, an action, or refused with why), never a panic,
//! the same twice; and with every run value bound, no step is refused for a `${run.…}` one.

#![no_main]

use isothrum_runner::fuzzing::{Planned, Values, plan};
use isothrum_runner::isoloom_manifest::{self, HostOs, RunVar};
use libfuzzer_sys::fuzz_target;

/// Every run value, bound to a plain word.
fn run_values() -> Values {
    let mut values = Values::default();
    for var in [
        RunVar::Name,
        RunVar::Dir,
        RunVar::StateDir,
        RunVar::Provider,
        RunVar::SshKey,
        RunVar::KnownHosts,
        RunVar::SshPublicKey,
        RunVar::Slot,
    ] {
        values.run(var, "x");
    }
    values
}

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let Ok(manifest) = isoloom_manifest::parse(text) else { return };
    let values = run_values();
    for host in [HostOs::Linux, HostOs::Macos, HostOs::Windows] {
        for provider in [None, Some("virtualbox"), Some("hyperv"), Some("libvirt")] {
            let planned = plan(&manifest, host, provider, &values);
            assert_eq!(planned, plan(&manifest, host, provider, &values), "planning is deterministic");
            for (at, step) in &planned {
                match step {
                    Planned::Command { program, .. } => assert!(!program.is_empty(), "{at}: a command with no program"),
                    Planned::Refused(why) => assert!(!why.contains("`${run."), "{at}: a bound run value refused: {why}"),
                    _ => {}
                }
            }
        }
    }
});
