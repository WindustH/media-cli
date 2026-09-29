//! Note and user references: ids, web URLs, xhslink short links and share
//! texts, plus a small id → `xsec_token` cache so bare ids keep working.

use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::time::Duration;

use media_core::{Ctx, Error, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::api::{Client, HOME};

/// `xsec_source` of feed / listing tokens, and the default like the reference client.
pub const SOURCE_FEED: &str = "pc_feed";
pub const SOURCE_SEARCH: &str = "pc_search";

#[derive(Debug, Clone, Default)]
pub struct NoteRef {
  pub id: String,
  pub token: Option<String>,
  pub source: Option<String>,
  /// The token came from the cache (and may be stale).
  pub cached: bool,
}

impl NoteRef {
  pub fn source(&self) -> &str {
    self.source.as_deref().unwrap_or(SOURCE_FEED)
  }

  pub fn url(&self) -> String {
    note_url(&self.id, self.token.as_deref(), self.source())
  }
}

/// Canonical note link; with a token it opens in a browser and round-trips through `read`.
pub fn note_url(id: &str, token: Option<&str>, source: &str) -> String {
  match token {
    Some(t) => format!(
      "{HOME}/explore/{id}?xsec_token={}&xsec_source={source}",
      sanitize(t)
    ),
    None => format!("{HOME}/explore/{id}"),
  }
}

pub fn user_url(id: &str) -> String {
  format!("{HOME}/user/profile/{id}")
}

/// Keep query-significant characters of a token from breaking the URL.
fn sanitize(token: &str) -> String {
  token
    .replace('%', "%25")
    .replace('+', "%2B")
    .replace('&', "%26")
    .replace('#', "%23")
}

static URL_IN_TEXT: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"https?://[^\s，,。]+").expect("regex"));

/// A URL inside pasted share text, or the argument itself.
fn extract_url(arg: &str) -> &str {
  let arg = arg.trim();
  if arg.starts_with("http") {
    return arg;
  }
  URL_IN_TEXT.find(arg).map_or(arg, |m| m.as_str())
}

fn valid_id(s: &str) -> bool {
  s.len() >= 8 && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// A `xiaohongshu.com` URL.
fn site_url(arg: &str) -> Result<url::Url> {
  let url = url::Url::parse(arg).map_err(|e| Error::input(format!("bad URL {arg}: {e}")))?;
  match url.host_str() {
    Some(h) if h == "xiaohongshu.com" || h.ends_with(".xiaohongshu.com") => Ok(url),
    _ => Err(Error::input(format!("not a Xiaohongshu URL: {arg}"))),
  }
}

/// Parse a note id or `xiaohongshu.com` note URL (without network access).
fn parse_note(arg: &str) -> Result<NoteRef> {
  let arg = extract_url(arg);
  if !arg.contains("://") {
    return match valid_id(arg) {
      true => Ok(NoteRef {
        id: arg.to_owned(),
        ..NoteRef::default()
      }),
      false => Err(Error::input(format!("not a note id or URL: {arg}"))),
    };
  }
  let url = site_url(arg)?;
  let id = url
    .path_segments()
    .and_then(|mut s| s.rfind(|p| !p.is_empty()))
    .filter(|id| valid_id(id))
    .ok_or_else(|| Error::input(format!("no note id in {arg}")))?;
  let query = |key: &str| {
    url
      .query_pairs()
      .find(|(k, v)| k == key && !v.is_empty())
      .map(|(_, v)| v.into_owned())
  };
  Ok(NoteRef {
    id: id.to_owned(),
    token: query("xsec_token"),
    source: query("xsec_source"),
    cached: false,
  })
}

/// Parse a user id or profile URL.
fn parse_user(arg: &str) -> Result<String> {
  let arg = extract_url(arg);
  if !arg.contains("://") {
    return match valid_id(arg) {
      true => Ok(arg.to_owned()),
      false => Err(Error::input(format!("not a user id or profile URL: {arg}"))),
    };
  }
  let url = site_url(arg)?;
  let segments: Vec<&str> = url
    .path_segments()
    .map(Iterator::collect)
    .unwrap_or_default();
  segments
    .iter()
    .position(|s| *s == "profile")
    .and_then(|i| segments.get(i + 1))
    .filter(|id| valid_id(id))
    .map(|id| (*id).to_owned())
    .ok_or_else(|| Error::input(format!("no user id in {arg}")))
}

fn is_short_link(arg: &str) -> bool {
  url::Url::parse(arg)
    .ok()
    .and_then(|u| u.host_str().map(|h| h.ends_with("xhslink.com")))
    .unwrap_or(false)
}

/// Follow an xhslink.com short link to the page it points at.
async fn expand(c: &Client, link: &str) -> Result<String> {
  let resp = c.ctx.http.get(link).no_cookies().send().await?;
  let url = url::Url::parse(&resp.url)
    .map_err(|e| Error::upstream(format!("short link led to a bad URL: {e}")))?;
  // Login walls wrap the target page in `redirectPath`.
  let wrapped = url
    .query_pairs()
    .find(|(k, _)| k == "redirectPath")
    .map(|(_, v)| match v.starts_with('/') {
      true => format!("{HOME}{v}"),
      false => v.into_owned(),
    });
  Ok(wrapped.unwrap_or(resp.url))
}

/// Resolve a note argument: short links are followed, a missing token comes
/// from the cache, and a token given explicitly is cached.
pub async fn note_ref(c: &Client, arg: &str) -> Result<NoteRef> {
  let mut r = match extract_url(arg) {
    link if is_short_link(link) => parse_note(&expand(c, link).await?)?,
    other => parse_note(other)?,
  };
  match &r.token {
    Some(token) => remember(&c.ctx, [(r.id.as_str(), token.as_str(), r.source())]),
    None => {
      if let Some(entry) = load(&c.ctx).remove(&r.id) {
        r.token = Some(entry.token);
        r.source = Some(entry.source).filter(|s| !s.is_empty());
        r.cached = true;
      }
    }
  }
  Ok(r)
}

/// Resolve a user argument (id, profile URL or short link).
pub async fn user_ref(c: &Client, arg: &str) -> Result<String> {
  match extract_url(arg) {
    link if is_short_link(link) => parse_user(&expand(c, link).await?),
    other => parse_user(other),
  }
}

// ── token cache ─────────────────────────────────────────────────────────

const CACHE_KEY: &str = "note-tokens";
const TOKEN_TTL: u64 = 86_400;
const MAX_TOKENS: usize = 500;

#[derive(Debug, Serialize, Deserialize)]
struct Entry {
  token: String,
  source: String,
  ts: u64,
}

fn now_secs() -> u64 {
  crate::sign::now_ms() / 1000
}

fn load(ctx: &Ctx) -> BTreeMap<String, Entry> {
  let now = now_secs();
  let mut map: BTreeMap<String, Entry> = ctx
    .store
    .cache_get(CACHE_KEY, Duration::from_secs(TOKEN_TTL))
    .unwrap_or_default();
  map.retain(|_, e| now.saturating_sub(e.ts) <= TOKEN_TTL);
  map
}

/// Cache `(id, token, source)` triples from a listing.
pub fn remember<'a>(ctx: &Ctx, items: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>) {
  let ts = now_secs();
  let fresh: Vec<(String, Entry)> = items
    .into_iter()
    .filter(|(_, token, _)| !token.is_empty())
    .map(|(id, token, source)| {
      let (token, source) = (token.to_owned(), source.to_owned());
      (id.to_owned(), Entry { token, source, ts })
    })
    .collect();
  if fresh.is_empty() {
    return;
  }
  let mut map = load(ctx);
  map.extend(fresh);
  if map.len() > MAX_TOKENS {
    let mut by_age: Vec<(u64, String)> = map.iter().map(|(k, e)| (e.ts, k.clone())).collect();
    by_age.sort();
    for (_, k) in by_age.into_iter().take(map.len() - MAX_TOKENS) {
      map.remove(&k);
    }
  }
  ctx.store.cache_put(CACHE_KEY, &map);
}

/// Drop a token that the API rejected.
pub fn forget(ctx: &Ctx, id: &str) {
  let mut map = load(ctx);
  if map.remove(id).is_some() {
    ctx.store.cache_put(CACHE_KEY, &map);
  }
}
