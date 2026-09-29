//! Import login cookies from locally installed browsers.

use crate::error::{Error, Result};
use crate::http::Cookies;

/// Browsers `login --browser <name>` understands.
pub const BROWSERS: &[&str] = &[
  "chrome",
  "chromium",
  "edge",
  "brave",
  "firefox",
  "librewolf",
  "zen",
  "vivaldi",
  "opera",
  "arc",
];

/// Cookies for `domains` from one browser, or from every installed browser in
/// turn. Each returned entry is `(browser, cookies)`; empty jars are skipped.
#[cfg(feature = "browser")]
pub fn import(browser: Option<&str>, domains: &[&str]) -> Result<Vec<(String, Cookies)>> {
  let names: Vec<&str> = match browser {
    Some(b) => {
      let b = b.to_ascii_lowercase();
      let found = BROWSERS.iter().find(|n| **n == b).ok_or_else(|| {
        Error::input(format!(
          "unknown browser `{b}`; expected one of {}",
          BROWSERS.join(", ")
        ))
      })?;
      vec![found]
    }
    None => BROWSERS.to_vec(),
  };
  let domains: Vec<String> = domains.iter().map(|d| d.to_string()).collect();
  let mut found = Vec::new();
  for name in names {
    let loader: fn(Option<Vec<String>>) -> rookie::Result<Vec<rookie::enums::Cookie>> = match name {
      "chrome" => rookie::chrome,
      "chromium" => rookie::chromium,
      "edge" => rookie::edge,
      "brave" => rookie::brave,
      "firefox" => rookie::firefox,
      "librewolf" => rookie::librewolf,
      "zen" => rookie::zen,
      "vivaldi" => rookie::vivaldi,
      "opera" => rookie::opera,
      "arc" => rookie::arc,
      _ => continue,
    };
    match loader(Some(domains.clone())) {
      Ok(list) => {
        let mut jar = Cookies::new();
        // Later entries win; prefer non-empty values.
        for c in list.into_iter().filter(|c| !c.value.is_empty()) {
          jar.insert(c.name, c.value);
        }
        if !jar.is_empty() {
          found.push((name.to_owned(), jar));
        }
      }
      Err(e) => tracing::debug!("{name}: {e}"),
    }
  }
  Ok(found)
}

#[cfg(not(feature = "browser"))]
pub fn import(_browser: Option<&str>, _domains: &[&str]) -> Result<Vec<(String, Cookies)>> {
  Err(
    Error::new(
      crate::error::ErrorCode::UnsupportedOperation,
      "this build cannot read browser cookies (it was built without the `browser` feature)",
    )
    .with_hint("log in with `--cookie '...'` instead"),
  )
}

/// Parse `a=1; b=2` (as copied from the browser's request headers).
pub fn parse_cookie_string(s: &str) -> Cookies {
  s.split(';')
    .filter_map(|pair| {
      let (k, v) = pair.split_once('=')?;
      let (k, v) = (k.trim(), v.trim().trim_matches('"'));
      (!k.is_empty()).then(|| (k.to_owned(), v.to_owned()))
    })
    .collect()
}
