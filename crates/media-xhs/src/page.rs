//! Server-rendered note pages (`xhs_cli/html_parser.py`): the fallback for
//! reading a note without a working `xsec_token`, and a source of tokens.

use std::sync::LazyLock;

use media_core::{Error, Result, Value, ValueExt};
use regex::Regex;

use crate::api::{Client, HOME};
use crate::parse::snake_keys;
use crate::refs::NoteRef;

static STATE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?s)window\.__INITIAL_STATE__=(\{.*?\})\s*</script>").expect("regex")
});
static UNDEFINED: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"([:,])\s*undefined").expect("regex"));

/// The HTML of `/explore/<id>`, with the token in the query when known.
pub async fn fetch(c: &Client, note: &NoteRef) -> Result<String> {
  let url = match note.token {
    Some(_) => note.url(),
    None => format!("{HOME}/explore/{}", note.id),
  };
  let resp = c
    .ctx
    .http
    .get(url)
    .header("referer", format!("{HOME}/"))
    .header(
      "accept",
      "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    )
    .send()
    .await?
    .check()?;
  Ok(resp.text())
}

/// The note object of the page state, with snake_case keys.
pub fn note(html: &str, id: &str) -> Result<Value> {
  let raw = STATE
    .captures(html)
    .map(|c| c[1].to_owned())
    .ok_or_else(|| Error::upstream("the note page has no __INITIAL_STATE__"))?;
  let cleaned = UNDEFINED.replace_all(&raw, r#"$1"""#);
  let state: Value = serde_json::from_str(&cleaned)
    .map_err(|e| Error::upstream(format!("cannot parse the note page state: {e}")))?;
  let map = state
    .pointer("/note/noteDetailMap")
    .and_then(Value::as_object)
    .filter(|m| !m.is_empty())
    .ok_or_else(|| Error::not_found(format!("note {id} is not on its page")))?;
  let entry = map.get(id).or_else(|| map.values().next());
  let note = entry
    .and_then(|e| e.get("note"))
    .filter(|n| n.get("noteId").is_some() || n.get("title").is_some())
    .ok_or_else(|| {
      Error::not_found(format!("note {id} is not available"))
        .with_hint("pass the full note URL (with xsec_token) from a listing")
    })?;
  Ok(snake_keys(note.clone()))
}

/// The note's own `xsec_token` (and source) on its page: from the page state,
/// else from a link to this note. The reference takes the first token of the
/// page, which is often another link's (e.g. the author's profile).
pub fn token(html: &str, id: &str) -> Option<(String, Option<String>)> {
  if let Some(t) = note(html, id).ok().and_then(|n| n.str("xsec_token")) {
    return Some((t, None));
  }
  let link = Regex::new(&format!(
    r#"{id}\?[^"'<>\s]*?xsec_token=([^&"'\\\s]+)(?:&(?:amp;)?xsec_source=([^&"'\\\s]+))?"#
  ))
  .ok()?;
  let c = link.captures(html)?;
  Some((c[1].to_owned(), c.get(2).map(|s| s.as_str().to_owned())))
}
