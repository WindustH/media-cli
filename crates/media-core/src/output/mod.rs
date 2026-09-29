//! Printing results: a human view for terminals, and a stable envelope
//! (`{ok, schema_version, platform, data | error}`) as JSON or YAML for
//! scripts and agents. Non-terminal stdout defaults to YAML.

mod human;

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

/// Print a successful result.
pub fn emit(format: Format, platform: Option<&str>, data: &Data) {
  match format {
    Format::Table => human::render(data),
    _ => print_structured(
      format,
      &Envelope {
        ok: true,
        schema_version: SCHEMA_VERSION,
        platform,
        data: Some(data),
        error: None,
      },
    ),
  }
}

/// Print a failure: the envelope on stdout for machines, a message on stderr for people.
pub fn emit_error(format: Format, platform: Option<&str>, error: &Error) {
  match format {
    Format::Table => human::error(error),
    _ => print_structured::<()>(
      format,
      &Envelope {
        ok: false,
        schema_version: SCHEMA_VERSION,
        platform,
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
