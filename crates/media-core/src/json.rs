//! Path-based accessors for loosely typed upstream JSON.
//!
//! `v.str("data.card.name")` walks objects by key and arrays by index and
//! tolerates the usual upstream quirks: numbers sent as strings, ids sent as
//! numbers, counts written as `1.2万` or `3.4k`.

use serde_json::Value;

use crate::text::parse_count;

static NULL: Value = Value::Null;

pub trait ValueExt {
  /// The value at a dotted path (`a.b.0.c`), or `Null`.
  fn at(&self, path: &str) -> &Value;
  /// Non-empty string, also accepting numbers and bools.
  fn str(&self, path: &str) -> Option<String>;
  fn i64(&self, path: &str) -> Option<i64>;
  fn u64(&self, path: &str) -> Option<u64>;
  fn f64(&self, path: &str) -> Option<f64>;
  fn bool(&self, path: &str) -> Option<bool>;
  /// Counter that may be a number, a numeric string or a `1.2万` style string.
  fn count(&self, path: &str) -> Option<u64>;
  /// Array items at the path (empty when missing).
  fn list(&self, path: &str) -> &[Value];
  /// First non-empty string among several paths.
  fn first_str(&self, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|p| self.str(p))
  }
  fn first_count(&self, paths: &[&str]) -> Option<u64> {
    paths.iter().find_map(|p| self.count(p))
  }
}

impl ValueExt for Value {
  fn at(&self, path: &str) -> &Value {
    if path.is_empty() {
      return self;
    }
    let mut cur = self;
    for key in path.split('.') {
      cur = match cur {
        Value::Object(m) => m.get(key).unwrap_or(&NULL),
        Value::Array(a) => key
          .parse::<usize>()
          .ok()
          .and_then(|i| a.get(i))
          .unwrap_or(&NULL),
        _ => &NULL,
      };
    }
    cur
  }

  fn str(&self, path: &str) -> Option<String> {
    match self.at(path) {
      Value::String(s) if !s.is_empty() => Some(s.clone()),
      Value::Number(n) => Some(n.to_string()),
      Value::Bool(b) => Some(b.to_string()),
      _ => None,
    }
  }

  fn i64(&self, path: &str) -> Option<i64> {
    match self.at(path) {
      Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
      Value::String(s) => s.trim().parse().ok(),
      Value::Bool(b) => Some(*b as i64),
      _ => None,
    }
  }

  fn u64(&self, path: &str) -> Option<u64> {
    self.i64(path).and_then(|v| u64::try_from(v).ok())
  }

  fn f64(&self, path: &str) -> Option<f64> {
    match self.at(path) {
      Value::Number(n) => n.as_f64(),
      Value::String(s) => s.trim().parse().ok(),
      _ => None,
    }
  }

  fn bool(&self, path: &str) -> Option<bool> {
    match self.at(path) {
      Value::Bool(b) => Some(*b),
      Value::Number(n) => n.as_i64().map(|v| v != 0),
      Value::String(s) => match s.as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
      },
      _ => None,
    }
  }

  fn count(&self, path: &str) -> Option<u64> {
    match self.at(path) {
      Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f.max(0.0) as u64)),
      Value::String(s) => parse_count(s),
      _ => None,
    }
  }

  fn list(&self, path: &str) -> &[Value] {
    match self.at(path) {
      Value::Array(a) => a,
      _ => &[],
    }
  }
}
