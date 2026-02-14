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
