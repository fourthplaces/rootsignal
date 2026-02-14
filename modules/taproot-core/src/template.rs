use anyhow::{bail, Context, Result};
use std::collections::HashMap;

/// Resolve `{{config.*}}` variables from the TOML value tree at load time.
/// Returns the template with config vars resolved; runtime vars (non-`config.` prefixed) are left as-is.
pub fn resolve_config_vars(template: &str, toml_value: &toml::Value) -> Result<String> {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            // Escaped \{{ → literal {{
            if chars.peek() == Some(&'{') {
                chars.next(); // consume first {
                if chars.peek() == Some(&'{') {
                    chars.next(); // consume second {
                    result.push_str("{{");
                } else {
                    result.push('\\');
                    result.push('{');
                }
                continue;
            }
            result.push(c);
        } else if c == '{' && chars.peek() == Some(&'{') {
            chars.next(); // consume second {

            // Read until }}
            let mut var_name = String::new();
            loop {
                match chars.next() {
                    Some('}') if chars.peek() == Some(&'}') => {
                        chars.next(); // consume second }
                        break;
                    }
                    Some(ch) => var_name.push(ch),
                    None => bail!("Unclosed template variable: {{{{{}}}", var_name),
                }
            }

            let var_name = var_name.trim();

            if let Some(path) = var_name.strip_prefix("config.") {
                // Resolve from TOML tree
                let value = lookup_toml_path(toml_value, path)
                    .with_context(|| format!("Config variable not found: {{{{{}}}}}", var_name))?;
                result.push_str(&toml_value_to_string(&value));
            } else {
                // Runtime variable — leave as-is
                result.push_str("{{");
                result.push_str(var_name);
                result.push_str("}}");
            }
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

/// Resolve remaining `{{var}}` placeholders from a runtime context map.
pub fn resolve_runtime_vars(template: &str, vars: &HashMap<&str, &str>) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' && chars.peek() == Some(&'{') {
            chars.next(); // consume second {

            let mut var_name = String::new();
            loop {
                match chars.next() {
                    Some('}') if chars.peek() == Some(&'}') => {
                        chars.next();
                        break;
                    }
                    Some(ch) => var_name.push(ch),
                    None => {
                        // Malformed — just emit what we have
                        result.push_str("{{");
                        result.push_str(&var_name);
                        return result;
                    }
                }
            }

            let var_name = var_name.trim();
            if let Some(value) = vars.get(var_name) {
                result.push_str(value);
            } else {
                // Unknown runtime var — leave as-is for debugging
                result.push_str("{{");
                result.push_str(var_name);
                result.push_str("}}");
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Validate that all `{{...}}` in a template are either `config.*` (resolvable) or in an allowed runtime set.
pub fn validate_template(
    template: &str,
    toml_value: &toml::Value,
    allowed_runtime: &[&str],
) -> Result<()> {
    let mut chars = template.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'{') {
            chars.next();
            if chars.peek() == Some(&'{') {
                chars.next();
            }
            continue;
        }

        if c == '{' && chars.peek() == Some(&'{') {
            chars.next();

            let mut var_name = String::new();
            loop {
                match chars.next() {
                    Some('}') if chars.peek() == Some(&'}') => {
                        chars.next();
                        break;
                    }
                    Some(ch) => var_name.push(ch),
                    None => bail!("Unclosed template variable: {{{{{}}}", var_name),
                }
            }

            let var_name = var_name.trim();

            if let Some(path) = var_name.strip_prefix("config.") {
                lookup_toml_path(toml_value, path)
                    .with_context(|| format!("Config variable not found: {{{{{}}}}}", var_name))?;
            } else if !allowed_runtime.contains(&var_name) {
                bail!(
                    "Unknown template variable: {{{{{}}}}}. Allowed runtime vars: {:?}",
                    var_name,
                    allowed_runtime
                );
            }
        }
    }

    Ok(())
}

/// Walk the TOML value tree by dotted path (e.g., "identity.region").
fn lookup_toml_path<'a>(value: &'a toml::Value, path: &str) -> Option<&'a toml::Value> {
    let mut current = value;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    Some(current)
}

/// Convert a TOML value to its string representation for template substitution.
fn toml_value_to_string(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Array(arr) => arr
            .iter()
            .map(|v| toml_value_to_string(v))
            .collect::<Vec<_>>()
            .join(", "),
        toml::Value::Table(_) => "[table]".to_string(),
        toml::Value::Datetime(dt) => dt.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_toml() -> toml::Value {
        toml::from_str(
            r#"
            [identity]
            region = "Twin Cities"
            description = "community signals"

            [models]
            extraction = "gpt-4o"
            "#,
        )
        .unwrap()
    }

    #[test]
    fn resolves_config_vars() {
        let toml = test_toml();
        let result =
            resolve_config_vars("Hello {{config.identity.region}}!", &toml).unwrap();
        assert_eq!(result, "Hello Twin Cities!");
    }

    #[test]
    fn leaves_runtime_vars_intact() {
        let toml = test_toml();
        let result =
            resolve_config_vars("Date: {{today}}, Region: {{config.identity.region}}", &toml)
                .unwrap();
        assert_eq!(result, "Date: {{today}}, Region: Twin Cities");
    }

    #[test]
    fn resolves_runtime_vars() {
        let result = resolve_runtime_vars(
            "Date: {{today}}, Tax: {{taxonomy}}",
            &HashMap::from([("today", "2026-02-14"), ("taxonomy", "categories here")]),
        );
        assert_eq!(result, "Date: 2026-02-14, Tax: categories here");
    }

    #[test]
    fn escapes_literal_braces() {
        let toml = test_toml();
        let result = resolve_config_vars(r#"JSON: \{{"key": "val"}}"#, &toml).unwrap();
        assert_eq!(result, r#"JSON: {{"key": "val"}}"#);
    }

    #[test]
    fn errors_on_missing_config_var() {
        let toml = test_toml();
        let result = resolve_config_vars("{{config.nonexistent.field}}", &toml);
        assert!(result.is_err());
    }

    #[test]
    fn validates_template() {
        let toml = test_toml();
        assert!(validate_template(
            "{{config.identity.region}} {{taxonomy}}",
            &toml,
            &["taxonomy"]
        )
        .is_ok());

        assert!(validate_template("{{unknown_var}}", &toml, &["taxonomy"]).is_err());
    }
}
