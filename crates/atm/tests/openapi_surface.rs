//! Additions-only gate for ATM's published OpenAPI contract.
//!
//! The gate compares the parsed YAML schema, not rendered documentation.
//! Set `ATM_OPENAPI_SURFACE_BLESS=1` only for a reviewed additive update;
//! removals, requiredness changes, and response/error changes still fail.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{Value, json};

const BLESS_ENV: &str = "ATM_OPENAPI_SURFACE_BLESS";
const OPENAPI_HTTP_METHODS: &[&str] = &[
    "get", "post", "put", "patch", "delete", "options", "head", "trace",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crates/atm has a workspace root two directories up")
        .to_path_buf()
}

fn document_path() -> PathBuf {
    workspace_root().join("docs/atm-daemon/openapi.yaml")
}

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/openapi_surface_baseline.json")
}

fn reviewed_removals_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/openapi_surface_reviewed_removals.json")
}

fn reviewed_removals() -> BTreeSet<String> {
    let source =
        std::fs::read_to_string(reviewed_removals_path()).expect("read reviewed OpenAPI removals");
    let document: Value = serde_json::from_str(&source).expect("parse reviewed OpenAPI removals");
    document["removals"]
        .as_array()
        .expect("reviewed OpenAPI removals must be an array")
        .iter()
        .map(|value| {
            value
                .as_str()
                .expect("reviewed OpenAPI removal must be a string")
                .to_owned()
        })
        .collect()
}

fn is_reviewed_breaking(entry: &str, reviewed_removals: &BTreeSet<String>) -> bool {
    entry
        .strip_prefix("removed OpenAPI contract entry ")
        .is_some_and(|path| reviewed_removals.contains(path))
}

fn object(value: &Value) -> &serde_json::Map<String, Value> {
    value.as_object().expect("OpenAPI value must be an object")
}

fn required(value: &Value) -> BTreeSet<String> {
    value
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn schema_reference(value: &Value) -> Option<&str> {
    value.get("$ref").and_then(Value::as_str)
}

fn response_shape(value: &Value) -> Value {
    let error = value
        .get("$ref")
        .and_then(Value::as_str)
        .is_some_and(|reference| reference.ends_with("/AtmError"));
    let schema = value
        .pointer("/content/application~1json/schema/$ref")
        .and_then(Value::as_str)
        .or_else(|| schema_reference(value));
    json!({ "error": error, "schema": schema })
}

fn live_surface() -> Value {
    let source = std::fs::read_to_string(document_path()).expect("read OpenAPI contract");
    let document: Value = serde_yaml::from_str(&source).expect("parse OpenAPI YAML");
    assert_eq!(document["openapi"], "3.1.0", "OpenAPI version changed");

    let mut paths = BTreeMap::new();
    for (path, item) in object(&document["paths"]) {
        let mut methods = BTreeMap::new();
        for method in ["get", "post", "put", "patch", "delete"] {
            let Some(operation) = item.get(method) else {
                continue;
            };
            let responses = object(&operation["responses"])
                .iter()
                .map(|(status, response)| (status.clone(), response_shape(response)))
                .collect::<BTreeMap<_, _>>();
            methods.insert(
                method,
                json!({
                    "operation_id": operation["operationId"],
                    "request_required": operation.pointer("/requestBody/required").and_then(Value::as_bool).unwrap_or(false),
                    "responses": responses,
                }),
            );
        }
        paths.insert(path.clone(), methods);
    }

    let mut schemas = BTreeMap::new();
    for (name, schema) in object(&document["components"]["schemas"]) {
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|properties| properties.keys().cloned().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        schemas.insert(
            name.clone(),
            json!({ "required": required(schema), "properties": properties }),
        );
    }
    json!({ "paths": paths, "schemas": schemas })
}

fn documented_route_surface(document: &Value) -> BTreeSet<(String, String)> {
    let server_base = document
        .pointer("/servers/0/url")
        .and_then(Value::as_str)
        .expect("OpenAPI document must declare its first server URL")
        .trim_end_matches('/');
    assert!(
        server_base.starts_with('/'),
        "OpenAPI server URL must be a path for the local daemon contract: {server_base}"
    );

    object(&document["paths"])
        .iter()
        .flat_map(|(path, item)| {
            OPENAPI_HTTP_METHODS.iter().filter_map(move |method| {
                item.get(*method)
                    .map(|_| (method.to_ascii_uppercase(), format!("{server_base}{path}")))
            })
        })
        .collect()
}

#[test]
fn openapi_routes_match_live_router_surface() {
    let source = std::fs::read_to_string(document_path()).expect("read OpenAPI contract");
    let document: Value = serde_yaml::from_str(&source).expect("parse OpenAPI YAML");
    let documented = documented_route_surface(&document);
    let live = atm_core::api::http_route_surface()
        .map(|route| (route.method.to_owned(), route.path_template.to_owned()))
        .collect::<BTreeSet<_>>();

    let missing_from_openapi = live.difference(&documented).collect::<Vec<_>>();
    let undocumented_live_routes = documented.difference(&live).collect::<Vec<_>>();
    assert!(
        missing_from_openapi.is_empty() && undocumented_live_routes.is_empty(),
        "OpenAPI routes must exactly match live routing; live routes missing from OpenAPI: \
         {missing_from_openapi:?}; OpenAPI routes not registered live: \
         {undocumented_live_routes:?}"
    );
}

fn compare_value(
    path: &str,
    baseline: &Value,
    live: &Value,
    breaking: &mut Vec<String>,
    additions: &mut Vec<String>,
) {
    match (baseline, live) {
        (Value::Object(old), Value::Object(new)) => {
            for (key, old_value) in old {
                match new.get(key) {
                    Some(new_value) => compare_value(
                        &format!("{path}/{key}"),
                        old_value,
                        new_value,
                        breaking,
                        additions,
                    ),
                    None => breaking.push(format!("removed OpenAPI contract entry {path}/{key}")),
                }
            }
            for key in new.keys().filter(|key| !old.contains_key(*key)) {
                additions.push(format!("new OpenAPI contract entry {path}/{key}"));
            }
        }
        (Value::Array(old), Value::Array(new)) => {
            for value in old.iter().filter(|value| !new.contains(*value)) {
                breaking.push(format!("removed OpenAPI contract value {path}: {value}"));
            }
            for value in new.iter().filter(|value| !old.contains(*value)) {
                additions.push(format!("new OpenAPI contract value {path}: {value}"));
            }
        }
        _ if baseline != live => breaking.push(format!(
            "changed OpenAPI contract entry {path}: {baseline} -> {live}"
        )),
        _ => {}
    }
}

#[test]
fn openapi_surface_is_additions_only() {
    let live = live_surface();
    let baseline_path = baseline_path();
    let baseline_raw = match std::fs::read_to_string(&baseline_path) {
        Ok(contents) => contents,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && std::env::var_os(BLESS_ENV).is_some() =>
        {
            let encoded = serde_json::to_string_pretty(&live).expect("serialize OpenAPI surface");
            std::fs::write(&baseline_path, format!("{encoded}\n"))
                .expect("write initial OpenAPI surface baseline");
            return;
        }
        Err(error) => panic!(
            "read OpenAPI surface baseline {}: {error}",
            baseline_path.display()
        ),
    };
    let baseline: Value =
        serde_json::from_str(&baseline_raw).expect("parse OpenAPI surface baseline");
    let mut breaking = Vec::new();
    let mut additions = Vec::new();
    compare_value("openapi", &baseline, &live, &mut breaking, &mut additions);
    let reviewed_removals = reviewed_removals();
    let unreviewed_breaking = breaking
        .iter()
        .filter(|entry| !is_reviewed_breaking(entry, &reviewed_removals))
        .collect::<Vec<_>>();
    assert!(
        unreviewed_breaking.is_empty(),
        "OpenAPI baseline update has unreviewed breaking entries:\n{}",
        unreviewed_breaking
            .iter()
            .map(|entry| entry.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    if std::env::var_os(BLESS_ENV).is_some() {
        let encoded = serde_json::to_string_pretty(&live).expect("serialize OpenAPI surface");
        std::fs::write(&baseline_path, format!("{encoded}\n"))
            .expect("write OpenAPI surface baseline");
        return;
    }
    assert!(
        additions.is_empty(),
        "OpenAPI surface has unbaselined additions:\n{}\nSet {BLESS_ENV}=1 for a reviewed additive update.",
        additions.join("\n")
    );
}

#[test]
fn reviewed_removals_allow_only_exact_removed_entries() {
    let reviewed = reviewed_removals();

    assert!(is_reviewed_breaking(
        "removed OpenAPI contract entry openapi/paths//teams",
        &reviewed
    ));
    assert!(!is_reviewed_breaking(
        "removed OpenAPI contract entry openapi/paths//teams/get",
        &reviewed
    ));
    assert!(!is_reviewed_breaking(
        "changed OpenAPI contract entry openapi/paths//teams: old -> new",
        &reviewed
    ));
}
