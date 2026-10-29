//! Role: Generate shared protocol examples from maintained contract templates.
use std::path::{Path, PathBuf};

use serde_json::Value;

pub(super) fn outputs(root: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let mut outputs = Vec::new();
    for name in [
        "contract",
        "optional-present",
        "bidirectional-server",
        "invalid",
        "render-navigation",
    ] {
        let source = root.join(format!("contracts/protocol/fixtures/{name}.json"));
        let text = std::fs::read_to_string(&source).map_err(|error| error.to_string())?;
        let mut value: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        update_versions(&mut value);
        let text = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
        outputs.push((
            root.join(format!("tests/fixtures/protocol/{name}.json")),
            format!("{text}\n"),
        ));
    }
    Ok(outputs)
}

fn update_versions(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                if name == "schemaVersion"
                    || (name == "protocolVersion" && value.as_u64().is_some_and(|n| n > 0))
                {
                    *value = fleximark_protocol::PROTOCOL_VERSION.into();
                } else {
                    update_versions(value);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(update_versions),
        _ => {}
    }
}
