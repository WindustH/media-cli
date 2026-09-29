//! Printing results: a human view for terminals, a stable envelope
//! (`{ok, schema_version, platform, fetched_at, data | error}`) as JSON or
//! YAML for scripts and agents, and flat rows (JSON Lines, CSV) for analysis.
//! Non-terminal stdout defaults to YAML.

mod human;
pub(crate) mod rows;

use std::io::{IsTerminal, Write};

use serde::Serialize;

use crate::error::Error;
use crate::model::Data;

pub const SCHEMA_VERSION: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
  /// Tables and cards for people.
  Table,
  Json,
  Yaml,
  /// One JSON object per item, no envelope (errors go to stderr).
  Jsonl,
  /// One row per item with dotted columns, no envelope (errors go to stderr).
  Csv,
}

impl Format {
  /// Explicit choice, else `table` on a terminal and `yaml` when piped.
  pub fn resolve(explicit: Option<Format>) -> Format {
    explicit.unwrap_or_else(|| {
      if std::io::stdout().is_terminal() {
        Format::Table
      } else {
        Format::Yaml
      }
    })
  }
}

#[derive(Serialize)]
struct Envelope<'a, T: Serialize> {
  ok: bool,
  schema_version: &'static str,
  #[serde(skip_serializing_if = "Option::is_none")]
  platform: Option<&'a str>,
  fetched_at: jiff::Timestamp,
  #[serde(skip_serializing_if = "Option::is_none")]
  data: Option<&'a T>,
  #[serde(skip_serializing_if = "Option::is_none")]
  error: Option<&'a Error>,
}

fn print_structured<T: Serialize>(format: Format, envelope: &Envelope<'_, T>) {
  let text = match format {
    Format::Json => serde_json::to_string_pretty(envelope).unwrap_or_default() + "\n",
    _ => serde_saphyr::to_string(envelope).unwrap_or_default(),
  };
  let mut out = std::io::stdout().lock();
  let _ = out.write_all(text.as_bytes());
  let _ = out.flush();
}

/// Rows with `platform` and `fetched_at` added, for repeated snapshots.
fn print_rows(format: Format, platform: Option<&str>, data: &Data) {
  let fetched_at = jiff::Timestamp::now().to_string();
  let rows: Vec<serde_json::Map<String, serde_json::Value>> = rows::rows(data)
    .iter()
    .map(|r| {
      let mut row = match format {
        Format::Csv => rows::flatten(r),
        _ => match r {
          serde_json::Value::Object(m) => m.clone(),
          other => serde_json::Map::from_iter([("value".to_owned(), other.clone())]),
        },
      };
      if let Some(p) = platform {
        row.insert("platform".into(), p.into());
      }
      row.insert("fetched_at".into(), fetched_at.clone().into());
      row
    })
    .collect();
  let mut out = std::io::stdout().lock();
  if format == Format::Jsonl {
    for row in &rows {
      let _ = writeln!(out, "{}", serde_json::Value::Object(row.clone()));
    }
    return;
  }
  // Columns in first-seen order across all rows.
  let mut columns: Vec<&String> = Vec::new();
  for row in &rows {
    for key in row.keys() {
      if !columns.contains(&key) {
        columns.push(key);
      }
    }
  }
  let mut w = csv::Writer::from_writer(out);
  let _ = w.write_record(columns.iter().map(|c| c.as_str()));
  for row in &rows {
    let _ = w.write_record(
      columns
        .iter()
        .map(|c| row.get(*c).map(rows::cell).unwrap_or_default()),
    );
  }
  let _ = w.flush();
}

/// Print a successful result.
pub fn emit(format: Format, platform: Option<&str>, data: &Data) {
  match format {
    Format::Table => human::render(data),
    Format::Jsonl | Format::Csv => print_rows(format, platform, data),
    _ => print_structured(
      format,
      &Envelope {
        ok: true,
        schema_version: SCHEMA_VERSION,
        platform,
        fetched_at: jiff::Timestamp::now(),
        data: Some(data),
        error: None,
      },
    ),
  }
}

/// Print a failure: the envelope on stdout for machines, a message on stderr for people.
pub fn emit_error(format: Format, platform: Option<&str>, error: &Error) {
  match format {
    Format::Table | Format::Jsonl | Format::Csv => human::error(error),
    _ => print_structured::<()>(
      format,
      &Envelope {
        ok: false,
        schema_version: SCHEMA_VERSION,
        platform,
        fetched_at: jiff::Timestamp::now(),
        data: None,
        error: Some(error),
      },
    ),
  }
}

/// Progress / status line for people, on stderr so it never mixes with data.
pub fn note(message: &str) {
  human::note(message);
}
