//! Results as flat rows, for JSON Lines and CSV (pandas, DuckDB, spreadsheets).
//!
//! Listings give one row per item; comment threads are flattened with
//! `depth` and `parent_id`; insights give one row per total, trend point and
//! breakdown slice, told apart by `section`.

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::model::{Comment, Data, Insights};

fn value<T: Serialize>(item: &T) -> Value {
  serde_json::to_value(item).unwrap_or(Value::Null)
}

fn each<T: Serialize>(items: &[T]) -> Vec<Value> {
  items.iter().map(value).collect()
}

fn comments(items: &[Comment], parent: Option<&str>, depth: usize, out: &mut Vec<Value>) {
  for c in items {
    let mut row = value(c);
    if let Value::Object(m) = &mut row {
      m.remove("replies");
      m.insert("depth".into(), depth.into());
      m.insert("parent_id".into(), parent.map_or(Value::Null, Into::into));
    }
    out.push(row);
    comments(&c.replies, Some(&c.id), depth + 1, out);
  }
}

fn insights(i: &Insights) -> Vec<Value> {
  let base = json!({ "kind": i.kind, "subject": i.subject });
  let with = |fields: Value| {
    let mut row = base.clone();
    if let (Value::Object(m), Value::Object(f)) = (&mut row, fields) {
      m.extend(f);
    }
    row
  };
  let mut out = Vec::new();
  for (metric, v) in &i.totals {
    out.push(with(
      json!({ "section": "total", "metric": metric, "value": v }),
    ));
  }
  for s in &i.series {
    for p in &s.points {
      out.push(with(
        json!({ "section": "series", "metric": s.metric, "date": p.date, "value": p.value }),
      ));
    }
  }
  for b in &i.breakdowns {
    for x in &b.items {
      out.push(with(json!({
        "section": "breakdown", "dimension": b.dimension, "label": x.label, "value": x.value, "ratio": x.ratio,
      })));
    }
  }
  out
}

pub fn rows(data: &Data) -> Vec<Value> {
  match data {
    Data::Posts(p) => each(&p.items),
    Data::Users(p) => each(&p.items),
    Data::Collections(p) => each(&p.items),
    Data::Notifications(p) => each(&p.items),
    Data::Comments(p) => {
      let mut out = Vec::new();
      comments(&p.items, None, 0, &mut out);
      out
    }
    Data::Post(x) => vec![value(x)],
    Data::User(x) => vec![value(x)],
    Data::Action(x) => vec![value(x)],
    Data::Auth(x) => vec![value(x)],
    Data::Counts(x) => vec![value(x)],
    Data::Transcript(t) => t
      .cues
      .iter()
      .map(|c| json!({ "lang": t.lang, "from": c.from, "to": c.to, "text": c.text }))
      .collect(),
    Data::Downloads(x) => each(x),
    Data::Insights(i) => insights(i),
    Data::Value(Value::Array(a)) => a.clone(),
    Data::Value(v) => vec![v.clone()],
  }
}

/// Nested objects become dotted columns; lists of plain values are joined
/// with `|`, other lists stay JSON text.
pub fn flatten(row: &Value) -> Map<String, Value> {
  fn walk(prefix: &str, v: &Value, out: &mut Map<String, Value>) {
    match v {
      Value::Object(m) => {
        for (k, x) in m {
          let key = if prefix.is_empty() {
            k.clone()
          } else {
            format!("{prefix}.{k}")
          };
          walk(&key, x, out);
        }
      }
      Value::Array(a) if a.iter().all(|x| !x.is_object() && !x.is_array()) => {
        let joined: Vec<String> = a.iter().map(cell).collect();
        out.insert(prefix.to_owned(), joined.join("|").into());
      }
      Value::Array(_) => {
        out.insert(prefix.to_owned(), v.to_string().into());
      }
      other => {
        out.insert(prefix.to_owned(), other.clone());
      }
    }
  }
  let mut out = Map::new();
  walk("", row, &mut out);
  out
}

/// Text of one CSV cell.
pub fn cell(v: &Value) -> String {
  match v {
    Value::Null => String::new(),
    Value::String(s) => s.clone(),
    other => other.to_string(),
  }
}
