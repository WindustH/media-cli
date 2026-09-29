//! Tweet, user and list references: bare ids, handles and x.com / twitter.com URLs.

use media_core::{Error, Result};
use url::Url;

/// Hosts whose status / profile URLs we accept (mirrors included).
const HOSTS: &[&str] = &[
  "x.com",
  "twitter.com",
  "mobile.twitter.com",
  "mobile.x.com",
  "fxtwitter.com",
  "vxtwitter.com",
  "fixupx.com",
];

/// First path segments that are not user handles.
const RESERVED: &[&str] = &[
  "i",
  "home",
  "explore",
  "search",
  "settings",
  "messages",
  "notifications",
  "compose",
  "intent",
  "share",
  "hashtag",
  "login",
  "logout",
  "tos",
  "privacy",
  "jobs",
];

/// An x.com / twitter.com URL, also without its `https://`.
fn parse_url(arg: &str) -> Option<Url> {
  let url = if arg.contains("://") {
    Url::parse(arg).ok()?
  } else {
    Url::parse(&format!("https://{arg}")).ok()?
  };
  let host = url.host_str()?.trim_start_matches("www.");
  HOSTS.contains(&host).then_some(url)
}

fn segments(url: &Url) -> Vec<&str> {
  url
    .path_segments()
    .map(|s| s.filter(|p| !p.is_empty()).collect())
    .unwrap_or_default()
}

fn is_id(s: &str) -> bool {
  !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_handle(s: &str) -> bool {
  !s.is_empty() && s.len() <= 15 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Tweet id from `123`, `https://x.com/user/status/123[/photo/1]`, `/i/web/status/123` ...
pub fn tweet_id(arg: &str) -> Result<String> {
  let arg = arg.trim();
  if is_id(arg) {
    return Ok(arg.to_owned());
  }
  let found = parse_url(arg).and_then(|url| {
    let segs = segments(&url);
    segs
      .windows(2)
      .find(|w| matches!(w[0], "status" | "statuses" | "article") && is_id(w[1]))
      .map(|w| w[1].to_owned())
  });
  found.ok_or_else(|| Error::input(format!("not a tweet id or URL: {arg}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserRef {
  Id(String),
  Handle(String),
}

/// `@name`, `name`, a numeric id, `https://x.com/name` or `https://x.com/i/user/123`.
pub fn user(arg: &str) -> Result<UserRef> {
  let arg = arg.trim();
  if let Some(handle) = arg.strip_prefix('@').filter(|h| is_handle(h)) {
    return Ok(UserRef::Handle(handle.to_owned()));
  }
  if is_id(arg) {
    return Ok(UserRef::Id(arg.to_owned()));
  }
  if is_handle(arg) {
    return Ok(UserRef::Handle(arg.to_owned()));
  }
  let found = parse_url(arg).and_then(|url| {
    let query = |key: &str| {
      url
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
    };
    match segments(&url).as_slice() {
      ["i", "user", id, ..] if is_id(id) => Some(UserRef::Id(id.to_string())),
      ["intent", ..] => query("user_id")
        .filter(|v| is_id(v))
        .map(UserRef::Id)
        .or_else(|| query("screen_name").map(UserRef::Handle)),
      [name, ..] if is_handle(name) && !RESERVED.contains(name) => {
        Some(UserRef::Handle(name.to_string()))
      }
      _ => None,
    }
  });
  found.ok_or_else(|| Error::input(format!("not a user handle, id or URL: {arg}")))
}

/// List id from `123` or `https://x.com/i/lists/123`.
pub fn list_id(arg: &str) -> Result<String> {
  let arg = arg.trim();
  if is_id(arg) {
    return Ok(arg.to_owned());
  }
  let found = parse_url(arg).and_then(|url| match segments(&url).as_slice() {
    ["i", "lists", id, ..] if is_id(id) => Some(id.to_string()),
    _ => None,
  });
  found.ok_or_else(|| Error::input(format!("not a list id or URL: {arg}")))
}

pub fn tweet_url(handle: &str, id: &str) -> String {
  format!("https://x.com/{handle}/status/{id}")
}

pub fn user_url(handle: &str) -> String {
  format!("https://x.com/{handle}")
}

pub fn list_url(id: &str) -> String {
  format!("https://x.com/i/lists/{id}")
}
