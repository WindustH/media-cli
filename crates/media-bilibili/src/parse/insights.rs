//! Creator-center payloads -> the parts of [`media_core::Insights`]: days,
//! daily series, distributions and rates.

use jiff::Timestamp;
use media_core::{Breakdown, Metrics, Point, Post, Series, Share, Value, ValueExt, json};

/// The data center counts days in Beijing time.
fn beijing() -> jiff::tz::TimeZone {
  jiff::tz::offset(8).to_time_zone()
}

/// `YYYY-MM-DD` from unix seconds or a `20260928` style number.
pub fn day(v: &Value) -> Option<String> {
  let n = v.i64("")?;
  if (19_000_000..30_000_000).contains(&n) {
    return Some(format!(
      "{:04}-{:02}-{:02}",
      n / 10_000,
      n / 100 % 100,
      n % 100
    ));
  }
  let ts = Timestamp::from_second(n).ok().filter(|_| n > 0)?;
  Some(ts.to_zoned(beijing()).date().to_string())
}

/// Today in Beijing time.
pub fn today() -> String {
  Timestamp::now().to_zoned(beijing()).date().to_string()
}

/// The day `n` days before `date` (`YYYY-MM-DD`).
pub fn days_before(date: &str, n: u32) -> Option<String> {
  let d: jiff::civil::Date = date.parse().ok()?;
  let back = d.checked_sub(jiff::Span::new().days(i64::from(n))).ok()?;
  Some(back.to_string())
}

/// A number as JSON: an integer when it has no fraction.
pub fn num(v: f64) -> Value {
  if v.fract() == 0.0 && v.abs() < 9e15 {
    json!(v as i64)
  } else {
    json!(v)
  }
}

/// A rate sent in basis points (`1234` = 12.34%) as a 0..1 fraction.
pub fn basis_points(v: &Value, path: &str) -> Option<Value> {
  v.f64(path).map(|bp| json!(round4(bp / 10_000.0)))
}

/// A daily series from `[{date_key | date, total_inc | fans_total_inc}]`,
/// oldest first; points before `from` are dropped.
pub fn series(metric: &str, list: &[Value], from: Option<&str>) -> Option<Series> {
  let mut points: Vec<Point> = list
    .iter()
    .filter_map(|p| {
      let date = ["date_key", "date", "log_date"]
        .iter()
        .find_map(|k| day(p.at(k)))?;
      let value = ["total_inc", "fans_total_inc", "value"]
        .iter()
        .map(|k| p.at(k))
        .find(|v| !v.is_null())?
        .clone();
      Some(Point { date, value })
    })
    .filter(|p| from.is_none_or(|f| p.date.as_str() >= f))
    .collect();
  points.sort_by(|a, b| a.date.cmp(&b.date));
  points.dedup_by(|a, b| a.date == b.date);
  (!points.is_empty()).then(|| Series {
    metric: metric.to_owned(),
    points,
  })
}

/// A distribution from `(label, amount)` pairs; ratios are shares of the sum.
/// `None` when every amount is zero.
pub fn breakdown(
  dimension: &str,
  items: impl IntoIterator<Item = (String, f64)>,
  sort: bool,
) -> Option<Breakdown> {
  let mut items: Vec<(String, f64)> = items.into_iter().filter(|(l, _)| !l.is_empty()).collect();
  let sum: f64 = items.iter().map(|(_, v)| v.max(0.0)).sum();
  if sum <= 0.0 {
    return None;
  }
  if sort {
    items.sort_by(|a, b| b.1.total_cmp(&a.1));
  }
  Some(Breakdown {
    dimension: dimension.to_owned(),
    items: items
      .into_iter()
      .map(|(label, v)| Share {
        label,
        value: num(v),
        ratio: Some(round4(v / sum)),
      })
      .collect(),
  })
}

pub fn round4(v: f64) -> f64 {
  (v * 10_000.0).round() / 10_000.0
}

/// `(label, amount)` pairs of an object's fields, renamed by `labels` (others dropped).
pub fn fields(v: &Value, labels: &[(&str, &str)]) -> Vec<(String, f64)> {
  labels
    .iter()
    .filter_map(|(key, label)| Some(((*label).to_owned(), v.f64(key)?)))
    .collect()
}

/// `(label, amount)` pairs of a list of objects.
pub fn pairs(list: &[Value], label: &str, amount: &str) -> Vec<(String, f64)> {
  list
    .iter()
    .filter_map(|x| Some((x.str(label)?, x.f64(amount)?)))
    .collect()
}

/// Age groups as the data center labels them.
pub const AGES: &[(&str, &str)] = &[
  ("age_one", "<16"),
  ("age_two", "16-25"),
  ("age_three", "25-40"),
  ("age_four", ">40"),
];

pub const GENDERS: &[(&str, &str)] = &[("male", "male"), ("female", "female")];

/// Plays by terminal (`play_proportion`).
pub const DEVICES: &[(&str, &str)] = &[
  ("new_mobile", "mobile"),
  ("new_pc", "pc"),
  ("new_h5", "h5"),
  ("new_ott", "tv"),
  ("new_others", "other"),
];

/// Plays by followers vs. other viewers (`audience_proportion`).
pub const VIEWERS: &[(&str, &str)] = &[("fans", "followers"), ("guest", "non_followers")];

/// Counters of a compare-table row (`archive_diagnose/compare` `stat`).
const COMPARED: &[(&str, &str)] = &[
  ("coin", "coins"),
  ("dm", "danmaku"),
  ("total_new_attention_cnt", "new_followers"),
  ("unfollow", "lost_followers"),
  ("vt", "watch_minutes"),
];

/// Rates of a compare-table row (basis points) and their names. Of the
/// click-through rate only its rank among similar videos (`tm_pass_rate`) is
/// taken: its value disagrees with `play_analyze` (see `archive.rs`).
const COMPARED_RATES: &[(&str, &str)] = &[
  ("tm_pass_rate", "ctr_beats_similar"),
  ("crash_rate", "three_second_bounce_rate"),
  ("interact_rate", "interaction_rate"),
  ("play_trans_fan_rate", "follow_conversion_rate"),
  ("play_viewer_rate", "non_follower_view_rate"),
  ("play_fan_rate", "follower_watch_rate"),
  ("full_play_ratio", "avg_watch_ratio"),
];

/// Rates that are only 0 while the day's data is not computed yet.
const NOT_YET_ZERO: &[&str] = &["crash_rate", "play_viewer_rate", "full_play_ratio"];

/// One of the account's videos with its creator-center numbers: counters in
/// `metrics`, rates (0..1) and average watch time in `extra`.
pub fn compared(v: &Value) -> Post {
  let mut p = super::video(v);
  let s = v.at("stat");
  p.metrics = Metrics {
    views: s.count("play"),
    likes: s.count("like"),
    comments: s.count("comment"),
    shares: s.count("share"),
    favorites: s.count("fav"),
    ..Metrics::default()
  };
  for (key, name) in COMPARED {
    if let Some(n) = s.count(key).filter(|n| *n > 0 || *key != "vt") {
      p.metrics.other.insert((*name).into(), n);
    }
  }
  for (key, name) in COMPARED_RATES {
    let pending = NOT_YET_ZERO.contains(key) && s.f64(key) == Some(0.0);
    if let Some(rate) = basis_points(s, key).filter(|_| !pending) {
      p.extra.insert((*name).into(), rate);
    }
  }
  if let Some(secs) = s.f64("avg_play_time").filter(|n| *n > 0.0) {
    p.extra.insert("avg_watch_seconds".into(), num(secs));
  }
  p
}
