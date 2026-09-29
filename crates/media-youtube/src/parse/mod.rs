//! InnerTube JSON → core models. Responses nest "renderers" (`videoRenderer`,
//! `lockupViewModel`, ...) inside layout containers; [`items`] walks a response
//! and maps every item renderer it knows, the other modules map one shape each.
//!
//! The web client is asked in English (`hl=en`), so counts (`1.2M views`),
//! relative times (`3 days ago`) and dates (`Oct 12, 2021`) parse reliably.

mod channel;
mod comment;
mod items;
mod post;
mod video;

use jiff::{SignedDuration, Timestamp};
use media_core::text::parse_count;
use media_core::{Extra, User, Value, ValueExt};

use crate::refs::channel_url;

pub use channel::{about, channel, channel_page};
pub use comment::{Entities, comment_view, header_count};
pub use items::{Listing, listing, listing_without, token};
pub use video::full as full_video;

/// Plain text of `simpleText`, `runs`, `content` (view models) or a string.
pub fn text(v: &Value) -> Option<String> {
  let s = match v {
    Value::String(s) => s.clone(),
    _ => v
      .str("simpleText")
      .or_else(|| v.str("content"))
      .or_else(|| {
        let runs = v.list("runs");
        (!runs.is_empty()).then(|| runs.iter().filter_map(|r| r.str("text")).collect())
      })?,
  };
  let s = s.trim().to_owned();
  (!s.is_empty()).then_some(s)
}

/// The first number of a text: `2,519,718 views`, `1.2M views`, `4.29M
/// subscribers`, `10 replies`; `No views` is zero.
pub fn count(text: &str) -> Option<u64> {
  let first = text.split_whitespace().next()?;
  if first.eq_ignore_ascii_case("no") {
    return Some(0);
  }
  parse_count(first)
}

/// `3 days ago`, `Streamed 2 weeks ago`, `1 year ago (edited)`, `4y ago`,
/// `3mo ago` → an approximate time (months as 30 days, years as 365).
pub fn ago(text: &str) -> Option<Timestamp> {
  let words: Vec<&str> = text.split_whitespace().collect();
  let i = words.iter().position(|w| w.starts_with("ago"))?;
  let before = words.get(i.checked_sub(1)?)?;
  // `4y` in one word, or `4 years` in two.
  let split = before.find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
  let (n, unit) = if split > 0 {
    before.split_at(split)
  } else {
    (*words.get(i.checked_sub(2)?)?, *before)
  };
  let n: i64 = n.parse().ok()?;
  let secs = match unit.trim_end_matches('s') {
    "second" | "sec" => 1,
    "minute" | "min" | "m" => 60,
    "hour" | "hr" | "h" => 3_600,
    "day" | "d" => 86_400,
    "week" | "wk" | "w" => 7 * 86_400,
    "month" | "mo" => 30 * 86_400,
    "year" | "yr" | "y" => 365 * 86_400,
    _ => return None,
  };
  Timestamp::now()
    .checked_sub(SignedDuration::from_secs(n * secs))
    .ok()
}

/// A date inside a text: `Oct 12, 2021`, `Joined Apr 7, 2017`, `Last updated
/// on Dec 11, 2021`, `Premiered Oct 12, 2021` (as UTC midnight).
pub fn date(text: &str) -> Option<Timestamp> {
  let words: Vec<&str> = text.split_whitespace().collect();
  words.windows(3).find_map(|w| {
    let s = format!("{} {} {}", w[0], w[1], w[2]);
    jiff::fmt::strtime::parse("%b %d, %Y", &s)
      .ok()?
      .to_date()
      .ok()?
      .to_zoned(jiff::tz::TimeZone::UTC)
      .ok()
      .map(|z| z.timestamp())
  })
}

/// A relative or absolute publication time.
pub fn when(text: &str) -> Option<Timestamp> {
  ago(text).or_else(|| date(text))
}

/// `7:10` / `1:02:03` → seconds.
pub fn duration(text: &str) -> Option<u64> {
  if !text.contains(':') {
    return None;
  }
  text.trim().split(':').try_fold(0u64, |acc, part| {
    Some(acc * 60 + part.trim().parse::<u64>().ok()?)
  })
}

/// The largest image of `thumbnails` / `sources` (protocol-relative URLs fixed).
pub fn image(v: &Value) -> Option<String> {
  let list = if v.list("thumbnails").is_empty() {
    v.list("sources")
  } else {
    v.list("thumbnails")
  };
  let url = list
    .iter()
    .max_by_key(|t| t.u64("width").unwrap_or(0))
    .and_then(|t| t.str("url"))?;
  Some(match url.strip_prefix("//") {
    Some(rest) => format!("https://{rest}"),
    None => url,
  })
}

/// A channel from the `browseEndpoint` of a byline or avatar link.
pub fn owner(endpoint: &Value, name: Option<String>) -> Option<User> {
  let b = endpoint.at("browseEndpoint");
  let id = b.str("browseId").filter(|id| id.starts_with("UC"))?;
  let handle = b
    .str("canonicalBaseUrl")
    .and_then(|u| u.strip_prefix("/").map(str::to_owned))
    .filter(|h| h.starts_with('@'));
  Some(User {
    name: name
      .or_else(|| handle.clone())
      .unwrap_or_else(|| id.clone()),
    url: Some(channel_url(&id)),
    handle,
    id,
    ..User::default()
  })
}

/// The channel of a `runs` byline (`ownerText`, `longBylineText` ...).
pub fn byline(v: &Value) -> Option<User> {
  let run = v.list("runs").first()?;
  owner(run.at("navigationEndpoint"), run.str("text"))
}

/// Whether a badge list marks a verified channel.
pub fn verified(badges: &Value) -> bool {
  badges.as_array().is_some_and(|list| {
    list.iter().any(|b| {
      b.str("metadataBadgeRenderer.style")
        .is_some_and(|s| s.contains("VERIFIED"))
    })
  })
}

/// Every value under `key` anywhere in `v` (not descending into matches).
pub fn find<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
  fn walk<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
      Value::Object(m) => {
        for (k, child) in m {
          if k == key {
            out.push(child);
          } else {
            walk(child, key, out);
          }
        }
      }
      Value::Array(a) => a.iter().for_each(|c| walk(c, key, out)),
      _ => {}
    }
  }
  let mut out = Vec::new();
  walk(v, key, &mut out);
  out
}

/// The first value under `key` anywhere in `v`.
pub fn first<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
  find(v, key).into_iter().next()
}

/// The first number with digits in a sentence (`like this video along with
/// 85,005 other people`).
pub fn number_in(text: &str) -> Option<u64> {
  text
    .split_whitespace()
    .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
    .filter(|w| w.starts_with(|c: char| c.is_ascii_digit()))
    .find_map(parse_count)
}

fn put(extra: &mut Extra, key: &str, value: impl Into<Value>) {
  let v = value.into();
  if !v.is_null() {
    extra.insert(key.into(), v);
  }
}
