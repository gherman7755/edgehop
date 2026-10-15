//! Typed, strict access to a request's `params`. Missing keys fall back to defaults; keys of the wrong type
//! are errors (a boolean is never a number and the other way round).

use std::ops::RangeInclusive;

use serde_json::{Map, Value};

use crate::ipc::IpcError;

pub struct Params<'a>(pub &'a Map<String, Value>);

impl Params<'_> {
    pub fn string(&self, key: &str) -> Result<Option<&str>, IpcError> {
        match self.0.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(_) => Err(IpcError::InvalidParams(format!(
                "\"{key}\" must be a string"
            ))),
        }
    }

    pub fn required_string(&self, key: &str) -> Result<&str, IpcError> {
        self.string(key)?
            .ok_or_else(|| IpcError::InvalidParams(format!("\"{key}\" is required")))
    }

    pub fn bool(&self, key: &str, default: bool) -> Result<bool, IpcError> {
        match self.0.get(key) {
            None => Ok(default),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(IpcError::InvalidParams(format!(
                "\"{key}\" must be true or false"
            ))),
        }
    }

    pub fn int(&self, key: &str, range: RangeInclusive<i64>) -> Result<Option<i64>, IpcError> {
        let Some(value) = self.0.get(key) else {
            return Ok(None);
        };
        match value.as_i64() {
            Some(n) if range.contains(&n) => Ok(Some(n)),
            _ => Err(IpcError::InvalidParams(format!(
                "\"{key}\" must be an integer from {} to {}",
                range.start(),
                range.end()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn strings() {
        let m = map(json!({"a": "x", "n": 1}));
        let p = Params(&m);
        assert_eq!(p.string("a"), Ok(Some("x")));
        assert_eq!(p.string("missing"), Ok(None));
        assert!(matches!(p.string("n"), Err(IpcError::InvalidParams(_))));
        assert_eq!(p.required_string("a"), Ok("x"));
        assert_eq!(
            p.required_string("missing"),
            Err(IpcError::InvalidParams("\"missing\" is required".into()))
        );
    }

    #[test]
    fn booleans_are_not_numbers_and_numbers_are_not_booleans() {
        let m = map(json!({"flag": true, "one": 1, "text": "yes"}));
        let p = Params(&m);
        assert_eq!(p.bool("flag", false), Ok(true));
        assert_eq!(p.bool("missing", true), Ok(true));
        assert!(p.bool("one", false).is_err());
        assert!(p.bool("text", false).is_err());
        assert!(p.int("flag", 0..=10).is_err());
    }

    #[test]
    fn integers_are_range_checked() {
        let m = map(json!({"port": 47474, "big": 70000, "neg": -1, "frac": 1.5}));
        let p = Params(&m);
        assert_eq!(p.int("port", 1..=65535), Ok(Some(47474)));
        assert_eq!(p.int("missing", 1..=65535), Ok(None));
        assert!(p.int("big", 1..=65535).is_err());
        assert!(p.int("neg", 0..=65535).is_err());
        assert!(p.int("frac", 0..=10).is_err());
        assert_eq!(
            p.int("big", 1..=65535),
            Err(IpcError::InvalidParams(
                "\"big\" must be an integer from 1 to 65535".into()
            ))
        );
    }
}
