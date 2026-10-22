//! Role: Translate normalized schemas into TypeScript type expressions.
use serde_json::{Map, Value};
use std::collections::BTreeSet;
pub(crate) fn schema_to_typescript(schema: &Value) -> Result<String, String> {
    let object = schema
        .as_object()
        .ok_or_else(|| "boolean schemas are unsupported in TypeScript output".to_owned())?;
    if let Some(reference) = object.get("x-typescript-ref").and_then(Value::as_str) {
        return Ok(reference.to_owned());
    }
    if let Some(value) = object.get("const") {
        return Ok(json_literal(value));
    }
    if let Some(Value::Array(values)) = object.get("enum") {
        return Ok(values
            .iter()
            .map(json_literal)
            .collect::<Vec<_>>()
            .join(" | "));
    }
    for (union_key, separator) in [("oneOf", " | "), ("anyOf", " | "), ("allOf", " & ")] {
        if let Some(Value::Array(items)) = object.get(union_key) {
            return items
                .iter()
                .map(schema_to_typescript)
                .collect::<Result<Vec<_>, _>>()
                .map(|items| items.join(separator));
        }
    }
    match object.get("type") {
        Some(Value::Array(types)) => types
            .iter()
            .map(|value| {
                let mut variant = object.clone();
                variant.insert("type".to_owned(), value.clone());
                schema_to_typescript(&Value::Object(variant))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|types| types.join(" | ")),
        Some(Value::String(kind)) if kind == "object" => object_schema_to_typescript(object),
        Some(Value::String(kind)) if kind == "array" => {
            let items = object
                .get("items")
                .map(schema_to_typescript)
                .transpose()?
                .unwrap_or_else(|| "unknown".to_owned());
            Ok(format!("Array<{items}>"))
        }
        Some(Value::String(kind)) => Ok(primitive_ts(kind).to_owned()),
        None if object.contains_key("properties") => object_schema_to_typescript(object),
        None => Ok("unknown".to_owned()),
        _ => Err("invalid schema type".to_owned()),
    }
}

fn object_schema_to_typescript(object: &Map<String, Value>) -> Result<String, String> {
    let required = required_properties(object);
    let mut fields = Vec::new();
    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        for (name, schema) in properties {
            fields.push(format!(
                "{}{}: {}",
                ts_property(name),
                if required.contains(name.as_str()) {
                    ""
                } else {
                    "?"
                },
                schema_to_typescript(schema)?
            ));
        }
    }
    if let Some(additional) = object.get("additionalProperties") {
        if additional != &Value::Bool(false) {
            let value_type = if additional == &Value::Bool(true) {
                "unknown".to_owned()
            } else {
                schema_to_typescript(additional)?
            };
            fields.push(format!("[key: string]: {value_type}"));
        }
    }
    Ok(format!("{{ {} }}", fields.join("; ")))
}

fn required_properties(object: &Map<String, Value>) -> BTreeSet<&str> {
    object
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn primitive_ts(kind: &str) -> &'static str {
    match kind {
        "null" => "null",
        "boolean" => "boolean",
        "integer" | "number" => "number",
        "string" => "string",
        "object" => "Record<string, unknown>",
        "array" => "unknown[]",
        _ => "unknown",
    }
}

fn json_literal(value: &Value) -> String {
    serde_json::to_string(value).expect("JSON literals serialize")
}

fn ts_property(name: &str) -> String {
    if name.chars().enumerate().all(|(index, character)| {
        character == '_'
            || character == '$'
            || (character.is_ascii_alphanumeric() && (index > 0 || !character.is_ascii_digit()))
    }) {
        name.to_owned()
    } else {
        json_literal(&Value::String(name.to_owned()))
    }
}
