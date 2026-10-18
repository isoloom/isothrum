use std::path::Path;

use super::*;

/// A run's project: `isoloom.yml`, its module `terraform/` (a variable file, a sub-module, a
/// provider cache and a state the copy leaves out), and the manifest's folder.
fn project() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let module = root.path().join("terraform");
    std::fs::create_dir_all(module.join("modules/site")).unwrap();
    std::fs::create_dir_all(module.join(".terraform/providers")).unwrap();
    for (file, text) in [
        ("main.tf", "resource {}"),
        ("vars.auto.tfvars.json", "{}"),
        ("modules/site/main.tf", "x"),
        ("index.html", "hello"),
        ("terraform.tfstate", "{}"),
        ("terraform.tfstate.backup", "{}"),
        (".terraform/providers/aws", "binary"),
    ] {
        std::fs::write(module.join(file), text).unwrap();
    }
    let dir = root.path().join(".isoloom/cloud-services");
    std::fs::create_dir_all(&dir).unwrap();
    (root, dir)
}

/// The files under `dir`, relative and sorted.
fn files(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            if e.path().is_dir() {
                walk(base, &e.path(), out);
            } else {
                out.push(e.path().strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn the_module_is_copied_without_its_providers_or_state() {
    let (_root, dir) = project();
    let source = inside_project(&dir, "../../terraform").unwrap();
    let copy = tempfile::tempdir().unwrap();
    copy_tree(&source, copy.path()).unwrap();
    assert_eq!(files(copy.path()), ["index.html", "main.tf", "modules/site/main.tf", "vars.auto.tfvars.json"]);
}

#[test]
fn a_new_copy_replaces_the_terraform_files_and_keeps_what_the_module_wrote() {
    let (_root, dir) = project();
    let source = inside_project(&dir, "../../terraform").unwrap();
    let copy = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(copy.path().join(".terraform")).unwrap();
    for (file, text) in [
        ("old.tf", "gone"),
        ("main.tf", "stale"),
        (".terraform.lock.hcl", "lock"),
        ("site.zip", "built"),
        (".terraform/x.tf", "cache"),
    ] {
        std::fs::write(copy.path().join(file), text).unwrap();
    }
    forget_terraform_files(copy.path()).unwrap();
    copy_tree(&source, copy.path()).unwrap();
    let now = files(copy.path());
    assert!(!now.contains(&"old.tf".to_owned()), "a file removed from the module isn't applied: {now:?}");
    assert_eq!(std::fs::read_to_string(copy.path().join("main.tf")).unwrap(), "resource {}");
    for kept in [".terraform.lock.hcl", "site.zip", ".terraform/x.tf"] {
        assert!(now.contains(&kept.to_owned()), "{kept}: {now:?}");
    }
}

#[test]
fn a_module_outside_the_project_is_refused() {
    let (root, dir) = project();
    assert!(inside_project(&dir, "../../terraform").is_some());
    assert!(inside_project(&dir, "../../../").is_none(), "above the project");
    assert!(inside_project(&dir, "../../missing").is_none());
    std::fs::write(root.path().join("file.tf"), "").unwrap();
    assert!(inside_project(&dir, "../../file.tf").is_none(), "a file, not a folder");
}

#[test]
fn terraform_files_are_its_own_extensions() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!has_terraform_files(dir.path()));
    std::fs::write(dir.path().join("notes.md"), "").unwrap();
    assert!(!has_terraform_files(dir.path()));
    std::fs::write(dir.path().join("main.tf.json"), "{}").unwrap();
    assert!(has_terraform_files(dir.path()));
}
