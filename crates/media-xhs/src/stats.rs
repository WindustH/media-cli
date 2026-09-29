//! Creator-center analytics payloads → parts of `Insights`.
//!
//! The creator center requests them with `transform: true` (snake_case on the
//! wire, camelCase in its UI code); payloads go through [`parse::snake_keys`]
//! first, so both spellings work here. Negative numbers mean "not available".

use media_core::text::{from_millis, from_unix, parse_count};
use media_core::{Breakdown, Insights, Point, Post, Series, Share, User, Value, ValueExt, json};

use crate::parse;
use crate::refs::{SOURCE_FEED, note_url, user_url};

/// How the creator center reports a number.
#[derive(Clone, Copy)]
enum Unit {
  Count,
  /// Percent (12.5 = 12.5 %), stored as a 0..1 fraction.
  Percent,
  /// Already a 0..1 fraction.
  Fraction,
  Seconds,
}

use Unit::{Count, Fraction, Percent, Seconds};

/// An upstream number: total key, key of its daily list (`""`: none), our name, unit.
pub struct Metric(&'static str, &'static str, &'static str, Unit);

/// Account overview (`account/base`, per window), as the page's cards name them (chunk 3074).
#[rustfmt::skip]
pub const ACCOUNT: &[Metric] = &[
  Metric("impl_count",              "impl_count_list",              "impressions",       Count),
  Metric("view_count",              "view_list",                    "views",             Count),
  Metric("cover_click_rate",        "cover_click_rate_list",        "ctr",               Percent),
  Metric("avg_view_time",           "avg_view_time_list",           "avg_watch_seconds", Seconds),
  // 观看总时长
  Metric("view_time_avg",           "view_time_list",               "watch_seconds",     Seconds),
  Metric("video_full_view_rate",    "video_full_view_rate_list",    "completion_rate",   Percent),
  Metric("like_count",              "like_list",                    "likes",             Count),
  Metric("comment_count",           "comment_list",                 "comments",          Count),
  Metric("collect_count",           "collect_list",                 "favorites",         Count),
  Metric("share_count",             "share_list",                   "shares",            Count),
  Metric("danmaku_count",           "danmaku_list",                 "danmaku",           Count),
  Metric("quote_count",             "quote_list",                   "quotes",            Count),
  Metric("net_rise_fans_count",     "net_rise_fans_count_list",     "net_followers",     Count),
  Metric("rise_fans_count",         "rise_fans_list",               "new_followers",     Count),
  Metric("loss_fans_count",         "loss_fans_count_list",         "lost_followers",    Count),
  Metric("home_view_count",         "home_view_list",               "profile_views",     Count),
  Metric("home_conversion_rise_fans_rate", "home_conversion_rise_fans_rate_list", "profile_follow_rate", Percent),
  Metric("publish_note_num",        "publish_note_num_list",        "posts",             Count),
  Metric("publish_video_note_num",  "publish_video_note_num_list",  "video_posts",       Count),
  Metric("publish_normal_note_num", "publish_normal_note_num_list", "image_posts",       Count),
];

/// Fans overview (`fans/overall_new`, per window; chunk 7763).
#[rustfmt::skip]
pub const FANS: &[Metric] = &[
  Metric("fans_count",       "fans_list",       "followers",      Count),
  Metric("rise_fans_count",  "rise_fans_list",  "new_followers",  Count),
  Metric("leave_fans_count", "leave_fans_list", "lost_followers", Count),
];

/// One note (`note/base`): lifetime totals, daily lists under `day` (chunks 4810 / 5111).
#[rustfmt::skip]
pub const NOTE: &[Metric] = &[
  Metric("impl_count",                "imp_list",              "impressions",       Count),
  Metric("view_count",                "view_list",             "views",             Count),
  Metric("cover_click_rate",          "cover_click_rate_list", "ctr",               Percent),
  Metric("view_time_avg_with_double", "view_time_list",        "avg_watch_seconds", Seconds),
  Metric("view_time_avg",             "",                      "avg_watch_seconds", Seconds),
  Metric("full_view_rate",            "finish_list",           "completion_rate",   Percent),
  Metric("exit_view2s_rate",          "exit_view2s_list",      "exit_2s_rate",      Percent),
  Metric("rise_fans_count",           "rise_fans_list",        "new_followers",     Count),
  Metric("like_count",                "like_list",             "likes",             Count),
  Metric("comment_count",             "comment_list",          "comments",          Count),
  Metric("collect_count",             "collect_list",          "favorites",         Count),
  Metric("share_count",               "share_list",            "shares",            Count),
  Metric("danmaku_count",             "danmaku_list",          "danmaku",           Count),
  // What fans account for (粉丝占比), or their own rate / time (粉丝 x%).
  Metric("impl_count_rate_with_fans",  "", "impressions_fan_ratio", Percent),
  Metric("view_rate_with_fans",        "", "views_fan_ratio",       Percent),
  Metric("cover_click_rate_with_fans", "", "fan_ctr",               Percent),
  Metric("view_time_avg_with_fans",    "", "fan_avg_watch_seconds", Seconds),
  Metric("full_view_rate_with_fans",   "", "fan_completion_rate",   Percent),
  Metric("exit_view2s_rate_with_fans", "", "fan_exit_2s_rate",      Percent),
  Metric("like_rate_with_fans",        "", "likes_fan_ratio",       Percent),
  Metric("comment_rate_with_fans",     "", "comments_fan_ratio",    Percent),
  Metric("collect_rate_with_fans",     "", "favorites_fan_ratio",   Percent),
  Metric("share_rate_with_fans",       "", "shares_fan_ratio",      Percent),
  Metric("danmaku_rate_with_fans",     "", "danmaku_fan_ratio",     Percent),
];

/// A number in our unit; `None` when missing or negative ("not available").
fn number(v: &Value, unit: Unit) -> Option<Value> {
  let n = match v {
    Value::Number(n) => n.as_f64()?,
    Value::String(s) => {
      let s = s.trim().trim_end_matches('%');
      s.parse()
        .ok()
        .or_else(|| parse_count(s).map(|c| c as f64))?
    }
    _ => return None,
  };
  if n < 0.0 || !n.is_finite() {
    return None;
  }
  Some(match unit {
    Percent => json!(round(n / 100.0)),
    Fraction => json!(round(n)),
    Count | Seconds if n.fract() == 0.0 => json!(n as u64),
    Count | Seconds => json!(round(n)),
  })
}

fn round(x: f64) -> f64 {
  (x * 1e6).round() / 1e6
}

/// Headline numbers of `block`; a metric already set is kept.
pub fn totals(ins: &mut Insights, block: &Value, metrics: &[Metric]) {
  for Metric(key, _, name, unit) in metrics {
    if !ins.totals.contains_key(*name) {
      ins.total(name, number(block.at(key), *unit));
    }
  }
}

/// Daily series from the `{date, count}` lists of `block`, oldest first; a
/// metric already present is kept. `since` drops earlier days (`YYYY-MM-DD`).
pub fn series(ins: &mut Insights, block: &Value, metrics: &[Metric], since: Option<&str>) {
  for Metric(_, list, name, unit) in metrics {
    if list.is_empty() || ins.series.iter().any(|s| s.metric == *name) {
      continue;
    }
    let mut points: Vec<Point> = block
      .list(list)
      .iter()
      .filter_map(|p| {
        Some(Point {
          date: day(p.i64("date")?)?,
          value: ["count_with_double", "count"]
            .iter()
            .find_map(|k| number(p.at(k), *unit))?,
        })
      })
      .filter(|p| since.is_none_or(|s| p.date.as_str() >= s))
      .collect();
    points.sort_by(|a, b| a.date.cmp(&b.date));
    if !points.is_empty() {
      ins.series.push(Series {
        metric: (*name).to_owned(),
        points,
      });
    }
  }
}

/// The Beijing-time day (`YYYY-MM-DD`) of a Unix time in seconds or milliseconds.
pub fn day(t: i64) -> Option<String> {
  let ms = from_unix(t)?.as_millisecond() + 8 * 3_600_000;
  Some(from_millis(ms)?.to_string().get(..10)?.to_owned())
}

/// A distribution of `[{title, value}]` items. With `percent` the values are
/// percentages; otherwise ratios come from their sum.
pub fn breakdown(ins: &mut Insights, dimension: &str, items: &[Value], percent: bool) {
  let rows: Vec<(String, f64)> = items
    .iter()
    .filter_map(|i| {
      let label = i.first_str(&["title", "name", "label"])?;
      let value = ["value_with_double", "value", "count", "rate"]
        .iter()
        .find_map(|k| i.f64(k))?;
      Some((label, value))
    })
    .collect();
  let sum: f64 = rows.iter().map(|(_, v)| v).sum();
  let items: Vec<Share> = rows
    .into_iter()
    .map(|(label, value)| Share {
      label: english(dimension, label),
      ratio: match percent {
        true => Some(round(value / 100.0)),
        false => (sum > 0.0).then(|| round(value / sum)),
      },
      value: number(&json!(value), Count).unwrap_or(Value::Null),
      ..Default::default()
    })
    .collect();
  if !items.is_empty() {
    ins.breakdowns.push(Breakdown {
      dimension: dimension.to_owned(),
      items,
      ..Default::default()
    });
  }
}

/// Fixed upstream labels in English; everything else as the platform names it.
fn english(dimension: &str, label: String) -> String {
  match (dimension, label.as_str()) {
    ("gender", "男") => "male".into(),
    ("gender", "女") => "female".into(),
    _ => label,
  }
}

/// `analyse_infos` (诊断): how the account or note ranks among similar ones;
/// `ratio` is the share of similar creators / notes it beats.
pub fn diagnosis(ins: &mut Insights, dimension: &str, infos: &[Value]) {
  let items: Vec<Share> = infos
    .iter()
    .filter_map(|i| {
      let quota = i.str("quota")?;
      Some(Share {
        label: quota_name(&quota),
        value: number(i.at("count"), Count).unwrap_or(Value::Null),
        ratio: i.f64("scale").filter(|s| *s >= 0.0).map(round),
        ..Default::default()
      })
    })
    .collect();
  if !items.is_empty() {
    ins.breakdowns.push(Breakdown {
      dimension: dimension.to_owned(),
      items,
      ..Default::default()
    });
  }
}

fn quota_name(quota: &str) -> String {
  match quota {
    "readFeed" => "views",
    "fansInc" | "followFromDiscovery" => "new_followers",
    "homeView" => "profile_views",
    "publishNote" => "posts",
    "engage" | "interactRate" | "noteEngage" => "engagement",
    "clickRate" => "ctr",
    "viewTimeAvg" => "avg_watch_seconds",
    "fullViewRate" => "completion_rate",
    "fullView5sRate" => "completion_5s_rate",
    "contentRich" => "content_richness",
    "vqa" => "video_quality",
    other => return parse::snake(other),
  }
  .to_owned()
}

// ── data center rows ────────────────────────────────────────────────────

/// A note of the 内容分析 list (columns of chunk 2179): `read_count` is the
/// views column, `cover_click_rate` a fraction there.
pub fn note_row(v: &Value) -> Option<Post> {
  let id = v.first_str(&["id", "note_id"])?;
  let mut post = Post {
    kind: if v.i64("type") == Some(2) {
      "video"
    } else {
      "note"
    }
    .into(),
    title: v.str("title"),
    url: Some(note_url(&id, None, SOURCE_FEED)),
    created_at: v.i64("post_time").and_then(from_unix),
    raw: Some(v.clone()),
    ..Post::default()
  };
  let m = &mut post.metrics;
  m.views = v.u64("read_count");
  m.likes = v.u64("like_count");
  m.comments = v.u64("comment_count");
  m.favorites = v.u64("fav_count");
  m.shares = v.u64("share_count");
  for (key, name) in [
    ("imp_count", "impressions"),
    ("increase_fans_count", "new_followers"),
    ("danmaku_count", "danmaku"),
  ] {
    if let Some(n) = v.u64(key) {
      m.other.insert(name.into(), n);
    }
  }
  let rates = [
    ("ctr", number(v.at("cover_click_rate"), Fraction)),
    ("avg_watch_seconds", number(v.at("view_time_avg"), Seconds)),
    ("cover", v.str("cover_url").map(Value::from)),
  ];
  for (name, value) in rates {
    if let Some(x) = value {
      post.extra.insert(name.into(), x);
    }
  }
  post.id = id;
  Some(post)
}

/// A fan of 我的活跃粉丝: `{user_id, name, url (avatar), count (interactions)}`.
pub fn active_fan(v: &Value) -> Option<User> {
  let id = v.first_str(&["user_id", "id"])?;
  let mut user = User {
    name: v.str("name").unwrap_or_default(),
    avatar: v.str("url"),
    url: Some(user_url(&id)),
    raw: Some(v.clone()),
    id,
    ..User::default()
  };
  if let Some(n) = v.u64("count") {
    user.stats.other.insert("interactions".into(), n);
  }
  Some(user)
}
