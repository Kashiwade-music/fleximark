//! Role: Generate versioned schemas for strict workspace configuration.
use serde_json::Value;

pub(super) fn schema() -> Result<String, String> {
    let mut schema: Value = serde_json::from_str(include_str!(
        "../../../contracts/config/schema.template.json"
    ))
    .map_err(|error| error.to_string())?;
    let version = fleximark_config::CONFIG_SCHEMA_VERSION;
    schema["$id"] = format!("https://fleximark.dev/schemas/config-v{version}.json").into();
    schema["title"] = format!("FlexiMark configuration v{version}").into();
    schema["properties"]["schema_version"]["const"] = version.into();
    serde_json::to_string_pretty(&schema)
        .map(|text| format!("{text}\n"))
        .map_err(|error| error.to_string())
}
