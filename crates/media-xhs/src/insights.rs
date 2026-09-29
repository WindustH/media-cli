//! Creator-center analytics (数据看板, creator.xiaohongshu.com/statistics):
//! account overview, fans, and the data of one of your notes.
//!
//! Paths come from the API list in the creator center's `index.<hash>.js`;
//! the lazy chunks of its pages call them as galaxy GETs (3074: account
//! overview, 7763: fans, 4323 / 5111: note analysis). [`stats`] maps payloads.

use media_core::{Error, ErrorCode, Insights, Result, Value, ValueExt, json};

use crate::api::Client;
use crate::parse::snake_keys;
use crate::refs;
use crate::stats;
use crate::{notes, people};

const PERMISSION: &str = "/api/galaxy/creator/datacenter/permission/query";
const ACCOUNT: &str = "/api/galaxy/v2/creator/datacenter/account/base";
const ACCOUNT_SOURCES: &str = "/api/galaxy/v2/creator/datacenter/audience/source/account";
const VIEW_HOURS: &str = "/api/galaxy/v2/creator/datacenter/audience/view/periods";
const FANS: &str = "/api/galaxy/creator/data/fans/overall_new";
const FANS_PORTRAIT: &str = "/api/galaxy/creator/data/fans_portrait_new";
const FANS_SOURCES: &str = "/api/galaxy/creator/data/fans_source";
const NOTE_BASE: &str = "/api/galaxy/creator/datacenter/note/base";
const NOTE_RETENTION: &str = "/api/galaxy/creator/datacenter/note/analyze/audience/trend";
const NOTE_SOURCES: &str = "/api/galaxy/creator/datacenter/note/audience/source";
const NOTE_AUDIENCE: &str = "/api/galaxy/creator/datacenter/note/audience/source/detail";

/// `permission/query` status when the data center shows numbers (0: not enabled, 1: from tomorrow).
const HAS_DATA: i64 = 2;
/// The fans page asks for the portrait and follow sources from 50 fans on.
const PORTRAIT_MIN_FANS: u64 = 50;

/// The creator center's windows: `seven` / `thirty` blocks of its payloads.
pub fn window(days: u32) -> (&'static str, u32) {
  if days <= 7 {
    ("seven", 7)
  } else {
    ("thirty", 30)
  }
}

/// A galaxy GET with snake_case keys.
async fn get(c: &Client, path: &str, params: &[(&str, &str)]) -> Result<Value> {
  Ok(snake_keys(c.creator_get(path, params).await?))
}

/// One panel of a page: an upstream refusal only leaves a line in
/// `extra.unavailable`; captchas, rate limits and login errors still stop.
async fn panel(
  c: &Client,
  ins: &mut Insights,
  path: &str,
  params: &[(&str, &str)],
) -> Result<Value> {
  match get(c, path, params).await {
    Ok(v) => Ok(v),
    Err(e) if soft(&e) => {
      let name = path.rsplit('/').take(2).collect::<Vec<_>>();
      unavailable(ins, &format!("{}/{}: {}", name[1], name[0], e.message));
      Ok(Value::Null)
    }
    Err(e) => Err(e),
  }
}

fn soft(e: &Error) -> bool {
  matches!(
    e.code,
    ErrorCode::UpstreamError | ErrorCode::NotFound | ErrorCode::PermissionDenied
  )
}

fn unavailable(ins: &mut Insights, text: &str) {
  let list = ins.extra.entry("unavailable".into()).or_insert(json!([]));
  if let Some(a) = list.as_array_mut() {
    a.push(json!(text));
  }
}

/// The rows under `key`; when there are none, the reason the page shows
/// (`<key>_tip_msg`, `no_data_tip_msg`) goes to `extra.unavailable`.
fn rows<'a>(ins: &mut Insights, dimension: &str, payload: &'a Value, key: &str) -> &'a [Value] {
  let items = payload.list(key);
  let tip = payload.first_str(&[&format!("{key}_tip_msg"), "no_data_tip_msg"]);
  if let (true, Some(tip)) = (items.is_empty(), tip) {
    unavailable(ins, &format!("{dimension}: {tip}"));
  }
  items
}

/// A breakdown of the `[{title, value}]` rows under `key`.
fn breakdown(ins: &mut Insights, dimension: &str, payload: &Value, key: &str, percent: bool) {
  let items = rows(ins, dimension, payload, key);
  stats::breakdown(ins, dimension, items, percent);
}

/// `[{<x>, count}]` rows labelled by `x` (hour of day, second of a video).
fn labelled(items: &[Value], x: &str, label: impl Fn(i64) -> String) -> Vec<Value> {
  items
    .iter()
    .filter_map(|i| Some(json!({"title": label(i.i64(x)?), "value": i.f64("count")?})))
    .collect()
}

// ── account ─────────────────────────────────────────────────────────────

pub async fn account(c: &Client, days: u32) -> Result<Insights> {
  let me = people::whoami(c).await?;
  let (key, window_days) = window(days);
  let mut ins = Insights {
    kind: "account".into(),
    subject: me.id.clone(),
    title: Some(me.name.clone()),
    url: me.url.clone(),
    ..Insights::default()
  };
  ins.extra.insert("window_days".into(), json!(window_days));
  let mut raw = serde_json::Map::new();

  // The page checks this first; the panels answer either way, with zeros.
  let permission = get(c, PERMISSION, &[]).await?;
  if permission.i64("status") != Some(HAS_DATA) {
    let tip = permission.str("tip_msg").unwrap_or_default();
    ins.extra.insert(
      "notice".into(),
      json!(format!(
        "the data center (数据看板) is not enabled yet, numbers may be missing: {tip}"
      )),
    );
  }
  let base = panel(c, &mut ins, ACCOUNT, &[]).await?;
  let block = base.at(key);
  stats::totals(&mut ins, block, stats::ACCOUNT);
  stats::series(&mut ins, block, stats::ACCOUNT, None);
  ins.from = block.i64("begin_time").and_then(stats::day);
  ins.to = block.i64("end_time").and_then(stats::day);
  stats::diagnosis(&mut ins, "vs_similar_creators", base.list("analyse_infos"));
  let sources = panel(c, &mut ins, ACCOUNT_SOURCES, &[]).await?;
  breakdown(&mut ins, "traffic_source", &sources, key, true);
  // 观看时段: `{start_point, end_point, count}` per hour.
  let hours = panel(c, &mut ins, VIEW_HOURS, &[]).await?;
  let items = labelled(
    rows(&mut ins, "hour_of_day", &hours, key),
    "start_point",
    |h| format!("{h:02}:00"),
  );
  stats::breakdown(&mut ins, "hour_of_day", &items, false);

  let fans = panel(c, &mut ins, FANS, &[]).await?;
  let block = fans.at(key);
  stats::totals(&mut ins, block, stats::FANS);
  stats::series(&mut ins, block, stats::FANS, None);
  if block.u64("fans_count").unwrap_or(0) >= PORTRAIT_MIN_FANS {
    let portrait = panel(c, &mut ins, FANS_PORTRAIT, &[]).await?;
    for dimension in ["gender", "age", "city", "interest"] {
      breakdown(&mut ins, dimension, &portrait, dimension, true);
    }
    let sources = panel(c, &mut ins, FANS_SOURCES, &[]).await?;
    let sources = match sources {
      list @ Value::Array(_) => json!({ "list": list }),
      other => other,
    };
    breakdown(&mut ins, "follow_source", &sources, "list", false);
    raw.insert("fans_portrait".into(), portrait);
    raw.insert("fans_sources".into(), sources);
  } else {
    unavailable(
      &mut ins,
      &format!("fan portrait: shown from {PORTRAIT_MIN_FANS} fans on"),
    );
  }
  if ins.from.is_none() {
    ins.from = first_last(&ins).map(|(a, _)| a);
    ins.to = first_last(&ins).map(|(_, b)| b);
  }
  for (name, v) in [
    ("permission", permission),
    ("account", base),
    ("sources", sources),
    ("view_hours", hours),
    ("fans", fans),
  ] {
    raw.insert(name.into(), v);
  }
  ins.raw = Some(Value::Object(raw));
  Ok(ins)
}

/// First and last day of all series.
fn first_last(ins: &Insights) -> Option<(String, String)> {
  let days = ins.series.iter().flat_map(|s| &s.points).map(|p| &p.date);
  Some((days.clone().min()?.clone(), days.max()?.clone()))
}

// ── one note ────────────────────────────────────────────────────────────

pub async fn note(c: &Client, arg: &str, days: u32) -> Result<Insights> {
  c.require_login()?;
  let r = refs::note_ref(c, arg).await?;
  let base = match get(c, NOTE_BASE, &[("note_id", &r.id)]).await {
    Ok(v) if v.at("note_info").is_object() => v,
    Ok(_) => return public(c, arg).await,
    Err(e) if soft(&e) => return public(c, arg).await,
    Err(e) => return Err(e),
  };
  let info = base.at("note_info");
  let mut ins = Insights {
    kind: "post".into(),
    subject: r.id.clone(),
    title: info.first_str(&["title", "desc"]),
    url: Some(r.url()),
    ..Insights::default()
  };
  ins.extra.insert("scope".into(), json!("own"));
  // Totals cover the note's whole life; series only the last `days` days.
  ins.extra.insert("totals_period".into(), json!("lifetime"));
  let kind = info.str("type").map(|t| t.to_lowercase());
  if let Some(t) = &kind {
    ins.extra.insert("type".into(), json!(t));
  }
  stats::totals(&mut ins, &base, stats::NOTE);
  let since = days_ago(days);
  stats::series(&mut ins, base.at("day"), stats::NOTE, since.as_deref());
  ins.from = info.i64("post_time").and_then(stats::day).max(since);
  ins.to = first_last(&ins).map(|(_, b)| b);
  stats::diagnosis(&mut ins, "vs_similar_notes", base.list("analyse_infos"));

  let id = [("note_id", r.id.as_str())];
  let sources = panel(c, &mut ins, NOTE_SOURCES, &id).await?;
  breakdown(&mut ins, "traffic_source", &sources, "source", true);
  let audience = panel(c, &mut ins, NOTE_AUDIENCE, &id).await?;
  for dimension in ["gender", "age", "city", "interest"] {
    breakdown(&mut ins, dimension, &audience, dimension, true);
  }
  let mut raw = serde_json::Map::new();
  if kind.as_deref() == Some("video") {
    // 观看趋势: the share of viewers still watching at each second.
    let trend = panel(c, &mut ins, NOTE_RETENTION, &id).await?;
    let items = labelled(
      rows(&mut ins, "retention", &trend, "trend_list"),
      "date",
      |s| format!("{s}s"),
    );
    stats::breakdown(&mut ins, "retention", &items, true);
    raw.insert("retention".into(), trend);
  }
  raw.insert("base".into(), base);
  raw.insert("sources".into(), sources);
  raw.insert("audience".into(), audience);
  ins.raw = Some(Value::Object(raw));
  Ok(ins)
}

/// Someone else's note: only its public counters.
async fn public(c: &Client, arg: &str) -> Result<Insights> {
  let post = notes::read(c, arg).await?;
  let mut ins = Insights {
    kind: "post".into(),
    subject: post.id.clone(),
    title: post.title.clone(),
    url: post.url.clone(),
    ..Insights::default()
  };
  let m = &post.metrics;
  for (name, value) in [
    ("likes", m.likes),
    ("comments", m.comments),
    ("shares", m.shares),
    ("favorites", m.favorites),
  ] {
    ins.total(name, value.map(Value::from));
  }
  for (name, value) in &m.other {
    ins.total(name, Some(Value::from(*value)));
  }
  ins.extra.insert("scope".into(), json!("public"));
  ins.extra.insert(
    "notice".into(),
    json!("not one of your notes: the creator center has data only for your own, these are its public counters"),
  );
  ins.raw = post.raw;
  Ok(ins)
}

/// The first day (Beijing time) of a window of `days` days ending today.
fn days_ago(days: u32) -> Option<String> {
  let now = crate::sign::now_ms() as i64;
  stats::day(now - i64::from(days.saturating_sub(1)) * 86_400_000)
}
