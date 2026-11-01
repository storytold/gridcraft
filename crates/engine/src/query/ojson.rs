//! JSON read with object keys in document order. Power Query lists a record's fields as the
//! document has them; `serde_json`'s own map sorts keys, which would reorder columns.

use std::fmt::Write as _;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

/// A JSON value; objects keep their keys in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Map),
}

/// An object's members in document order (a repeated key keeps its first place, last value).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map(Vec<(String, Json)>);

impl Map {
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Json)> {
        self.0.iter().map(|(k, v)| (k, v))
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(k, _)| k)
    }
    pub fn retain(&mut self, mut f: impl FnMut(&str) -> bool) {
        self.0.retain(|(k, _)| f(k));
    }
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(m) => m.get(key),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Json::Object(m) => Some(m),
            _ => None,
        }
    }
    pub fn as_object_mut(&mut self) -> Option<&mut Map> {
        match self {
            Json::Object(m) => Some(m),
            _ => None,
        }
    }
    pub fn is_object(&self) -> bool {
        matches!(self, Json::Object(_))
    }

    /// Compact JSON text, at most `max` bytes (longer output is cut).
    pub fn text(&self, max: usize) -> String {
        let mut out = String::new();
        write_json(self, &mut out, max);
        out
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_json(v: &Json, out: &mut String, max: usize) {
    if out.len() > max {
        return;
    }
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Number(n) => out.push_str(&gridcraft_core::number_to_text(*n)),
        Json::String(s) => write_str(s, out),
        Json::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(x, out, max);
            }
            out.push(']');
        }
        Json::Object(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write_json(x, out, max);
            }
            out.push('}');
        }
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }
    fn visit_none<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Json, D::Error> {
        Json::deserialize(d)
    }
    fn visit_bool<E>(self, b: bool) -> Result<Json, E> {
        Ok(Json::Bool(b))
    }
    fn visit_i64<E>(self, n: i64) -> Result<Json, E> {
        Ok(Json::Number(n as f64))
    }
    fn visit_u64<E>(self, n: u64) -> Result<Json, E> {
        Ok(Json::Number(n as f64))
    }
    fn visit_f64<E>(self, n: f64) -> Result<Json, E> {
        Ok(Json::Number(n))
    }
    fn visit_str<E>(self, s: &str) -> Result<Json, E> {
        Ok(Json::String(s.to_string()))
    }
    fn visit_string<E>(self, s: String) -> Result<Json, E> {
        Ok(Json::String(s))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut v = Vec::new();
        while let Some(x) = seq.next_element::<Json>()? {
            v.push(x);
        }
        Ok(Json::Array(v))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut members: Vec<(String, Json)> = Vec::new();
        let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        while let Some((k, v)) = map.next_entry::<String, Json>()? {
            match index.get(&k) {
                Some(&i) => {
                    if let Some(slot) = members.get_mut(i) {
                        slot.1 = v;
                    }
                }
                None => {
                    index.insert(k.clone(), members.len());
                    members.push((k, v));
                }
            }
        }
        Ok(Json::Object(Map(members)))
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Json, D::Error> {
        d.deserialize_any(JsonVisitor)
    }
}

/// Parses JSON text (a leading byte order mark is ignored). Nesting is limited by the parser's
/// recursion limit, so hostile input is an error, not a stack overflow.
pub fn parse(text: &str) -> Result<Json, String> {
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("The JSON couldn't be read: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_keep_document_order() {
        let v = parse(r#"{"z":1,"a":[true,null,"x\n"],"m":{"b":2,"a":3},"z":4}"#).unwrap();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["z", "a", "m"]);
        assert_eq!(v.get("z"), Some(&Json::Number(4.0)));
        assert_eq!(v.text(1000), r#"{"z":4,"a":[true,null,"x\n"],"m":{"b":2,"a":3}}"#);
        assert!(parse(&"[".repeat(10_000)).is_err());
    }
}
