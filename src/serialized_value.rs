//! Conversion between [`serde_json::Value`] and Playwright's `SerializedValue`
//! wire format used by `evaluate` and friends.
//!
//! Playwright cannot send raw JSON across the protocol because JSON cannot
//! represent `undefined`, `NaN`, `Infinity` or `Date`. It uses a tagged
//! encoding instead: `{ "n": 1 }` for numbers, `{ "s": "x" }` for strings,
//! `{ "b": true }` for booleans, `{ "v": "null" }` for the JS literals, `{ "a":
//! [...] }` for arrays and `{ "o": [{ "k":.., "v":.. }] }` for objects.

use serde_json::{json, Map, Value};

/// Encodes a JSON value as a Playwright `SerializedValue`.
pub fn serialize(value: &Value) -> Value {
    match value {
        Value::Null => json!({ "v": "null" }),
        Value::Bool(b) => json!({ "b": b }),
        Value::Number(n) => json!({ "n": n }),
        Value::String(s) => json!({ "s": s }),
        Value::Array(items) => {
            let encoded: Vec<Value> = items.iter().map(serialize).collect();
            json!({ "a": encoded })
        }
        Value::Object(map) => {
            let encoded: Vec<Value> = map
                .iter()
                .map(|(k, v)| json!({ "k": k, "v": serialize(v) }))
                .collect();
            json!({ "o": encoded })
        }
    }
}

/// Wraps a serialized value as the `arg` payload expected by `evaluate`.
pub fn to_arg(value: &Value) -> Value {
    json!({ "value": serialize(value), "handles": [] })
}

/// Decodes a Playwright `SerializedValue` back into a JSON value.
///
/// JS-only values that have no JSON representation are mapped as follows:
/// `undefined` and `null` become [`Value::Null`]; `NaN`/`Infinity`/`-Infinity`
/// become their closest `f64` encoding (and therefore JSON null if not
/// representable); `-0` becomes `0`.
pub fn deserialize(value: &Value) -> Value {
    let obj = match value.as_object() {
        Some(o) => o,
        None => return Value::Null,
    };

    if let Some(n) = obj.get("n") {
        return n.clone();
    }
    if let Some(s) = obj.get("s") {
        return s.clone();
    }
    if let Some(b) = obj.get("b") {
        return b.clone();
    }
    if let Some(v) = obj.get("v").and_then(Value::as_str) {
        return match v {
            "null" | "undefined" => Value::Null,
            "NaN" => number_or_null(f64::NAN),
            "Infinity" => number_or_null(f64::INFINITY),
            "-Infinity" => number_or_null(f64::NEG_INFINITY),
            "-0" => json!(0),
            _ => Value::Null,
        };
    }
    if let Some(d) = obj.get("d") {
        // Dates are surfaced as their ISO-8601 string.
        return d.clone();
    }
    if let Some(items) = obj.get("a").and_then(Value::as_array) {
        return Value::Array(items.iter().map(deserialize).collect());
    }
    if let Some(entries) = obj.get("o").and_then(Value::as_array) {
        let mut map = Map::new();
        for entry in entries {
            if let (Some(k), Some(v)) = (entry.get("k").and_then(Value::as_str), entry.get("v")) {
                map.insert(k.to_string(), deserialize(v));
            }
        }
        return Value::Object(map);
    }

    Value::Null
}

fn number_or_null(f: f64) -> Value {
    serde_json::Number::from_f64(f)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_plain_json() {
        let original = json!({
            "name": "widget",
            "count": 3,
            "active": true,
            "tags": ["a", "b"],
            "nested": { "x": 1.5 }
        });
        let encoded = serialize(&original);
        let decoded = deserialize(&encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn decodes_js_specials() {
        assert_eq!(deserialize(&json!({ "v": "null" })), Value::Null);
        assert_eq!(deserialize(&json!({ "v": "undefined" })), Value::Null);
        assert_eq!(deserialize(&json!({ "v": "-0" })), json!(0));
    }
}
