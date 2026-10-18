//! Isoloom's controller: a Windows machine's own steps are PowerShell scripts, so the Ansible
//! playbooks drawn on one run from the controller Isoloom adds (`provision:`), which reaches it
//! over WinRM, as Isoloom's own Windows examples do.

use crate::document::ProvisionDoc;

/// A playbook the controller runs, and the machine it was drawn on (the one it sets up).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControllerPlaybook {
    /// The playbook's path in the project.
    pub(crate) path: String,
    /// The machine whose provisioning step it is.
    pub(crate) machine: String,
}

impl From<&ControllerPlaybook> for ProvisionDoc {
    fn from(p: &ControllerPlaybook) -> ProvisionDoc {
        ProvisionDoc { ansible: p.path.clone() }
    }
}

/// A controller playbook's starter: it sets up its own machine, over WinRM.
pub(crate) fn starter(machine: &str) -> String {
    format!(
        "# Starter playbook for `{machine}`, run from Isoloom's controller over WinRM, written by Isothrum: edit it to build your project.\n- hosts: {machine}\n  tasks:\n    - name: The machine answers\n      ansible.windows.win_ping:\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_controller_playbook_is_a_provision_step_and_its_starter_sets_up_its_machine() {
        let p = ControllerPlaybook {
            path: "provision/dc01.yml".into(),
            machine: "dc01".into(),
        };
        assert_eq!(
            ProvisionDoc::from(&p),
            ProvisionDoc {
                ansible: "provision/dc01.yml".into()
            }
        );
        let starter = starter("dc01");
        assert!(starter.contains("\n- hosts: dc01\n"), "{starter}");
        assert!(starter.contains("ansible.windows.win_ping:"), "{starter}");
        assert!(!starter.contains("become"), "WinRM has no sudo: {starter}");
    }
}
