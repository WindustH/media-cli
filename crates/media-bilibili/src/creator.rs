//! Creator-center analytics of the logged-in account (`insights`): headline
//! numbers against the previous period, daily trends, where plays and
//! followers come from, and the follower portrait.
//!
//! Endpoints are those of the data-center app that
//! member.bilibili.com/platform/data-up embeds (member.bilibili.com/york/data-center-web,
//! `creator-monorepo/data-center-web` bundle `index` and lazy chunks 55, 782,
//! 6099, 8359; 2026-09). Days follow Beijing time; a day's data appears the
//! next noon.

use media_core::{Ctx, Error, ErrorCode, Insights, Page, Post, Result, Value, ValueExt, json};

use crate::account;
use crate::api::{self, Call};
use crate::parse::insights::{self as pi, AGES, DEVICES, GENDERS, VIEWERS};
use crate::refs::{self, Video};

const DATA: &str = "https://member.bilibili.com/x/web/data";
const MEMBER: &str = "https://member.bilibili.com/";

/// A data-center GET (`/x/web/data` + `path`) as the web app sends it.
pub fn data<'a>(ctx: &'a Ctx, path: &str, mid: &str) -> Call<'a> {
  api::get(ctx, &format!("{DATA}{path}"))
    .header("referer", MEMBER)
    .arg("tmid", mid)
}

/// Send, but let a failing side request only leave a note in `errors`
/// (login, captcha and rate limits still abort).
pub async fn soft(call: Call<'_>, name: &str, errors: &mut Vec<Value>) -> Result<Option<Value>> {
  match call.send().await {
    Ok(v) => Ok(Some(v)),
    Err(e) if is_fatal(&e) => Err(e),
    Err(e) => {
      errors.push(json!({ "source": name, "error": e.message }));
      Ok(None)
    }
  }
}

fn is_fatal(e: &Error) -> bool {
  matches!(
    e.code,
    ErrorCode::NotAuthenticated | ErrorCode::VerificationRequired | ErrorCode::RateLimited
  )
}

/// The data center's windows: yesterday, 7, 30 or 90 days, or everything.
#[derive(Debug, Clone, Copy)]
pub struct Period {
  code: i8,
  days: Option<u32>,
}

impl Period {
  /// The smallest window that covers `days`.
  pub fn covering(days: u32) -> Self {
    let (code, days) = match days {
      0..=1 => (-1, Some(1)),
      2..=7 => (0, Some(7)),
      8..=30 => (1, Some(30)),
      31..=90 => (2, Some(90)),
      _ => (3, None),
    };
    Self { code, days }
  }

  fn label(self) -> &'static str {
    match self.code {
      -1 => "yesterday",
      0 => "7d",
      1 => "30d",
      2 => "90d",
      _ => "all",
    }
  }

  /// Trends know no "yesterday" (the web app then shows 7 days).
  fn trend(self) -> i8 {
    self.code.max(0)
  }

  /// Follower numbers know no "everything" (90 days at most).
  fn fans(self) -> i8 {
    self.code.min(2)
  }
}

/// `(data-center key, metric)` of the overview numbers, by the `tab` that serves them.
const TABS: &[(u8, &[(&str, &str)])] = &[
  (
    0,
    &[
      ("play", "views"),
      ("visitor", "profile_visitors"),
      ("fan", "net_followers"),
      ("vt", "watch_minutes"),
    ],
  ),
  (
    1,
    &[("like", "likes"), ("fav", "favorites"), ("coin", "coins")],
  ),
  (
    2,
    &[
      ("comment", "comments"),
      ("dm", "danmaku"),
      ("share", "shares"),
    ],
  ),
];

/// Follower numbers (`v3/fans/stat/num`, `export`) and their metrics.
const FANS: &[(&str, &str)] = &[
  ("total", "followers"),
  ("follow", "new_followers"),
  ("unfollow", "lost_followers"),
  ("active", "active_followers"),
];

pub async fn account(ctx: &Ctx, days: u32) -> Result<Insights> {
  let mid = account::my_mid(ctx).await?;
  let period = Period::covering(days);
  let mut i = Insights {
    kind: "account".into(),
    subject: mid.clone(),
    url: Some(refs::space_url(&mid)),
    ..Insights::default()
  };
  let (mut raw, mut errors) = (serde_json::Map::new(), Vec::new());

  // Headline numbers: three tabs, each with the previous period (`*_last`).
  let mut previous = serde_json::Map::new();
  let mut log_date = None;
  for (tab, keys) in TABS {
    let v = data(ctx, "/v2/overview/stat/num", &mid)
      .arg("period", period.code)
      .arg("s_locale", "zh_CN")
      .arg("tab", tab)
      .send()
      .await?;
    for (key, metric) in *keys {
      i.total(metric, v.f64(key).map(pi::num));
      if let Some(last) = v.f64(&format!("{key}_last")) {
        previous.insert((*metric).into(), pi::num(last));
      }
    }
    log_date = log_date.or_else(|| pi::day(v.at("log_date")));
    raw.insert(format!("overview_stat_num_{tab}"), v);
  }

  // The window: `days` of data up to the last finished day.
  let to = log_date.or_else(|| pi::days_before(&pi::today(), 1));
  let from = match (period.days, &to) {
    (Some(n), Some(to)) => pi::days_before(to, n - 1),
    _ => None,
  };

  let fans = data(ctx, "/v3/fans/stat/num", &mid).arg("period", period.fans());
  if let Some(v) = soft(fans, "fans_stat_num", &mut errors).await? {
    for (key, metric) in FANS {
      i.total(metric, v.f64(&format!("{key}_num")).map(pi::num));
    }
    raw.insert("fans_stat_num".into(), v);
  }

  // Daily trends: all overview metrics in one request (as the web export does).
  let keys: Vec<&str> = TABS
    .iter()
    .flat_map(|(_, k)| k.iter().map(|(k, _)| *k))
    .collect();
  let graph = data(ctx, "/v2/overview/stat/graph", &mid)
    .arg("period", period.trend())
    .arg("s_locale", "zh_CN")
    .arg("type", keys.join(","));
  if let Some(v) = soft(graph, "overview_stat_graph", &mut errors).await? {
    for (_, metrics) in TABS {
      for (key, metric) in *metrics {
        let list = v.list(&format!("data_tendency.{key}"));
        i.series.extend(pi::series(metric, list, from.as_deref()));
      }
    }
    raw.insert("overview_stat_graph".into(), v);
  }
  let export = data(ctx, "/v3/fans/stat/export", &mid).arg("period", period.fans());
  if let Some(v) = soft(export, "fans_stat_export", &mut errors).await? {
    for (key, metric) in FANS {
      let list = v.list(&format!("data_tendency_{key}"));
      i.series.extend(pi::series(metric, list, from.as_deref()));
    }
    raw.insert("fans_stat_export".into(), v);
  }

  sources(ctx, &mid, &mut i, &mut raw, &mut errors).await?;
  portrait(ctx, &mid, &mut i, &mut raw, &mut errors).await?;

  // The watch-time index is only filled for accounts Bilibili enabled it for;
  // over all time the web app shows no visitors, and `fan` is the total.
  let zero = |v: Option<&Value>| v.and_then(Value::as_f64) == Some(0.0);
  let mut drop = Vec::new();
  if zero(i.totals.get("watch_minutes")) && zero(previous.get("watch_minutes")) {
    drop.push("watch_minutes");
  }
  if period.days.is_none() {
    drop.extend(["profile_visitors", "net_followers"]);
  }
  for metric in drop {
    i.totals.remove(metric);
    previous.remove(metric);
    i.series.retain(|s| s.metric != metric);
  }
  i.from = from.or_else(|| {
    let firsts = i.series.iter().filter_map(|s| s.points.first());
    firsts.map(|p| p.date.clone()).min()
  });
  i.to = to;
  i.extra.insert("period".into(), json!(period.label()));
  // Distributions the data center only has for fixed windows.
  let windows = [
    ("device", "30d"),
    ("viewer_type", "30d"),
    ("follow_source", "30d"),
    ("video_last_day", "1d"),
  ];
  let windows: serde_json::Map<String, Value> = windows
    .iter()
    .filter(|(d, _)| i.breakdowns.iter().any(|b| b.dimension == *d))
    .map(|(d, w)| ((*d).to_owned(), json!(w)))
    .collect();
  if !windows.is_empty() {
    i.extra
      .insert("breakdown_windows".into(), Value::Object(windows));
  }
  if !previous.is_empty() {
    i.extra
      .insert("previous_period".into(), Value::Object(previous));
  }
  if !errors.is_empty() {
    i.extra.insert("errors".into(), json!(errors));
  }
  i.raw = Some(Value::Object(raw));
  Ok(i)
}

/// The account's videos side by side with their creator-center numbers
/// (the data center's video comparison, `archive_diagnose/compare`, chunk
/// 2853): the latest `size` ones, or the given ones.
pub async fn compare(ctx: &Ctx, videos: &[Video], size: usize) -> Result<Page<Post>> {
  let mid = account::my_mid(ctx).await?;
  let mut call = data(ctx, "/archive_diagnose/compare", &mid).arg("size", size);
  if !videos.is_empty() {
    let ids: Vec<&str> = videos.iter().map(|v| v.bvid.as_str()).collect();
    call = call.arg("compare_bvids", ids.join(","));
  }
  let v = call.send().await?;
  Ok(Page::last(
    v.list("list").iter().map(pi::compared).collect(),
  ))
}

/// Where plays come from (terminal, followers or not, which videos) and where
/// new followers found the account.
async fn sources(
  ctx: &Ctx,
  mid: &str,
  i: &mut Insights,
  raw: &mut serde_json::Map<String, Value>,
  errors: &mut Vec<Value>,
) -> Result<()> {
  let call = data(ctx, "/v2/overview/source", mid).arg("s_locale", "zh_CN");
  if let Some(v) = soft(call, "overview_source", errors).await? {
    // Terminal shares come in basis points, followers / others as plays.
    let device = pi::fields(v.at("play_proportion"), DEVICES)
      .into_iter()
      .map(|(label, bp)| (label, bp / 10_000.0));
    i.breakdowns.extend(pi::breakdown("device", device, true));
    let viewers = pi::fields(v.at("audience_proportion"), VIEWERS);
    i.breakdowns
      .extend(pi::breakdown("viewer_type", viewers, true));
    // Plays of each video on the last day of data.
    let videos = pi::pairs(v.list("single_arc_inc"), "title", "incr");
    i.breakdowns
      .extend(pi::breakdown("video_last_day", videos, true));
    raw.insert("overview_source".into(), v);
  }
  let call = data(ctx, "/v2/fans/stat/source", mid);
  if let Some(v) = soft(call, "fans_stat_source", errors).await? {
    let items = v
      .as_object()
      .into_iter()
      .flatten()
      .filter_map(|(k, n)| Some((k.clone(), n.as_f64()?)));
    i.breakdowns
      .extend(pi::breakdown("follow_source", items, true));
    raw.insert("fans_stat_source".into(), v);
  }
  Ok(())
}

/// Age, gender, region, interests and active hours of the followers.
async fn portrait(
  ctx: &Ctx,
  mid: &str,
  i: &mut Insights,
  raw: &mut serde_json::Map<String, Value>,
  errors: &mut Vec<Value>,
) -> Result<()> {
  let call = data(ctx, "/v3/fans/stat/portrayal", mid).arg("type", "all");
  let Some(v) = soft(call, "fans_stat_portrayal", errors).await? else {
    return Ok(());
  };
  i.breakdowns.extend(pi::breakdown(
    "gender",
    pi::fields(v.at("fans_gender"), GENDERS),
    false,
  ));
  i.breakdowns.extend(pi::breakdown(
    "age",
    pi::fields(v.at("fans_age"), AGES),
    false,
  ));
  let region = pi::pairs(v.list("viewer_area"), "location", "count");
  i.breakdowns.extend(pi::breakdown("region", region, true));
  let mut interest = pi::pairs(v.list("viewer_ty"), "tag_name", "count");
  interest.sort_by(|a, b| b.1.total_cmp(&a.1));
  interest.truncate(10);
  i.breakdowns
    .extend(pi::breakdown("interest", interest, true));
  let mut hours: Vec<(u64, f64)> = v
    .list("fans_activity")
    .iter()
    .filter_map(|h| Some((h.u64("hour_key")?, h.f64("total_inc")?)))
    .collect();
  hours.sort_by_key(|(h, _)| *h);
  let hours = hours.into_iter().map(|(h, n)| (format!("{h:02}"), n));
  i.breakdowns
    .extend(pi::breakdown("active_hour", hours, false));
  i.extra.insert("portrait".into(), json!("followers"));
  raw.insert("fans_stat_portrayal".into(), v);
  Ok(())
}
