//! The values an operation runs with, given on the command line and parsed once: params
//! (`NAME=VALUE`) and the slots' values, read from the environment (`NAME[=VAR]`).

use std::collections::BTreeMap;

use isothrum_runner::{SecretValue, Secrets};

use crate::error::{Error, Result};

/// `NAME=VALUE` pairs, by name.
pub(crate) fn params(pairs: &[String]) -> Result<BTreeMap<String, String>> {
    pairs
        .iter()
        .map(|p| {
            p.split_once('=')
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .ok_or_else(|| Error::Usage(format!("`--param {p}`: write NAME=VALUE")))
        })
        .collect()
}

/// The slots' values, each `NAME[=VAR]` read from variable `VAR` (default `NAME`) through `var`.
pub(crate) fn secrets(specs: &[String], var: &dyn Fn(&str) -> Option<String>) -> Result<Secrets> {
    let mut out = Secrets::new();
    for spec in specs {
        let (name, from) = spec.split_once('=').unwrap_or((spec, spec));
        let value = var(from).ok_or_else(|| Error::Usage(format!("`--secret-from-env {spec}`: `{from}` isn't set")))?;
        out.insert(name, SecretValue::new(value));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_parsed_once() {
        assert_eq!(
            params(&["A=1".into(), "B=x=y".into()]).unwrap(),
            BTreeMap::from([("A".into(), "1".into()), ("B".into(), "x=y".into())])
        );
        assert!(params(&["A".into()]).is_err());
        let env = |k: &str| (k == "FLAG_VAR").then(|| "flag{x}".to_owned());
        let s = secrets(&["CTF_FLAG_MAIN=FLAG_VAR".into()], &env).unwrap();
        assert_eq!(s.get("CTF_FLAG_MAIN").map(SecretValue::expose), Some("flag{x}"));
        assert!(
            secrets(&["CTF_FLAG_MAIN".into()], &env)
                .unwrap_err()
                .to_string()
                .contains("`CTF_FLAG_MAIN` isn't set")
        );
    }

    #[test]
    fn a_slot_named_alone_is_read_from_its_own_variable() {
        let env = |k: &str| (k == "FLAG_VAR").then(|| "flag{y}".to_owned());
        let s = secrets(&["FLAG_VAR".into()], &env).unwrap();
        assert_eq!(s.get("FLAG_VAR").map(SecretValue::expose), Some("flag{y}"));
    }

    proptest::proptest! {
        #[test]
        fn a_param_is_its_name_up_to_the_first_equals_sign_and_the_rest_its_value(name in "[A-Za-z_][A-Za-z0-9_]{0,12}", value in ".{0,20}") {
            let parsed = params(&[format!("{name}={value}")]).unwrap();
            proptest::prop_assert_eq!(parsed, BTreeMap::from([(name, value)]));
        }
    }
}
