//! Role: Generate runtime schemas and browser constants from distribution policy.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

pub(super) fn outputs(root: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let text = std::fs::read_to_string(root.join("contracts/distribution/distribution.json"))
        .map_err(|error| error.to_string())?;
    let spec: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let targets = spec["targets"]
        .as_array()
        .ok_or("distribution target table missing")?;
    let files = spec["files"]
        .as_array()
        .ok_or("distribution payload table missing")?;
    let mut definitions = BTreeMap::new();
    definitions.insert(
        "sha256".to_owned(),
        json!({"type":"string","pattern":"^[0-9a-f]{64}$"}),
    );
    let mut choices = Vec::new();
    let mut platforms = BTreeMap::new();
    let mut triples = Vec::new();
    for target in targets {
        let triple = target["triple"]
            .as_str()
            .ok_or("distribution triple missing")?;
        let suffix = target["executableSuffix"]
            .as_str()
            .ok_or("executable suffix missing")?;
        let mut properties = BTreeMap::new();
        let mut required = Vec::new();
        for file in files {
            let path = file["path"]
                .as_str()
                .ok_or("distribution payload path missing")?
                .replace("{exe}", suffix);
            properties.insert(path.clone(), json!({"$ref":"#/$defs/sha256"}));
            required.push(path);
        }
        let name = format!("files-{triple}");
        definitions.insert(name.clone(), json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}));
        choices.push(json!({"properties":{"target":{"const":triple},"files":{"$ref":format!("#/$defs/{name}")}}}));
        let platform = target["vscode"].as_str().ok_or("VS Code target missing")?;
        platforms.insert(platform, triple);
        triples.push(triple);
    }
    let schema = json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "$id":"https://fleximark.dev/schemas/runtime-manifest.json",
        "title":"FlexiMark runtime manifest",
        "type":"object",
        "properties":{
            "schemaVersion":{"const":1},
            "protocolVersion":{"const":fleximark_protocol::PROTOCOL_VERSION},
            "version":{"type":"string","pattern":"^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\\+[0-9A-Za-z.-]+)?$"},
            "target":{"enum":triples},
            "files":{"type":"object"}
        },
        "required":["schemaVersion","protocolVersion","version","target","files"],
        "additionalProperties":false,
        "$defs":definitions,
        "oneOf":choices
    });
    let constants = [
        ("DISTRIBUTION_TARGETS", &spec["targets"]),
        ("DISTRIBUTION_FILES", &spec["files"]),
        ("DISTRIBUTION_LIMITS", &spec["limits"]),
        ("RUNTIME_MANIFEST_SCHEMA", &schema),
    ];
    let mut ts = String::from("// Generated from distribution.json; DO NOT EDIT.\n\n");
    let mut js = ts.clone();
    let platforms = serde_json::to_string(&platforms).map_err(|error| error.to_string())?;
    ts.push_str(&format!(
        "export const RUNTIME_TARGETS = {platforms} as const;\n\n"
    ));
    js.push_str(&format!("export const RUNTIME_TARGETS = {platforms};\n\n"));
    for (name, value) in constants {
        let value = serde_json::to_string(value).map_err(|error| error.to_string())?;
        ts.push_str(&format!("export const {name} = {value} as const;\n\n"));
        js.push_str(&format!("export const {name} = {value};\n\n"));
    }
    let schema = serde_json::to_string_pretty(&schema).map_err(|error| error.to_string())?;
    Ok(vec![
        (
            root.join("contracts/distribution/runtime-manifest.schema.json"),
            format!("{schema}\n"),
        ),
        (
            root.join("contracts/typescript/distribution.generated.mts"),
            ts,
        ),
        (
            root.join("contracts/distribution/distribution.generated.mjs"),
            js,
        ),
    ])
}
