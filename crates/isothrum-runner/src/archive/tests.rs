use std::io::Read;

use flate2::read::GzDecoder;

use super::*;

/// Every entry of the archive at `path`: its path, and whether it is a link.
fn listed(path: &Path) -> Vec<(String, bool)> {
    let mut tar = tar::Archive::new(GzDecoder::new(File::open(path).unwrap()));
    tar.entries()
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            let link = e.header().entry_type().is_symlink();
            (e.path().unwrap().to_string_lossy().trim_end_matches('/').to_owned(), link)
        })
        .collect()
}

fn file(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn version_control_the_tools_state_and_earlier_archives_stay_out() {
    let none = Rules::default();
    let out = Visit { add: false, descend: false };
    for (rel, folder) in [
        (".git", true),
        ("src/.git", true),
        ("a/b/.vagrant", true),
        (".terraform", true),
        (".isoloom-project-env.tgz", false),
    ] {
        assert_eq!(visit(rel, folder, &none), out, "{rel}");
    }
    // Isoloom's outputs go in, and only the top's earlier archives are Isoloom's.
    assert_eq!(visit(".isoloom", true, &none), Visit { add: true, descend: true });
    assert_eq!(visit("docs/.isoloom-project", false, &none), Visit { add: true, descend: false });
}

#[test]
fn an_ignored_folder_is_walked_only_when_a_rule_may_bring_something_back() {
    let plain = Rules::parse("build/\n");
    assert_eq!(visit("build", true, &plain), Visit { add: false, descend: false });
    let back = Rules::parse("build/\n!build/keep.txt\n");
    assert_eq!(visit("build", true, &back), Visit { add: false, descend: true });
    assert!(visit("build/keep.txt", false, &back).add);
    assert!(!visit("build/out.o", false, &back).add);
}

#[test]
fn the_archive_holds_the_project_as_the_generated_files_copy_it() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    file(root, "isoloom.yml", "version: 1\n");
    file(root, ".isoloom/docker/compose.yml", "name: x\n");
    file(root, "build/web/index.html", "hello\n");
    file(root, "notes.log", "noise\n");
    file(root, ".git/config", "[core]\n");
    file(root, "src/.vagrant/key", "secret\n");
    file(root, ".isoloom-project-env.tgz", "old\n");
    file(root, ".isoloomignore", "*.log\n");
    #[cfg(unix)]
    std::os::unix::fs::symlink("build", root.join("latest")).unwrap();
    let state = tempfile::tempdir().unwrap();
    let out = state.path().join(ARCHIVE);
    write(root, &out).unwrap();
    let names: Vec<String> = listed(&out).into_iter().map(|(n, _)| n).collect();
    let mut expected = vec![".isoloom", ".isoloomignore", "build", "isoloom.yml"];
    if cfg!(unix) {
        expected.push("latest");
    }
    expected.extend(["src", ".isoloom/docker", ".isoloom/docker/compose.yml", "build/web", "build/web/index.html"]);
    assert_eq!(names, expected);
    #[cfg(unix)]
    assert!(listed(&out).iter().any(|(n, link)| n == "latest" && *link), "a link goes in as a link");
    // The file reads back whole, and the archive is the run's alone.
    let mut tar = tar::Archive::new(GzDecoder::new(File::open(&out).unwrap()));
    let mut page = String::new();
    for e in tar.entries().unwrap() {
        let mut e = e.unwrap();
        if e.path().unwrap().ends_with("index.html") {
            e.read_to_string(&mut page).unwrap();
        }
    }
    assert_eq!(page, "hello\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&out).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert!(!out.with_file_name(format!("{ARCHIVE}.part")).exists());
}

#[test]
fn a_missing_project_is_an_error_and_leaves_no_partial_archive() {
    let state = tempfile::tempdir().unwrap();
    let out = state.path().join(ARCHIVE);
    assert!(write(&state.path().join("no-such-project"), &out).is_err());
    assert!(!out.exists() && !out.with_file_name(format!("{ARCHIVE}.part")).exists());
}
