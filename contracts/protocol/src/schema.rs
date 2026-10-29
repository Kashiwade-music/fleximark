//! Role: Express optional non-null wire fields in derived contract schemas.
pub(crate) fn non_null(schema: &mut schemars::Schema) {
    if let Some(types) = schema
        .get_mut("type")
        .and_then(serde_json::Value::as_array_mut)
    {
        types.retain(|kind| kind.as_str() != Some("null"));
        if types.len() == 1 {
            let kind = types[0].clone();
            schema.insert("type".into(), kind);
        }
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(variants) = schema
            .get_mut(key)
            .and_then(serde_json::Value::as_array_mut)
        {
            variants.retain(|variant| {
                variant.get("type").and_then(serde_json::Value::as_str) != Some("null")
            });
            if variants.len() == 1 {
                *schema = variants[0].clone().try_into().expect("schema variant");
            }
            break;
        }
    }
}
