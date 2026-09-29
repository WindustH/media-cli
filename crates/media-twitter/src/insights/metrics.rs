//! Analytics payloads → parts of `Insights`: metric names, totals, daily
//! series and audience breakdowns. Shapes and metric lists follow the Relay
//! queries of the web client's analytics pages (`bundle.AccountAnalytics`,
//! see `graphql.rs`).

use std::collections::{BTreeMap, BTreeSet};

use media_core::{Breakdown, Insights, Point, Series, Share, Value, ValueExt};

/// Labels a dimension value of an audience row.
type Label = fn(&str) -> String;

/// `overviewDataUserQuery`: the client's list plus `Unfollows` of its follower chart.
pub const ACCOUNT_METRICS: &[&str] = &[
  "Engagements",
  "Impressions",
  "ProfileVisits",
  "Follows",
  "Unfollows",
  "VideoViews",
  "Replies",
  "Likes",
  "Retweets",
  "MediaViews",
  "Bookmark",
  "Share",
  "UrlClicks",
  "CreateTweet",
  "CreateQuote",
];
/// `overviewDataPostQuery`.
pub const POST_SERIES_METRICS: &[&str] = &[
  "Engagements",
  "Impressions",
  "ProfileVisits",
  "Follows",
  "VideoViews",
  "Replies",
  "Likes",
  "Retweets",
  "MediaViews",
  "Bookmark",
  "Share",
  "UrlClicks",
];
/// `postDetailsProviderMetricsTotalQuery`: the post page's list plus the content tab's clicks.
pub const POST_METRICS: &[&str] = &[
  "Impressions",
  "Engagements",
  "ProfileVisits",
  "Follows",
  "Replies",
  "Likes",
  "Retweets",
  "Bookmark",
  "Share",
  "MediaViews",
  "VideoViews",
  "DetailExpands",
  "UrlClicks",
  "HashtagClicks",
  "PermalinkClicks",
];
/// All the free rollup accepts ("The metricType is not allowed in Free analytics").
pub const FREE_METRICS: &[&str] = &[
  "Impressions",
  "Likes",
  "ProfileVisits",
  "Follows",
  "Replies",
  "Retweets",
];
/// Audience dimensions of the analytics pages.
pub const DIMENSIONS: &[&str] = &[
  "Age",
  "Gender",
  "EngagementType",
  "ClientAppId",
  "IsInNetwork",
];

/// Upstream metric → our key, where snake_case of the upstream name would not do.
const RENAMED: &[(&str, &str)] = &[
  ("Impressions", "views"),
  ("Replies", "comments"),
  ("Retweets", "shares"),
  ("Bookmark", "favorites"),
  ("Share", "link_shares"),
  ("Follows", "new_followers"),
  ("Unfollows", "lost_followers"),
  ("UrlClicks", "link_clicks"),
  ("CreateTweet", "posts"),
  ("CreateQuote", "quote_posts"),
];

/// Engagement types of the audience rows (the client's engagement set) → our keys.
const ENGAGEMENTS: &[(&str, &str)] = &[
  ("Fav", "likes"),
  ("Reply", "comments"),
  ("Retweet", "shares"),
  ("QuoteTweet", "quotes"),
  ("Bookmark", "favorites"),
  ("Share", "link_shares"),
  ("ProfilePic", "profile_visits"),
];

/// Our key of an upstream metric name.
pub fn key(metric: &str) -> String {
  if let Some((_, k)) = RENAMED.iter().find(|(m, _)| *m == metric) {
    return (*k).to_owned();
  }
  let mut out = String::new();
  for (i, c) in metric.chars().enumerate() {
    if c.is_uppercase() && i > 0 {
      out.push('_');
    }
    out.extend(c.to_lowercase());
  }
  out
}

/// `extra.upstream_metrics`: X's names of the keys that are not plain snake_case of them.
pub fn upstream_names(insights: &mut Insights) {
  let names: serde_json::Map<String, Value> = RENAMED
    .iter()
    .filter(|(_, k)| insights.totals.contains_key(*k))
    .map(|(m, k)| ((*k).to_owned(), (*m).into()))
    .collect();
  if !names.is_empty() {
    insights
      .extra
      .insert("upstream_metrics".into(), Value::Object(names));
  }
}

/// `[{metric_type, metric_value}]` → our keys; a listed metric without a value is 0.
pub fn totals(list: &[Value]) -> BTreeMap<String, u64> {
  list
    .iter()
    .filter_map(|m| {
      let name = m.str("metric_type")?;
      Some((key(&name), m.count("metric_value").unwrap_or(0)))
    })
    .collect()
}

/// Rows `[{metric_values, timestamp: {iso8601_time}}]` → one daily series per
/// metric (missing days as 0) and the sum of each.
pub fn series(rows: &[Value]) -> (Vec<Series>, BTreeMap<String, u64>) {
  let mut days: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
  let mut metrics = BTreeSet::new();
  for row in rows {
    let Some(time) = row.str("timestamp.iso8601_time") else {
      continue;
    };
    let date = time.get(..10).unwrap_or(&time).to_owned();
    let values = days.entry(date).or_default();
    for (k, v) in totals(row.list("metric_values")) {
      *values.entry(k.clone()).or_default() += v;
      metrics.insert(k);
    }
  }
  let mut sums = BTreeMap::new();
  let series = metrics
    .into_iter()
    .map(|metric| {
      let points = days
        .iter()
        .map(|(date, values)| {
          let v = values.get(&metric).copied().unwrap_or(0);
          *sums.entry(metric.clone()).or_default() += v;
          Point {
            date: date.clone(),
            value: v.into(),
          }
        })
        .collect();
      Series { metric, points }
    })
    .collect();
  (series, sums)
}

/// Put numbers into `totals`.
pub fn put_totals(insights: &mut Insights, values: BTreeMap<String, u64>) {
  for (k, v) in values {
    insights.total(&k, Value::from(v));
  }
}

/// `engagement_rate` (engagements / views) and `net_followers`, where their parts are known.
pub fn derive(insights: &mut Insights) {
  let n = |k: &str| insights.totals.get(k).and_then(Value::as_u64);
  let rate = match (n("engagements"), n("views")) {
    (Some(e), Some(v)) if v > 0 => Some((e as f64 / v as f64 * 10_000.0).round() / 10_000.0),
    _ => None,
  };
  let net = match (n("new_followers"), n("lost_followers")) {
    (Some(a), Some(b)) => Some(a as i64 - b as i64),
    _ => None,
  };
  insights.total("engagement_rate", rate.map(Value::from));
  insights.total("net_followers", net.map(Value::from));
}

/// Audience rows (`uec_metrics_daily_time_series_count` over [`DIMENSIONS`] and
/// `uec_country_metrics_daily_time_series_count`): who saw the posts (age,
/// gender, device, network, country; impressions) and engagements by type.
pub fn audience(rows: &[Value], countries: &[Value]) -> Vec<Breakdown> {
  let seen = |r: &&Value| r.str("engagement_type").as_deref() == Some("Displayed");
  let viewed: Vec<&Value> = rows.iter().filter(seen).collect();
  let dims: [(&str, &str, Label); 4] = [
    ("age", "age", age),
    ("gender", "gender", |g| g.to_lowercase()),
    ("device", "client_app_id", device),
    ("network", "is_in_network", |n| {
      if n == "true" {
        "in_network"
      } else {
        "out_of_network"
      }
      .into()
    }),
  ];
  let mut out = Vec::new();
  for (dimension, field, label) in dims {
    let items = viewed
      .iter()
      .filter_map(|r| Some((label(&r.str(field)?), count(r))));
    push(&mut out, dimension, items, true);
  }
  let items = countries
    .iter()
    .filter(seen)
    .filter_map(|r| Some((r.str("country")?.to_uppercase(), count(r))));
  push(&mut out, "country", items, true);
  let items = rows.iter().filter_map(|r| {
    let t = r.str("engagement_type")?;
    let (_, k) = ENGAGEMENTS.iter().find(|(e, _)| *e == t)?;
    Some(((*k).to_owned(), count(r)))
  });
  push(&mut out, "engagement_type", items, true);
  out
}

/// Hourly rows of `organic_metrics_time_series` → views by hour of the day (UTC).
pub fn hours(rows: &[Value]) -> Option<Breakdown> {
  let items = rows.iter().filter_map(|row| {
    let hour = row.str("timestamp.iso8601_time")?.get(11..13)?.to_owned();
    Some((
      hour,
      totals(row.list("metric_values")).get("views").copied()?,
    ))
  });
  let mut out = Vec::new();
  push(&mut out, "hour_utc", items, false);
  out.pop()
}

fn count(row: &Value) -> u64 {
  row.count("count").unwrap_or(0)
}

/// Sum `items` by label into a breakdown (largest first when `rank`), unless all are zero.
fn push(
  out: &mut Vec<Breakdown>,
  dimension: &str,
  items: impl Iterator<Item = (String, u64)>,
  rank: bool,
) {
  let mut sums: BTreeMap<String, u64> = BTreeMap::new();
  for (label, v) in items {
    *sums.entry(label).or_default() += v;
  }
  let total: u64 = sums.values().sum();
  if total == 0 {
    return;
  }
  let mut items: Vec<Share> = sums
    .into_iter()
    .map(|(label, v)| Share {
      label,
      value: v.into(),
      ratio: Some((v as f64 / total as f64 * 10_000.0).round() / 10_000.0),
      ..Default::default()
    })
    .collect();
  if rank {
    items.sort_by_key(|s| std::cmp::Reverse(s.value.as_u64()));
  }
  out.push(Breakdown {
    dimension: dimension.into(),
    items,
    ..Default::default()
  });
}

/// `Age18To24` / `age18to24` → `18-24`, `…Over65` → `65+`.
fn age(a: &str) -> String {
  let a = a.to_lowercase();
  let a = a.trim_start_matches("age");
  if let Some(n) = a.strip_prefix("over") {
    return format!("{n}+");
  }
  a.replace("to", "-")
}

/// Client app ids of the web client's device chart.
fn device(id: &str) -> String {
  match id {
    "129032" | "191841" | "557701" => "ios",
    "258901" => "android",
    "3033300" => "web",
    _ => "other",
  }
  .into()
}

/// A breakdown of ranked `(label, value)` pairs, e.g. the account's top posts.
pub fn ranked(dimension: &str, items: Vec<(String, u64)>) -> Option<Breakdown> {
  let mut out = Vec::new();
  push(&mut out, dimension, items.into_iter(), true);
  out.pop()
}
