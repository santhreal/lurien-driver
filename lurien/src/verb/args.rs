//! One argument decoder for every face.
//!
//! CLI flags, MCP `arguments`, and HTTP bodies all land here, so a verb sees
//! identical arguments whichever transport delivered them, and an unknown
//! argument fails closed instead of being silently dropped.

use super::{ArgType, VerbSpec};
use crate::error::Error;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// Decoded arguments for one verb call.
#[derive(Debug, Clone, Default)]
pub struct Args {
    map: Map<String, Value>,
}

impl Args {
    /// Empty argument set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode a JSON object. `null` is an empty set; anything else is an error.
    pub fn from_value(value: Value) -> Result<Self, Error> {
        match value {
            Value::Null => Ok(Self::new()),
            Value::Object(map) => Ok(Self { map }),
            other => Err(Error::BadArgs {
                verb: "arguments".into(),
                detail: format!("expected a JSON object, got {other}"),
            }),
        }
    }

    /// Insert one argument. Chainable for face code that builds args by hand.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<Value>) -> &mut Self {
        self.map.insert(key.into(), value.into());
        self
    }

    /// Underlying object.
    #[must_use]
    pub fn as_map(&self) -> &Map<String, Value> {
        &self.map
    }

    /// Consume into a JSON object.
    #[must_use]
    pub fn into_value(self) -> Value {
        Value::Object(self.map)
    }

    /// True when nothing was supplied.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Required string.
    pub fn str(&self, key: &str) -> Result<&str, Error> {
        self.map
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| Error::BadArgs {
                verb: key.to_string(),
                detail: format!("missing required argument {key}"),
            })
    }

    /// Optional string.
    #[must_use]
    pub fn opt_str(&self, key: &str) -> Option<&str> {
        self.map.get(key).and_then(Value::as_str)
    }

    /// Signed integer, or `default`.
    #[must_use]
    pub fn i64(&self, key: &str, default: i64) -> i64 {
        self.map.get(key).and_then(Value::as_i64).unwrap_or(default)
    }

    /// Unsigned integer, or `default`.
    #[must_use]
    pub fn u64(&self, key: &str, default: u64) -> u64 {
        self.map.get(key).and_then(Value::as_u64).unwrap_or(default)
    }

    /// Double, or `default`.
    #[must_use]
    pub fn f64(&self, key: &str, default: f64) -> f64 {
        self.map.get(key).and_then(Value::as_f64).unwrap_or(default)
    }

    /// Boolean, or `default`.
    #[must_use]
    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.map.get(key).and_then(Value::as_bool).unwrap_or(default)
    }

    /// Required path.
    pub fn path(&self, key: &str) -> Result<PathBuf, Error> {
        self.str(key).map(PathBuf::from)
    }

    /// Optional path.
    #[must_use]
    pub fn opt_path(&self, key: &str) -> Option<PathBuf> {
        self.opt_str(key).map(PathBuf::from)
    }

    /// String list. A bare string counts as a one-element list.
    pub fn str_list(&self, key: &str) -> Result<Vec<String>, Error> {
        match self.map.get(key) {
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| {
                    v.as_str().map(str::to_string).ok_or_else(|| Error::BadArgs {
                        verb: key.to_string(),
                        detail: format!("{key} takes strings, got {v}"),
                    })
                })
                .collect(),
            Some(Value::String(s)) => Ok(vec![s.clone()]),
            _ => Err(Error::BadArgs {
                verb: key.to_string(),
                detail: format!("missing required argument {key}"),
            }),
        }
    }

    /// String list that may be absent, which reads as no items. A bare string
    /// counts as one item and an empty string as none, so a face that fills
    /// defaults in as text and a client that omits the key agree.
    pub fn opt_str_list(&self, key: &str) -> Result<Vec<String>, Error> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::String(s)) if s.trim().is_empty() => Ok(Vec::new()),
            Some(_) => self.str_list(key),
        }
    }

    /// Reject unknown arguments, missing required ones, and type mismatches.
    /// Called by [`VerbSpec::call`], so no face can skip it.
    pub fn validate(&self, spec: &VerbSpec) -> Result<(), Error> {
        for (key, value) in &self.map {
            let Some(arg) = spec.arg(key) else {
                let known: Vec<&str> = spec.args.iter().map(|a| a.name).collect();
                return Err(Error::BadArgs {
                    verb: spec.name.to_string(),
                    detail: format!("unknown argument {key:?}; accepts {known:?}"),
                });
            };
            if !type_matches(arg.ty, value) {
                return Err(Error::BadArgs {
                    verb: spec.name.to_string(),
                    detail: format!(
                        "{key} expects {}, got {value}",
                        arg.ty.json_type()
                    ),
                });
            }
        }
        for arg in spec.args {
            if arg.required && !self.map.contains_key(arg.name) {
                return Err(Error::BadArgs {
                    verb: spec.name.to_string(),
                    detail: format!("missing required argument {}: {}", arg.name, arg.help),
                });
            }
        }
        Ok(())
    }
}

fn type_matches(ty: ArgType, value: &Value) -> bool {
    match ty {
        ArgType::Str | ArgType::Path => value.is_string(),
        ArgType::Int => value.is_i64() || value.is_u64(),
        ArgType::Float => value.is_number(),
        ArgType::Bool => value.is_boolean(),
        ArgType::StrList => {
            value.is_string() || value.as_array().is_some_and(|a| a.iter().all(Value::is_string))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verb::lookup;
    use serde_json::json;

    #[test]
    fn unknown_argument_fails_closed() {
        let spec = lookup("goto").expect("goto");
        let args = Args::from_value(json!({"url": "https://x", "typo": 1})).expect("object");
        let err = args.validate(spec).expect_err("typo must be refused");
        assert!(err.to_string().contains("typo"), "{err}");
    }

    #[test]
    fn missing_required_names_the_argument() {
        let spec = lookup("goto").expect("goto");
        let err = Args::new().validate(spec).expect_err("url is required");
        assert!(err.to_string().contains("url"), "{err}");
    }

    #[test]
    fn wrong_type_is_refused() {
        let spec = lookup("wait").expect("wait");
        let args = Args::from_value(json!({"ms": "soon"})).expect("object");
        let err = args.validate(spec).expect_err("ms is an integer");
        assert!(err.to_string().contains("integer"), "{err}");
    }

    #[test]
    fn a_bare_string_is_a_one_element_list() {
        let args = Args::from_value(json!({"files": "/tmp/a.png"})).expect("object");
        assert_eq!(args.str_list("files").expect("list"), vec!["/tmp/a.png"]);
    }

    #[test]
    fn non_object_arguments_are_refused() {
        let err = Args::from_value(json!([1, 2])).expect_err("array is not an argument set");
        assert!(err.to_string().contains("JSON object"), "{err}");
    }
}
