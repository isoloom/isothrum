//! The spec as the `isoloom.yml` a person would write.

use isoloom_core::Spec;

use crate::CompileError;

/// The spec as YAML, ready to write to `isoloom.yml`: only what differs from Isoloom's defaults,
/// as a person would write it.
pub fn to_yaml(spec: &Spec) -> Result<String, CompileError> {
    let mut value = serde_yaml_ng::to_value(spec).map_err(|e| CompileError::Spec(e.to_string()))?;
    strip_defaults(&mut value);
    serde_yaml_ng::to_string(&value).map_err(|e| CompileError::Spec(e.to_string()))
}

/// Fields whose value is Isoloom's default, so leaving them out means the same thing.
/// `internet` is not here: it defaults to true, and every cable sets it to false.
const DEFAULT_FALSE: &[&str] = &["privileged", "read_only", "access", "http", "idle"];

fn strip_defaults(value: &mut serde_yaml_ng::Value) {
    use serde_yaml_ng::Value;
    match value {
        Value::Mapping(map) => {
            map.retain(|k, v| {
                let key = k.as_str().unwrap_or_default();
                !(v.is_null()
                    || v.as_sequence().is_some_and(|s| s.is_empty())
                    || v.as_mapping().is_some_and(|m| m.is_empty())
                    || (DEFAULT_FALSE.contains(&key) && v.as_bool() == Some(false))
                    || (key == "arch" && v.as_str() == Some("amd64")))
            });
            for v in map.values_mut() {
                strip_defaults(v);
            }
        }
        Value::Sequence(seq) => seq.iter_mut().for_each(strip_defaults),
        _ => {}
    }
}
