//! Values delivered as a file in the run's state folder (`state_file`): a Terraform variable
//! file (`tfvars_json`, one JSON object) or `NAME=value` lines (`dotenv`), owner-only, every
//! param and slot naming the file merged into it. A start writes them; any other operation
//! writes one only when it is missing, so a destroy reads the values its apply had; a successful
//! `down` removes them.

use std::collections::BTreeMap;

use isoloom_manifest::{Delivery, StateFileFormat};

use super::Cx;
use super::delivery::valued;
use crate::Result;
use crate::error::Error;

/// One file: its format and its entries, by key.
struct File {
    format: StateFileFormat,
    entries: BTreeMap<String, String>,
}

/// The state files `cx`'s operation has values for, by path in the run's state folder.
fn files(cx: &Cx<'_>) -> BTreeMap<String, File> {
    let mut files: BTreeMap<String, File> = BTreeMap::new();
    for (name, value, deliveries) in valued(cx) {
        for d in deliveries {
            if let Delivery::StateFile { path, format, key } = d {
                let file = files.entry(path.as_str().to_owned()).or_insert_with(|| File {
                    format: *format,
                    entries: BTreeMap::new(),
                });
                file.entries.insert(key.clone().unwrap_or_else(|| name.to_owned()), value.clone());
            }
        }
    }
    files
}

/// Writes the state files of `cx`'s operation: every one on a start (`fresh`), else only those
/// missing.
pub(crate) fn write(cx: &Cx<'_>, fresh: bool) -> Result<()> {
    let dir = cx.run.runner.store.run_dir(cx.run.name())?;
    for (path, file) in files(cx) {
        let target = dir.join(&path);
        if !fresh && target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            crate::store::private_dir(parent).map_err(Error::io(parent, "created"))?;
        }
        let text = match file.format {
            StateFileFormat::TfvarsJson => tfvars_json(&file.entries),
            StateFileFormat::Dotenv => dotenv(check_dotenv_keys(&file.entries)?),
        };
        crate::store::write_private(&target, text.as_bytes()).map_err(Error::io(&target, "written"))?;
    }
    Ok(())
}

/// Removes every state file the manifest names, after a successful `down`.
pub(crate) fn forget(cx: &Cx<'_>) -> Result<()> {
    let dir = cx.run.runner.store.run_dir(cx.run.name())?;
    for d in super::every_delivery(cx.run.manifest()) {
        if let Delivery::StateFile { path, .. } = d {
            let target = dir.join(path.as_str());
            match std::fs::remove_file(&target) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(Error::io(&target, "removed")(e)),
                _ => {}
            }
        }
    }
    Ok(())
}

/// The entries as one JSON object, a string each, in key order.
fn tfvars_json(entries: &BTreeMap<String, String>) -> String {
    let object: serde_json::Map<String, serde_json::Value> = entries.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect();
    serde_json::to_string_pretty(&serde_json::Value::Object(object)).unwrap_or_default() + "\n"
}

/// `entries` back, once every key is one a dotenv file can hold: a key with a line break, a NUL
/// or `=` would start a new line or split one, corrupting the owner-only file (the value is
/// always escaped, the key can't be). A manifest-authored key, so a sound manifest never trips
/// this; the runner refuses it rather than write a broken file.
///
/// # Errors
///
/// [`Error::Unsupported`] when a key holds a line break, a NUL or `=`.
fn check_dotenv_keys(entries: &BTreeMap<String, String>) -> Result<&BTreeMap<String, String>> {
    match entries.keys().find(|k| k.is_empty() || k.contains(['\n', '\r', '\0', '='])) {
        Some(bad) => Err(Error::Unsupported {
            what: format!(
                "a dotenv state-file key that is empty or holds a line break, NUL or `=` (`{}`)",
                bad.escape_debug()
            ),
        }),
        None => Ok(entries),
    }
}

/// The entries as `NAME=value` lines: a value of plain characters as it is, any other in double
/// quotes with `\`, `"`, `$` and the line breaks escaped.
fn dotenv(entries: &BTreeMap<String, String>) -> String {
    entries.iter().fold(String::new(), |mut out, (k, v)| {
        out.push_str(k);
        out.push('=');
        out.push_str(&dotenv_value(v));
        out.push('\n');
        out
    })
}

/// `v` as a dotenv value.
pub(crate) fn dotenv_value(v: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_-.,/:@+%".contains(c);
    if v.chars().all(plain) {
        return v.to_owned();
    }
    let mut out = String::from("\"");
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// A dotenv value read back as a dotenv reader does (double quotes: `\` escapes).
    fn read_back(v: &str) -> String {
        let Some(inner) = v.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
            return v.to_owned();
        };
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match (c, c == '\\') {
                (_, true) => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some(other) => out.push(other),
                    None => {}
                },
                (c, false) => out.push(c),
            }
        }
        out
    }

    #[test]
    fn a_tfvars_file_is_one_json_object_of_strings_in_key_order() {
        let entries = BTreeMap::from([("region".to_owned(), "eu-west-3".to_owned()), ("flag".to_owned(), "a\"b".to_owned())]);
        assert_eq!(tfvars_json(&entries), "{\n  \"flag\": \"a\\\"b\",\n  \"region\": \"eu-west-3\"\n}\n");
    }

    #[test]
    fn a_dotenv_key_that_would_corrupt_the_file_is_refused() {
        let ok = BTreeMap::from([("TF_VAR_region".to_owned(), "eu".to_owned())]);
        assert!(check_dotenv_keys(&ok).is_ok());
        for bad in ["A=B", "A\nB", "A\rB", "A\0B", ""] {
            let entries = BTreeMap::from([(bad.to_owned(), "v".to_owned())]);
            let err = check_dotenv_keys(&entries).expect_err(bad);
            assert_eq!(err.code(), crate::error::ErrorCode::Unsupported, "{bad:?}");
        }
    }

    #[test]
    fn a_dotenv_line_quotes_only_what_needs_it() {
        let entries = BTreeMap::from([("A".to_owned(), "eu-west-3".to_owned()), ("B".to_owned(), "it's $HOME\n".to_owned())]);
        assert_eq!(dotenv(&entries), "A=eu-west-3\nB=\"it's \\$HOME\\n\"\n");
    }

    proptest! {
        #[test]
        fn any_dotenv_value_is_one_line_that_reads_back_as_itself(v in "\\PC{0,30}|[\\n\\r\"$\\\\ a-z]{0,20}") {
            let written = dotenv_value(&v);
            prop_assert!(!written.contains(['\n', '\r']));
            prop_assert_eq!(read_back(&written), v);
        }
    }
}
