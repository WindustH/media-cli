//! Creator-center statistics: headline numbers, daily trends, audience
//! portraits and the rows of the content list.
//!
//! Zhihu has two positive reactions: 赞同 (upvote, our `likes`, as on posts)
//! and 喜欢 (like, our `hearts`). A pin's 赞同 is counted as `reaction`.

use media_core::text::html_to_text;
use media_core::{Breakdown, Insights, Metrics, Point, Post, Series, Share, Value, ValueExt};

use super::time;
use crate::refs::Target;

/// Our metric and the upstream fields (dotted paths) to try in order.
type Table = &'static [(&'static str, &'static [&'static str])];

const CONTENT: Table = &[
  ("views", &["pv"]),
  ("plays", &["play"]),
  ("impressions", &["show"]),
  ("likes", &["upvote", "new_upvote"]),
  ("hearts", &["like", "new_like"]),
  ("reactions", &["reaction"]),
  ("comments", &["comment"]),
  ("favorites", &["collect"]),
  ("shares", &["share"]),
  ("reposts", &["re_pin"]),
  ("likes_added", &["incr_upvote_num", "new_incr_upvote_num"]),
  ("likes_removed", &["desc_upvote_num", "new_desc_upvote_num"]),
  ("hearts_added", &["incr_like_num", "new_incr_like_num"]),
  ("hearts_removed", &["desc_like_num", "new_desc_like_num"]),
  ("posts_published", &["publish_cnt"]),
  ("ctr", &["click_rate", "rec_click_rate"]),
  (
    "completion_rate",
    &["read_finished_rate", "advanced.finish_read_percent"],
  ),
  ("play_completion_rate", &["play_finished_rate"]),
  ("avg_read_seconds", &["avg_read_duration"]),
  ("unique_visitors", &["pageshow_uv"]),
  (
    "new_followers",
    &["advanced.follower_translate", "new_follow_uv"],
  ),
  ("follow_rate", &["follower_conversion_rate"]),
  (
    "engagement_rate",
    &[
      "positive_interact_rate",
      "advanced.positive_interact_percent",
    ],
  ),
];

/// On pins the creator center shows `reaction` as 赞同 and `new_like` as 喜欢.
const PIN: Table = &[
  ("likes", &["reaction", "upvote"]),
  ("hearts", &["new_like", "like"]),
];

const FOLLOWERS: Table = &[
  ("new_followers", &["new_follow"]),
  ("lost_followers", &["unfollow"]),
  ("net_followers", &["net_increase_follow"]),
  ("profile_visitors", &["homepage_visitor_num"]),
  ("profile_follows", &["homepage_follow_cnt"]),
  ("profile_follow_rate", &["homepage_conversion_rate"]),
];

/// Trends worth a series; the rest stay in the totals.
const TRENDS: &[&str] = &[
  "views",
  "impressions",
  "likes",
  "hearts",
  "comments",
  "favorites",
  "shares",
  "ctr",
];
/// Trends shown only when they are not all zero (video plays, pin reactions ...).
const SPARSE_TRENDS: &[&str] = &[
  "plays",
  "reactions",
  "reposts",
  "avg_read_seconds",
  "new_followers",
];
const FOLLOWER_TRENDS: &[&str] = &[
  "new_followers",
  "lost_followers",
  "net_followers",
  "profile_visitors",
];

/// Upstream names of the metrics whose mapping is not obvious, for `extra`.
pub const NAMES: &[(&str, &str)] = &[
  ("likes", "赞同 (upvote; reaction on pins)"),
  ("hearts", "喜欢 (like)"),
  ("reactions", "想法赞同 (reaction)"),
  ("reposts", "转发 (re_pin)"),
  ("impressions", "展现 (show)"),
  ("unique_visitors", "pageshow_uv"),
  ("new_followers", "关注者转化 / 新增关注者"),
  ("engagement_rate", "正向互动率"),
];

fn lookup(table: Table, name: &str) -> Option<&'static [&'static str]> {
  table.iter().find(|(n, _)| *n == name).map(|(_, f)| *f)
}

fn fields(name: &str, pin: bool) -> &'static [&'static str] {
  let own = if pin { lookup(PIN, name) } else { None };
  own.or_else(|| lookup(CONTENT, name)).unwrap_or(&[])
}

/// A number at `path`; `"12.5%"` becomes the fraction 0.125, `NaN%` nothing.
pub fn number(v: &Value, path: &str) -> Option<Value> {
  match v.at(path) {
    Value::Number(n) => Some(Value::Number(n.clone())),
    Value::String(s) => {
      let (digits, scale) = match s.trim().strip_suffix('%') {
        Some(d) => (d, 100.0),
        None => (s.trim(), 1.0),
      };
      let f = digits.parse::<f64>().ok().filter(|f| f.is_finite())?;
      serde_json::Number::from_f64(f / scale).map(Value::Number)
    }
    _ => None,
  }
}

fn first(v: &Value, paths: &[&str]) -> Option<Value> {
  paths.iter().find_map(|p| number(v, p))
}

/// Headline numbers of a creator `aggr` object (account or one post).
pub fn totals(ins: &mut Insights, v: &Value, pin: bool) {
  for (name, _) in CONTENT {
    ins.total(name, first(v, fields(name, pin)));
  }
}

/// Follower changes summed over the daily rows of `follow/detail/v2`.
pub fn follower_totals(ins: &mut Insights, rows: &[Value]) {
  for (name, paths) in &FOLLOWERS[..5] {
    // Net changes can be negative.
    let values: Vec<i64> = rows.iter().filter_map(|r| r.i64(paths[0])).collect();
    if !values.is_empty() {
      ins.total(name, Some(values.iter().sum::<i64>().into()));
    }
  }
}

/// Daily trends of content rows (`p_date` + counters), oldest first.
pub fn series(rows: &[Value], pin: bool) -> Vec<Series> {
  trends(rows, TRENDS, SPARSE_TRENDS, |n| fields(n, pin))
}

/// Daily follower changes from the rows of `follow/detail/v2`.
pub fn follower_series(rows: &[Value]) -> Vec<Series> {
  trends(rows, FOLLOWER_TRENDS, &[], |n| {
    lookup(FOLLOWERS, n).unwrap_or(&[])
  })
}

/// Series of `names` (and of `sparse` ones unless all zero); rows without a
/// value for a day leave that day out.
fn trends(
  rows: &[Value],
  names: &[&str],
  sparse: &[&str],
  paths: impl Fn(&str) -> &'static [&'static str],
) -> Vec<Series> {
  let mut rows: Vec<&Value> = rows.iter().filter(|r| date(r).is_some()).collect();
  rows.sort_by_key(|r| date(r));
  let mut out = Vec::new();
  for name in names.iter().chain(sparse) {
    let paths = paths(name);
    let points: Vec<Point> = rows
      .iter()
      .filter_map(|r| {
        Some(Point {
          date: date(r)?,
          value: first(r, paths)?,
        })
      })
      .collect();
    let zero = points.iter().all(|p| p.value.as_f64() == Some(0.0));
    if points.is_empty() || (zero && sparse.contains(name)) {
      continue;
    }
    out.push(Series {
      metric: (*name).into(),
      points,
    });
  }
  out
}

fn date(row: &Value) -> Option<String> {
  row
    .first_str(&["p_date", "date", "statistics_date"])
    .map(|d| d.chars().take(10).collect())
}

/// A portrait object (`{source: [{name, value, real_value}], gender: [...], ...}`)
/// as breakdowns; `prefix` tells follower portraits apart from reader ones.
pub fn portrait(v: &Value, prefix: &str) -> Vec<Breakdown> {
  let Value::Object(m) = v else { return vec![] };
  let mut out = Vec::new();
  for (key, list) in m {
    let items: Vec<Share> = list
      .as_array()
      .into_iter()
      .flatten()
      .filter_map(|x| {
        let label = x.first_str(&["name", "title"])?;
        let ratio = x.f64("value");
        Some(Share {
          label: gender(&label).unwrap_or(label),
          value: x.at("real_value").clone(),
          ratio,
        })
      })
      .map(|mut s| {
        if s.value.is_null() {
          s.value = s.ratio.map(Value::from).unwrap_or_default();
        }
        s
      })
      .collect();
    if items.is_empty() {
      continue;
    }
    let dimension = match key.as_str() {
      "source" => "traffic_source",
      "location" => "region",
      other => other,
    };
    out.push(Breakdown {
      dimension: format!("{prefix}{dimension}"),
      items,
    });
  }
  out
}

fn gender(label: &str) -> Option<String> {
  match label {
    "男" => Some("male".into()),
    "女" => Some("female".into()),
    _ => None,
  }
}

/// Where the creator center keeps a post's title (a pin's is inside `pin_content`).
pub const TITLE: [&str; 4] = ["title", "excerpt_title", "pin_content.0.title", "excerpt"];

/// A row of the creator content list: the post with its lifetime numbers.
pub fn creation(v: &Value, kind: &str) -> Post {
  let obj = v.at(kind);
  let id = obj.first_str(&["url_token", "id"]).unwrap_or_default();
  let target = match kind {
    "article" => Some(Target::Article(id.clone())),
    "pin" => Some(Target::Pin(id.clone())),
    "answer" => Some(Target::Answer(id.clone())),
    _ => None,
  };
  let mut p = Post {
    kind: kind.into(),
    title: obj.first_str(&TITLE[..3]).filter(|t| !t.trim().is_empty()),
    text: obj.str("excerpt").map(|t| html_to_text(&t)),
    url: target.map(|t| t.url()),
    created_at: time(obj, &["created_time", "created", "create_time"]),
    updated_at: time(obj, &["updated_time", "updated"]),
    raw: Some(v.clone()),
    id,
    ..Post::default()
  };
  let pin = kind == "pin";
  let count = |name: &str| first(v, fields(name, pin)).and_then(|n| n.as_u64());
  p.metrics = Metrics {
    views: count("views"),
    likes: count("likes"),
    comments: count("comments"),
    shares: count("shares"),
    favorites: count("favorites"),
    ..Metrics::default()
  };
  for name in ["plays", "impressions", "hearts", "reposts", "new_followers"] {
    if let Some(n) = count(name).filter(|n| *n > 0 || name != "plays") {
      p.metrics.other.insert(name.into(), n);
    }
  }
  for name in ["ctr", "engagement_rate", "follow_rate", "avg_read_seconds"] {
    if let Some(x) = first(v, fields(name, pin)) {
      p.extra.insert(name.into(), x);
    }
  }
  p
}
