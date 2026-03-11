use schemars::{schema_for, JsonSchema};
use serde::de::DeserializeOwned;

/// Trait for types that can be used as OpenAI structured output.
///
/// Automatically implemented for any type that implements `JsonSchema + DeserializeOwned`.
pub trait StructuredOutput: JsonSchema + DeserializeOwned {
    /// Generate an OpenAI-compatible JSON schema for this type.
    ///
    /// OpenAI requires:
    /// 1. `additionalProperties: false` on all object schemas
    /// 2. ALL properties listed in `required`, even nullable ones
    /// 3. Fully inlined schemas (no `$ref` references)
    fn openai_schema() -> serde_json::Value {
        let schema = schema_for!(Self);
        let mut value = serde_json::to_value(schema).unwrap_or_default();

        fix_object_schemas(&mut value);
        inline_refs(&mut value);
        flatten_oneof_string_enums(&mut value);

        if let serde_json::Value::Object(map) = &mut value {
            map.remove("definitions");
            map.remove("$schema");
        }

        value
    }

    fn type_name() -> String {
        <Self as JsonSchema>::schema_name()
    }
}

impl<T: JsonSchema + DeserializeOwned> StructuredOutput for T {}

/// Prepare a raw schemars JSON schema for OpenAI strict mode.
///
/// Adds `additionalProperties: false`, makes all properties required,
/// inlines `$ref` definitions, and strips `$schema`/`definitions`.
pub fn prepare_strict_schema(schema: serde_json::Value) -> serde_json::Value {
    let mut value = schema;
    fix_object_schemas(&mut value);
    inline_refs(&mut value);
    flatten_oneof_string_enums(&mut value);

    if let serde_json::Value::Object(map) = &mut value {
        map.remove("definitions");
        map.remove("$schema");
    }

    value
}

fn fix_object_schemas(value: &mut serde_json::Value) {
    if let serde_json::Value::Object(map) = value {
        if map.get("type") == Some(&serde_json::Value::String("object".to_string())) {
            map.insert(
                "additionalProperties".to_string(),
                serde_json::Value::Bool(false),
            );

            if let Some(serde_json::Value::Object(props)) = map.get("properties") {
                let all_keys: Vec<serde_json::Value> = props
                    .keys()
                    .map(|k| serde_json::Value::String(k.clone()))
                    .collect();
                map.insert("required".to_string(), serde_json::Value::Array(all_keys));
            }
        }

        for (_, v) in map.iter_mut() {
            fix_object_schemas(v);
        }
    } else if let serde_json::Value::Array(arr) = value {
        for item in arr.iter_mut() {
            fix_object_schemas(item);
        }
    }
}

fn inline_refs(value: &mut serde_json::Value) {
    let definitions = if let serde_json::Value::Object(map) = value {
        map.get("definitions").cloned()
    } else {
        None
    };

    if let Some(defs) = definitions {
        inline_refs_recursive(value, &defs);
    }
}

fn inline_refs_recursive(value: &mut serde_json::Value, definitions: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(ref_path)) = map.get("$ref").cloned() {
                if ref_path.starts_with("#/definitions/") {
                    let type_name = ref_path.trim_start_matches("#/definitions/");
                    if let Some(def) = definitions.get(type_name) {
                        *value = def.clone();
                        inline_refs_recursive(value, definitions);
                        return;
                    }
                }
            }

            if let Some(serde_json::Value::Array(all_of)) = map.get("allOf").cloned() {
                if all_of.len() == 1 {
                    *value = all_of.into_iter().next().unwrap();
                    inline_refs_recursive(value, definitions);
                    return;
                }
            }

            for (_, v) in map.iter_mut() {
                inline_refs_recursive(v, definitions);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                inline_refs_recursive(item, definitions);
            }
        }
        _ => {}
    }
}

/// Converts `oneOf` string enum variants (schemars output for documented enums)
/// into flat `{"type": "string", "enum": [...]}` that OpenAI accepts.
///
/// Handles both `{"const": "val"}` and `{"type": "string", "enum": ["val"]}`
/// variant shapes — schemars emits the latter for documented enum variants.
fn flatten_oneof_string_enums(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Array(variants)) = map.get("oneOf") {
                let values: Option<Vec<serde_json::Value>> = variants
                    .iter()
                    .map(|v| {
                        if let Some(c) = v.get("const") {
                            return Some(c.clone());
                        }
                        if let Some(serde_json::Value::Array(arr)) = v.get("enum") {
                            if arr.len() == 1 {
                                return Some(arr[0].clone());
                            }
                        }
                        None
                    })
                    .collect();

                if let Some(vals) = values {
                    if !vals.is_empty() {
                        map.remove("oneOf");
                        map.insert("type".to_string(), serde_json::Value::String("string".to_string()));
                        map.insert("enum".to_string(), serde_json::Value::Array(vals));
                        return;
                    }
                }
            }

            for (_, v) in map.iter_mut() {
                flatten_oneof_string_enums(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                flatten_oneof_string_enums(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::Deserialize;

    #[derive(Deserialize, JsonSchema)]
    struct TestPost {
        title: String,
        description: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    struct TestResponse {
        posts: Vec<TestPost>,
    }

    #[test]
    fn test_openai_schema_generation() {
        let schema = TestResponse::openai_schema();
        assert!(schema.is_object());
    }

    #[test]
    fn test_additional_properties_false() {
        let schema = TestResponse::openai_schema();
        let schema_str = serde_json::to_string(&schema).unwrap();
        assert!(schema_str.contains("additionalProperties"));
    }

    #[test]
    fn test_all_properties_required() {
        #[derive(Deserialize, JsonSchema)]
        struct Contact {
            phone: Option<String>,
            email: Option<String>,
            name: String,
        }

        let schema = Contact::openai_schema();
        let schema_obj = schema.as_object().unwrap();

        assert!(!schema_obj.contains_key("definitions"));

        let required = schema_obj
            .get("required")
            .expect("should have required array")
            .as_array()
            .unwrap();
        let required_strs: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();

        assert!(required_strs.contains(&"phone"));
        assert!(required_strs.contains(&"email"));
        assert!(required_strs.contains(&"name"));
    }

    #[test]
    fn documented_enum_flattened_to_string_enum() {
        #[derive(Deserialize, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        enum Color {
            /// A warm color.
            Red,
            /// A cool color.
            Blue,
            /// A neutral color.
            Green,
        }

        #[derive(Deserialize, JsonSchema)]
        struct Palette {
            primary: Color,
        }

        let schema = Palette::openai_schema();
        let props = schema.get("properties").unwrap().as_object().unwrap();
        let primary = props.get("primary").unwrap().as_object().unwrap();

        assert!(!primary.contains_key("oneOf"), "oneOf should be flattened");
        assert_eq!(
            primary.get("type"),
            Some(&serde_json::Value::String("string".to_string()))
        );
        let variants = primary.get("enum").unwrap().as_array().unwrap();
        let strs: Vec<&str> = variants.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(strs, vec!["red", "blue", "green"]);
    }

    #[test]
    fn test_nested_struct_inlined() {
        #[derive(Deserialize, JsonSchema)]
        struct ContactInfo {
            phone: Option<String>,
            email: Option<String>,
        }

        #[derive(Deserialize, JsonSchema)]
        struct ExtractedPost {
            contact: ContactInfo,
            title: String,
        }

        let schema = ExtractedPost::openai_schema();
        let schema_obj = schema.as_object().unwrap();

        assert!(!schema_obj.contains_key("definitions"));
        assert!(!schema_obj.contains_key("$schema"));

        let properties = schema_obj.get("properties").unwrap().as_object().unwrap();
        let contact = properties.get("contact").unwrap().as_object().unwrap();

        assert!(!contact.contains_key("$ref"));
        assert_eq!(
            contact.get("type"),
            Some(&serde_json::Value::String("object".to_string()))
        );
        assert_eq!(
            contact.get("additionalProperties"),
            Some(&serde_json::Value::Bool(false))
        );
    }
}
