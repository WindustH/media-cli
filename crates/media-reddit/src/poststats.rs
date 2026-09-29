//! The post insights page Reddit shows the author, `/poststats/t3_<id>/`
//! (the web app's `post_stats` route; comments have `/commentstats/t1_<id>/`).
//! It is rendered on the server, and its client bundle (chart tooltips,
//! `data-viz-breakdown`) shows where the numbers sit:
//!
//! - trends in the `tooltip-data` JSON of `<chart-tooltips-controller>`:
//!   `{dataNames: [..], data: {name: [..]}, xLabels: [..]}`;
//! - distributions in `<data-viz-breakdown>` (title in `slot="title"`) with
//!   `<data-viz-breakdown-item metric=".." percentage="..">label</..>` rows;
//! - headline numbers in `<faceplate-number number="..">` after their label.
//!
//! Other people's and deleted posts answer "no access", old ones "not
//! available" (insights are kept for about 90 days); both give `None`.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use jiff::Timestamp;
use jiff::civil::Date;
use media_core::http::status_error;
use media_core::text::{html_to_text, parse_count};
use media_core::{Breakdown, Error, Insights, Point, Result, Series, Share, Value, ValueExt};
use regex::Regex;

use crate::api::{Api, WWW};

fn re(pattern: &str) -> Regex {
  Regex::new(pattern).expect("valid pattern")
}

static CHART: LazyLock<Regex> = LazyLock::new(|| {
  re(r#"<chart-tooltips-controller\b[^>]*?\btooltip-data=(?:"([^"]*)"|'([^']*)')"#)
});
static BREAKDOWN: LazyLock<Regex> =
  LazyLock::new(|| re(r"(?s)<data-viz-breakdown\b([^>]*)>(.*?)</data-viz-breakdown>"));
static ITEM: LazyLock<Regex> =
  LazyLock::new(|| re(r"(?s)<data-viz-breakdown-item\b([^>]*)>(.*?)</data-viz-breakdown-item>"));
static TITLE: LazyLock<Regex> = LazyLock::new(|| re(r#"(?s)slot="title"[^>]*>(.*?)</"#));
static NUMBER: LazyLock<Regex> =
  LazyLock::new(|| re(r#"<faceplate-number\b[^>]*?\bnumber="([^"]*)""#));
static TAG: LazyLock<Regex> = LazyLock::new(|| re(r"<[^>]*>"));

/// What the page shows beyond the public counters.
#[derive(Debug, Default)]
pub struct Stats {
  series: Vec<Series>,
  breakdowns: Vec<Breakdown>,
  /// Headline numbers by their label on the page (in the account's language).
  numbers: BTreeMap<String, Value>,
}

pub async fn fetch(api: &Api, id: &str) -> Result<Option<Stats>> {
  api.require_login()?;
  let resp = api
    .ctx
    .http
    .get(format!("{WWW}/poststats/t3_{id}/"))
    .header("accept", "text/html,application/xhtml+xml")
    .send()
    .await?;
  if resp.url.contains("/login") {
    return Err(Error::auth("Reddit asked for a login").with_hint(api.ctx.login_hint()));
  }
  if !resp.status.is_success() {
    return Err(status_error(resp.status, &resp.text()));
  }
  let html = resp.text();
  let Some(start) = html.find("<main") else {
    return Ok(None);
  };
  let main = &html[start
    ..html[start..]
      .find("</main>")
      .map_or(html.len(), |e| start + e)];
  let stats = parse(main);
  let empty = stats.series.is_empty() && stats.breakdowns.is_empty() && stats.numbers.is_empty();
  Ok((!empty).then_some(stats))
}

fn parse(main: &str) -> Stats {
  let mut stats = Stats::default();
  for c in CHART.captures_iter(main) {
    let raw = c.get(1).or(c.get(2)).map_or("", |m| m.as_str());
    if let Ok(v) = serde_json::from_str::<Value>(&unescape(raw)) {
      stats.series.extend(series(&v));
    }
  }
  for (i, c) in BREAKDOWN.captures_iter(main).enumerate() {
    let title = TITLE
      .captures(&c[2])
      .map(|t| text(&t[1]))
      .filter(|t| !t.is_empty())
      .or_else(|| attr(&c[1], "breakdown-item-title"))
      .filter(|t| !t.is_empty())
      .unwrap_or_else(|| format!("breakdown {}", i + 1));
    let items: Vec<Share> = ITEM
      .captures_iter(&c[2])
      .map(|it| Share {
        label: text(&it[2]),
        value: attr(&it[1], "metric").map_or(Value::Null, |m| number(&m)),
        ratio: attr(&it[1], "percentage")
          .and_then(|p| p.trim().parse::<f64>().ok())
          .map(|p| if p > 1.0 { p / 100.0 } else { p }),
        ..Default::default()
      })
      .collect();
    if !items.is_empty() {
      let dimension = snake(&title);
      stats.breakdowns.push(Breakdown {
        dimension,
        items,
        ..Default::default()
      });
    }
  }
  // Headline numbers: outside the breakdowns, labelled by the text before them.
  let rest = BREAKDOWN.replace_all(main, "");
  for m in NUMBER.captures_iter(&rest) {
    let at = m.get(0).map_or(0, |m| m.start());
    let mut from = at.saturating_sub(600);
    while !rest.is_char_boundary(from) {
      from += 1;
    }
    let before = &rest[from..at];
    let before = &before[before.find('>').unwrap_or(0)..];
    let label = TAG
      .split(before)
      .map(text)
      .filter(|t| !t.is_empty() && parse_count(t).is_none())
      .last();
    if let Some(label) = label {
      stats.numbers.entry(label).or_insert_with(|| number(&m[1]));
    }
  }
  stats
}

/// One series per data name of a chart.
fn series(v: &Value) -> Vec<Series> {
  let labels = v.list("xLabels");
  v.list("dataNames")
    .iter()
    .filter_map(Value::as_str)
    .map(|name| Series {
      metric: snake(name),
      points: labels
        .iter()
        .zip(v.at("data").get(name).map_or(&[][..], |d| d.list("")))
        .map(|(label, value)| Point {
          date: date(
            &label
              .as_str()
              .map_or_else(|| label.to_string(), str::to_owned),
          ),
          // `Not available` and other text: no value.
          value: match value {
            Value::String(s) => Some(number(s))
              .filter(|n| !n.is_string())
              .unwrap_or_default(),
            other => other.clone(),
          },
        })
        .collect(),
    })
    .collect()
}

impl Stats {
  /// Add the page's numbers to `ins`; known headline labels become totals.
  pub fn merge_into(self, ins: &mut Insights) {
    for (label, value) in &self.numbers {
      let key = match label.to_ascii_lowercase().as_str() {
        "views" | "total views" | "post views" => "views",
        "shares" | "total shares" => "shares",
        "comments" => "comments",
        "upvotes" | "score" => "likes",
        "upvote rate" => "upvote_ratio",
        "crossposts" => "crossposts",
        _ => continue,
      };
      let value = match (key, value.as_f64()) {
        ("upvote_ratio", Some(p)) if p > 1.0 => Value::from(p / 100.0),
        _ => value.clone(),
      };
      ins.totals.insert(key.into(), value);
    }
    if !self.numbers.is_empty() {
      let numbers = self.numbers.into_iter().collect();
      ins
        .extra
        .insert("page_numbers".into(), Value::Object(numbers));
    }
    ins.series.extend(self.series);
    ins.breakdowns.extend(self.breakdowns);
  }
}

fn attr(attrs: &str, name: &str) -> Option<String> {
  let pattern = format!(r#"\b{}=(?:"([^"]*)"|'([^']*)')"#, regex::escape(name));
  let c = Regex::new(&pattern).ok()?.captures(attrs)?;
  c.get(1).or(c.get(2)).map(|m| unescape(m.as_str()))
}

fn unescape(s: &str) -> String {
  s.replace("&quot;", "\"")
    .replace("&#39;", "'")
    .replace("&#x27;", "'")
    .replace("&lt;", "<")
    .replace("&gt;", ">")
    .replace("&amp;", "&")
}

fn text(html: &str) -> String {
  html_to_text(html)
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
}

/// `1,234`, `1.2k`, `45%`, `0.93` → a number; anything else stays text.
fn number(s: &str) -> Value {
  let t = s.trim();
  if let Some(p) = t
    .strip_suffix('%')
    .and_then(|p| p.trim().parse::<f64>().ok())
  {
    return (p / 100.0).into();
  }
  if let Ok(n) = t.replace(',', "").parse::<i64>() {
    return n.into();
  }
  if let Ok(f) = t.replace(',', "").parse::<f64>() {
    return f.into();
  }
  parse_count(t).map_or_else(|| Value::from(t), Value::from)
}

/// `Post Upvotes` → `post_upvotes`.
fn snake(name: &str) -> String {
  let mut out = String::new();
  for w in name
    .split(|c: char| !c.is_alphanumeric())
    .filter(|w| !w.is_empty())
  {
    if !out.is_empty() {
      out.push('_');
    }
    out.push_str(&w.to_lowercase());
  }
  out
}

/// `2026-09-28`, `Sep 28`, `Sep 28, 2026`, `9月28日` → `YYYY-MM-DD` (without
/// a year: the latest such day not in the future); anything else, such as
/// hours, stays as it is.
fn date(label: &str) -> String {
  static MONTH_DAY: LazyLock<Regex> =
    LazyLock::new(|| re(r"^([A-Za-z]{3})[a-z]*\.? (\d{1,2})(?:, (\d{4}))?$"));
  static CJK: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:(\d{4})年)?(\d{1,2})月(\d{1,2})日$"));
  const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
  ];
  let l = label.trim();
  if let Some(d) = l.get(..10).and_then(|p| p.parse::<Date>().ok()) {
    return d.to_string();
  }
  let found = if let Some(c) = MONTH_DAY.captures(l) {
    let month = MONTHS
      .iter()
      .position(|m| m.eq_ignore_ascii_case(&c[1]))
      .and_then(|i| i8::try_from(i + 1).ok());
    civil(c.get(3), month, &c[2])
  } else if let Some(c) = CJK.captures(l) {
    civil(c.get(1), c[2].parse().ok(), &c[3])
  } else {
    None
  };
  found.unwrap_or_else(|| l.to_owned())
}

fn civil(year: Option<regex::Match>, month: Option<i8>, day: &str) -> Option<String> {
  let today = Timestamp::now()
    .to_zoned(jiff::tz::TimeZone::system())
    .date();
  let (month, day) = (month?, day.parse().ok()?);
  let year = year.and_then(|y| y.as_str().parse().ok());
  let mut d = Date::new(year.unwrap_or(today.year()), month, day).ok()?;
  if year.is_none() && d > today {
    d = Date::new(d.year() - 1, month, day).ok()?;
  }
  Some(d.to_string())
}
