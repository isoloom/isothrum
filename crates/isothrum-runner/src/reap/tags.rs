//! What a cloud still holds tagged with an expiry, read from its own command line's JSON: the
//! tags Isoloom's modules put on every resource (`isoloom-expires-at = <unix seconds>` and
//! `isoloom-instance = <the environment's name>` as key-value tags; on the clouds whose tags are
//! words, `isoloom-expires-<unix seconds>` and the environment's name). Each cloud's query is a
//! read-only argv; its answer is parsed by a pure function.

use isoloom_manifest::Cloud;
use serde_json::Value;

use crate::process::ProcessSpec;

/// The key-value tag holding a resource's expiry.
pub(crate) const EXPIRES_KEY: &str = "isoloom-expires-at";
/// The key-value tag holding a resource's environment.
pub(crate) const INSTANCE_KEY: &str = "isoloom-instance";
/// The word tag's prefix holding a resource's expiry (DigitalOcean, Linode).
pub(crate) const EXPIRES_WORD: &str = "isoloom-expires-";

/// A resource a cloud holds with Isoloom's tags.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[non_exhaustive]
pub struct Tagged {
    /// Its id as the cloud names it (an ARN, a resource id, a droplet's number).
    pub id: String,
    /// What it is (`ec2 instance`, `Microsoft.Compute/virtualMachines`, `droplet`).
    pub kind: String,
    /// When its environment ends, in seconds since the Unix epoch, when it says.
    pub expires_at: Option<u64>,
    /// Its environment's name (`isoloom-solo-web-ab12cd34`), when it says.
    pub instance: Option<String>,
}

/// Where a query looks: the regions (AWS) or the project (Google Cloud); the CLI's own default
/// when unset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Place {
    /// AWS regions, each queried.
    pub regions: Vec<String>,
    /// The Google Cloud project.
    pub project: Option<String>,
}

impl Place {
    /// AWS `regions` (each queried) and the Google Cloud `project`.
    #[must_use]
    pub fn new(regions: Vec<String>, project: Option<String>) -> Place {
        Place { regions, project }
    }
}

/// The read-only queries listing `cloud`'s tagged resources in `place`: one per AWS region, one
/// otherwise.
pub(crate) fn queries(cloud: Cloud, place: &Place) -> Vec<ProcessSpec> {
    match cloud {
        Cloud::Aws => {
            let base = [
                "resourcegroupstaggingapi",
                "get-resources",
                "--tag-filters",
                "Key=isoloom-expires-at",
                "--output",
                "json",
            ];
            let one = |region: Option<&str>| {
                let tail = region.map(|r| vec!["--region".to_owned(), r.to_owned()]).unwrap_or_default();
                ProcessSpec::new("aws", base.iter().map(|s| (*s).to_owned()).chain(tail))
            };
            if place.regions.is_empty() {
                vec![one(None)]
            } else {
                place.regions.iter().map(|r| one(Some(r))).collect()
            }
        }
        Cloud::Azure => vec![ProcessSpec::new("az", ["resource", "list", "--tag", EXPIRES_KEY, "--output", "json"])],
        Cloud::Gcp => {
            let mut args: Vec<String> = ["compute", "instances", "list", "--filter=labels.isoloom-expires-at:*", "--format=json"]
                .map(str::to_owned)
                .to_vec();
            if let Some(p) = &place.project {
                args.push(format!("--project={p}"));
            }
            vec![ProcessSpec::new("gcloud", args)]
        }
        Cloud::Digitalocean => vec![ProcessSpec::new("doctl", ["compute", "droplet", "list", "--output", "json"])],
        Cloud::Linode => vec![ProcessSpec::new("linode-cli", ["linodes", "list", "--json"])],
        Cloud::Oci => vec![ProcessSpec::new(
            "oci",
            [
                "search",
                "resource",
                "structured-search",
                "--query-text",
                "query all resources where (freeformTags.key = 'isoloom-expires-at')",
                "--output",
                "json",
            ],
        )],
    }
}

/// The tagged resources in `cloud`'s answer `json` to its query (those without Isoloom's expiry
/// tag left out on the clouds whose query can't filter by it).
///
/// # Errors
///
/// The answer isn't JSON, or isn't the shape the cloud's command line gives.
pub(crate) fn parse(cloud: Cloud, json: &str) -> Result<Vec<Tagged>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let shape = || format!("not the shape {} gives", cloud_cli(cloud));
    let items: Vec<Tagged> = match cloud {
        Cloud::Aws => list(v.get("ResourceTagMappingList")).ok_or_else(shape)?.iter().filter_map(aws).collect(),
        Cloud::Azure => list(Some(&v)).ok_or_else(shape)?.iter().filter_map(azure).collect(),
        Cloud::Gcp => list(Some(&v)).ok_or_else(shape)?.iter().filter_map(gcp).collect(),
        Cloud::Digitalocean => list(Some(&v)).ok_or_else(shape)?.iter().filter_map(|d| words(d, "id", "droplet")).collect(),
        Cloud::Linode => list(Some(&v)).ok_or_else(shape)?.iter().filter_map(|l| words(l, "id", "linode")).collect(),
        Cloud::Oci => list(v.get("data").and_then(|d| d.get("items")))
            .ok_or_else(shape)?
            .iter()
            .filter_map(oci)
            .collect(),
    };
    Ok(items)
}

/// The command line `cloud`'s query runs.
pub(crate) fn cloud_cli(cloud: Cloud) -> &'static str {
    match cloud {
        Cloud::Aws => "aws",
        Cloud::Azure => "az",
        Cloud::Gcp => "gcloud",
        Cloud::Digitalocean => "doctl",
        Cloud::Linode => "linode-cli",
        Cloud::Oci => "oci",
    }
}

fn list(v: Option<&Value>) -> Option<&Vec<Value>> {
    v?.as_array()
}

fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A resource from its id, kind and key-value tags (an object of strings).
fn keyed(id: Option<String>, kind: String, tags: Option<&Value>) -> Option<Tagged> {
    let tags = tags?.as_object()?;
    Some(Tagged {
        id: id?,
        kind,
        expires_at: text(tags.get(EXPIRES_KEY)).and_then(|t| t.trim().parse().ok()),
        instance: text(tags.get(INSTANCE_KEY)),
    })
}

/// An AWS resource: its ARN, its service and type (`ec2 instance`), its `Tags` list.
fn aws(r: &Value) -> Option<Tagged> {
    let arn = text(r.get("ResourceARN"))?;
    let tags: serde_json::Map<String, Value> = list(r.get("Tags"))?
        .iter()
        .filter_map(|t| Some((text(t.get("Key"))?, t.get("Value")?.clone())))
        .collect();
    let mut parts = arn.split(':');
    let service = parts.nth(2).unwrap_or_default().to_owned();
    let kind = parts.nth(2).and_then(|resource| resource.split(['/', ':']).next()).unwrap_or_default();
    keyed(Some(arn.clone()), format!("{service} {kind}").trim().to_owned(), Some(&Value::Object(tags)))
}

fn azure(r: &Value) -> Option<Tagged> {
    keyed(text(r.get("id")), text(r.get("type")).unwrap_or_default(), r.get("tags"))
}

fn gcp(r: &Value) -> Option<Tagged> {
    let id = text(r.get("selfLink")).or_else(|| text(r.get("name")));
    keyed(id, "compute instance".into(), r.get("labels"))
}

fn oci(r: &Value) -> Option<Tagged> {
    keyed(
        text(r.get("identifier")),
        text(r.get("resource-type")).unwrap_or_default(),
        r.get("freeform-tags"),
    )
}

/// A resource whose tags are words (DigitalOcean, Linode): its expiry from
/// `isoloom-expires-<time>`, its environment the other `isoloom-…` word; none without the
/// expiry word.
fn words(r: &Value, id: &str, kind: &str) -> Option<Tagged> {
    let tags: Vec<String> = list(r.get("tags"))?.iter().filter_map(|t| text(Some(t))).collect();
    let expires = tags.iter().find_map(|t| t.strip_prefix(EXPIRES_WORD))?;
    Some(Tagged {
        id: text(r.get(id))?,
        kind: kind.to_owned(),
        expires_at: expires.parse().ok(),
        instance: tags.iter().find(|t| t.starts_with("isoloom-") && !t.starts_with(EXPIRES_WORD)).cloned(),
    })
}

/// The environments a Terraform state's resources carry (their `isoloom-instance` tag, or the
/// `isoloom-…` word beside an expiry word): what a record covers.
pub(crate) fn instances_in_state(state: &Value, out: &mut std::collections::BTreeSet<String>) {
    match state {
        Value::Object(o) => {
            if let Some(Value::String(i)) = o.get(INSTANCE_KEY) {
                out.insert(i.clone());
            }
            for v in o.values() {
                instances_in_state(v, out);
            }
        }
        Value::Array(a) => {
            let words: Vec<&str> = a.iter().filter_map(Value::as_str).collect();
            if words.iter().any(|w| w.starts_with(EXPIRES_WORD)) {
                out.extend(
                    words
                        .iter()
                        .filter(|w| w.starts_with("isoloom-") && !w.starts_with(EXPIRES_WORD))
                        .map(|w| (*w).to_owned()),
                );
            }
            for v in a {
                instances_in_state(v, out);
            }
        }
        _ => {}
    }
}
