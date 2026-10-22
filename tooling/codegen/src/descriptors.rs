//! Role: Compile JSON Schema into compact browser validator descriptors.
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub(crate) struct DescriptorCompiler<'a> {
    definitions: &'a Map<String, Value>,
    pub(crate) pool: Vec<Value>,
    interned: BTreeMap<String, usize>,
    named: BTreeMap<String, usize>,
}

impl<'a> DescriptorCompiler<'a> {
    pub(crate) fn new(definitions: &'a Map<String, Value>) -> Self {
        Self {
            definitions,
            pool: Vec::new(),
            interned: BTreeMap::new(),
            named: BTreeMap::new(),
        }
    }

    pub(crate) fn compile_named(&mut self, name: &str, schema: &Value) -> Result<usize, String> {
        if let Some(index) = self.named.get(name) {
            return Ok(*index);
        }
        let index = self.pool.len();
        self.pool.push(Value::Null);
        self.named.insert(name.to_owned(), index);
        let descriptor = self.descriptor(schema)?;
        self.pool[index] = descriptor.clone();
        self.interned
            .entry(serde_json::to_string(&descriptor).expect("descriptors serialize"))
            .or_insert(index);
        Ok(index)
    }

    fn compile(&mut self, schema: &Value) -> Result<usize, String> {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let name = local_ref(reference)
                .map_err(|_| format!("unsupported validator reference: {reference}"))?;
            if let Some(index) = self.named.get(name) {
                return Ok(*index);
            }
            let target = self
                .definitions
                .get(name)
                .ok_or_else(|| format!("missing validator definition: {name}"))?
                .clone();
            return self.compile_named(name, &target);
        }
        let descriptor = self.descriptor(schema)?;
        Ok(self.intern(descriptor))
    }

    fn descriptor(&mut self, schema: &Value) -> Result<Value, String> {
        let object = schema
            .as_object()
            .ok_or_else(|| "boolean validator schemas are unsupported".to_owned())?;
        if let Some(value) = object.get("const") {
            return Ok(json!(["c", value]));
        }
        if let Some(Value::Array(values)) = object.get("enum") {
            let mut descriptor = vec![json!("e")];
            descriptor.extend(values.iter().cloned());
            return Ok(Value::Array(descriptor));
        }
        for key in ["oneOf", "anyOf", "allOf"] {
            if let Some(Value::Array(variants)) = object.get(key) {
                let opcode = if key == "allOf" { "l" } else { "u" };
                let mut descriptor = vec![json!(opcode)];
                for variant in variants {
                    descriptor.push(json!(self.compile(variant)?));
                }
                return Ok(Value::Array(descriptor));
            }
        }
        if let Some(Value::Array(types)) = object.get("type") {
            let mut descriptor = vec![json!("u")];
            for kind in types.iter().filter_map(Value::as_str) {
                let mut variant = object.clone();
                variant.insert("type".to_owned(), json!(kind));
                descriptor.push(json!(self.compile(&Value::Object(variant))?));
            }
            return Ok(Value::Array(descriptor));
        }
        match object.get("type").and_then(Value::as_str) {
            Some("null") => Ok(json!("z")),
            Some("boolean") => Ok(json!("b")),
            Some("integer") => Ok(constrained_descriptor("i", object, ["minimum", "maximum"])),
            Some("number") => Ok(constrained_descriptor("n", object, ["minimum", "maximum"])),
            Some("string") => Ok(constrained_descriptor(
                "s",
                object,
                ["pattern", "minLength", "maxLength"],
            )),
            Some("array") => {
                let item = object
                    .get("items")
                    .map(|schema| self.compile(schema))
                    .transpose()?
                    .unwrap_or_else(|| self.intern(json!("v")));
                Ok(with_optional_tail(
                    vec![json!("a"), json!(item)],
                    [
                        object.get("minItems").cloned(),
                        object.get("maxItems").cloned(),
                        object.get("uniqueItems").cloned(),
                    ],
                ))
            }
            Some("object") | None
                if object.contains_key("properties")
                    || object.contains_key("additionalProperties") =>
            {
                self.object_descriptor(object)
            }
            Some(_) | None => Ok(json!("v")),
        }
    }

    fn object_descriptor(&mut self, object: &Map<String, Value>) -> Result<Value, String> {
        let required = required_properties(object);
        let mut required_fields = Map::new();
        let mut optional_fields = Map::new();
        if let Some(properties) = object.get("properties").and_then(Value::as_object) {
            for (name, schema) in properties {
                let target = if required.contains(name.as_str()) {
                    &mut required_fields
                } else {
                    &mut optional_fields
                };
                target.insert(name.clone(), json!(self.compile(schema)?));
            }
        }
        let additional = match object.get("additionalProperties") {
            None | Some(Value::Bool(false)) => None,
            Some(Value::Bool(true)) => Some(self.intern(json!("v"))),
            Some(schema) => Some(self.compile(schema)?),
        };
        let property_names = object
            .get("propertyNames")
            .map(|schema| self.compile(schema))
            .transpose()?;
        Ok(with_optional_tail(
            vec![
                json!("o"),
                Value::Object(required_fields),
                Value::Object(optional_fields),
            ],
            [
                additional.map(|value| json!(value)),
                property_names.map(|value| json!(value)),
            ],
        ))
    }

    fn intern(&mut self, descriptor: Value) -> usize {
        let key = serde_json::to_string(&descriptor).expect("descriptors serialize");
        if let Some(index) = self.interned.get(&key) {
            return *index;
        }
        let index = self.pool.len();
        self.pool.push(descriptor);
        self.interned.insert(key, index);
        index
    }
}

fn with_optional_tail<const N: usize>(
    mut values: Vec<Value>,
    optional: [Option<Value>; N],
) -> Value {
    let mut tail = optional.map(|value| value.unwrap_or(Value::Null)).to_vec();
    while tail.last() == Some(&Value::Null) {
        tail.pop();
    }
    values.extend(tail);
    Value::Array(values)
}

fn constrained_descriptor<const N: usize>(
    opcode: &str,
    schema: &Map<String, Value>,
    constraints: [&str; N],
) -> Value {
    with_optional_tail(
        vec![json!(opcode)],
        constraints.map(|key| schema.get(key).cloned()),
    )
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

fn local_ref(reference: &str) -> Result<&str, String> {
    reference
        .strip_prefix("#/$defs/")
        .ok_or_else(|| format!("unsupported reference: {reference}"))
}
