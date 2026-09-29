//! Creator-center analytics (创作中心 → 数据分析): totals, daily trends, follower
//! changes and audience of the account, or of one of its own posts.
//!
//! Endpoints from the creator center bundle (static.zhihu.com/heifetz/main.app.*.js:
//! the `LOAD_ANALYTICS_*` actions; pages in chunks/main-creator-routes.*.js). They
//! answer the logged-in session without the `x-zse-96` signature, and report
//! failures as HTTP 200 `{"code": "403", "msg": "no auth"}` (e.g. someone else's post).

use jiff::tz::{self, TimeZone};
use jiff::{Timestamp, ToSpan};
use media_core::text::{one_line, truncate};
use media_core::{
  Breakdown, Ctx, Error, ErrorCode, Insights, Page, PageReq, Post, Result, Share, Value, ValueExt,
  json,
};

use crate::api::{self, V4};
use crate::parse::stats;
use crate::refs::Target;
use crate::{account, read};

/// The last `days` days up to today (China time), as the creator center asks for them.
fn window(days: u32) -> (String, String) {
  let today = Timestamp::now()
    .to_zoned(TimeZone::fixed(tz::offset(8)))
    .date();
  let from = today.saturating_sub((i64::from(days) - 1).days());
  (from.to_string(), today.to_string())
}

async fn get(ctx: &Ctx, path: &str, query: &[(&str, &str)]) -> Result<Value> {
  let url = format!("{V4}/creators/analysis/{path}");
  let v = api::call(ctx, api::get(ctx, &url).queries(query.iter().copied())).await?;
  match (v.str("code"), v.str("msg")) {
    (Some(code), Some(msg)) if code == "403" => Err(Error::new(
      ErrorCode::PermissionDenied,
      format!("Zhihu creator center: {msg}"),
    )),
    (Some(code), Some(msg)) => Err(Error::upstream(format!(
      "Zhihu creator center: {msg} ({code})"
    ))),
    _ => Ok(v),
  }
}

/// A secondary section: problems that end the session or the network stop
/// everything; anything else leaves the section out and is noted in `extra.missing`.
fn soft(ins: &mut Insights, section: &str, result: Result<Value>) -> Result<Value> {
  match result {
    Ok(v) => Ok(v),
    Err(e)
      if matches!(
        e.code,
        ErrorCode::PermissionDenied | ErrorCode::NotFound | ErrorCode::UpstreamError
      ) =>
    {
      let missing = ins
        .extra
        .entry("missing".into())
        .or_insert_with(|| json!({}));
      missing[section] = e.message.into();
      Ok(Value::Null)
    }
    Err(e) => Err(e),
  }
}

/// Daily rows: a bare array or `{data: [...]}`.
fn rows(v: &Value) -> &[Value] {
  v.as_array().map(Vec::as_slice).unwrap_or(v.list("data"))
}

fn names() -> Value {
  stats::NAMES
    .iter()
    .map(|(k, v)| ((*k).to_owned(), Value::from(*v)))
    .collect::<serde_json::Map<_, _>>()
    .into()
}

pub async fn account(ctx: &Ctx, days: u32) -> Result<Insights> {
  let me = account::whoami(ctx).await?;
  let (from, to) = window(days);
  let range = [("start", from.as_str()), ("end", to.as_str())];
  let tab = [("tab", "all"), range[0], range[1]];
  let mut ins = Insights {
    kind: "account".into(),
    subject: me.handle.clone().unwrap_or_else(|| me.id.clone()),
    title: Some(me.name.clone()),
    url: me.url.clone(),
    ..Insights::default()
  };
  let aggr = get(ctx, "realtime/member/aggr", &tab).await?;
  let daily = get(ctx, "realtime/member/daily", &tab).await;
  let daily = soft(&mut ins, "daily", daily)?;
  let follow = get(ctx, "aggregation/tab/follow", &[]).await;
  let follow = soft(&mut ins, "followers", follow)?;
  let changes = get(ctx, "aggregation/tab/follow/detail/v2", &range).await;
  let changes = soft(&mut ins, "follower_changes", changes)?;
  let readers = get(ctx, "realtime/member/visitor_portrait", &[("type", "all")]).await;
  let readers = soft(&mut ins, "reader_portrait", readers)?;
  let profile = get(ctx, "aggregation/tab/follow/profile", &[]).await;
  let profile = soft(&mut ins, "follower_portrait", profile)?;

  stats::totals(&mut ins, &aggr, false);
  let followers = follow.u64("follow.total").or(me.stats.followers);
  ins.total("followers", followers.map(Value::from));
  ins.total(
    "active_followers",
    stats::number(&follow, "follow.active_follower_num"),
  );
  ins.total(
    "active_follower_ratio",
    stats::number(&follow, "follow.active_follower_ratio"),
  );
  stats::follower_totals(&mut ins, rows(&changes));

  ins.series = stats::series(rows(&daily), false);
  ins.series.extend(stats::follower_series(rows(&changes)));

  ins.breakdowns = stats::portrait(readers.at("content.content"), "");
  let status = profile.i64("profile.status");
  if status == Some(1) {
    let all = profile.at("profile.all_follower");
    ins.breakdowns.extend(stats::portrait(all, "follower_"));
  } else if let Some(status) = status {
    // 0 not active, 2..4 updating / processing / generating (creator bundle `lj`).
    let note = format!("not available (status {status}); Zhihu builds it for larger audiences");
    ins.extra.insert("follower_portrait".into(), note.into());
  }
  ins.breakdowns.extend(content_types(&follow, &me));

  ins.from = Some(from);
  ins.to = Some(to);
  if let Some(updated) = aggr.str("updated") {
    ins.extra.insert("updated".into(), updated.into());
  }
  // Reader portraits are what Zhihu reports overall, not only the window.
  for b in ins
    .breakdowns
    .iter_mut()
    .filter(|b| !matches!(b.dimension.as_str(), "content_type"))
  {
    b.period.get_or_insert_with(|| "lifetime".into());
  }
  ins.extra.insert("metric_names".into(), names());
  ins.raw = Some(json!({
    "aggr": aggr, "daily": daily, "follow": follow, "follower_changes": changes,
    "reader_portrait": readers, "follower_portrait": profile,
  }));
  Ok(ins)
}

/// Published works per content type (`creation_count`, plus pins from the profile).
fn content_types(follow: &Value, me: &media_core::User) -> Option<Breakdown> {
  let mut items: Vec<(String, u64)> = ["answer", "article", "video"]
    .iter()
    .filter_map(|k| Some(((*k).to_owned(), follow.u64(&format!("creation_count.{k}"))?)))
    .collect();
  if let Some(pins) = me.stats.other.get("pins") {
    items.push(("pin".into(), *pins));
  }
  let total: u64 = items.iter().map(|(_, n)| n).sum();
  (total > 0).then(|| Breakdown {
    dimension: "content_type".into(),
    items: items
      .into_iter()
      .map(|(label, n)| Share {
        label,
        value: n.into(),
        ratio: Some(n as f64 / total as f64),
        ..Default::default()
      })
      .collect(),
    ..Default::default()
  })
}

pub async fn post(ctx: &Ctx, target: &Target, days: u32) -> Result<Insights> {
  let kind = match target {
    Target::Question(_) => {
      return public(ctx, target, "Zhihu has no creator analytics for questions").await;
    }
    Target::Answer(_) => "answer",
    Target::Article(_) => "article",
    Target::Pin(_) => "pin",
  };
  let (from, to) = window(days);
  let one = [("type", kind), ("token", target.id())];
  let q = [
    one[0],
    one[1],
    ("start", from.as_str()),
    ("end", to.as_str()),
  ];
  let aggr = match get(ctx, "realtime/content/aggr", &q).await {
    Err(e) if e.code == ErrorCode::PermissionDenied => {
      let note = "not your post: public counters only (creator analytics cover your own posts)";
      return public(ctx, target, note).await;
    }
    other => other?,
  };
  let mut ins = Insights {
    kind: "post".into(),
    subject: target.id().to_owned(),
    url: Some(target.url()),
    ..Insights::default()
  };
  let daily = get(ctx, "realtime/content/daily", &q).await;
  let daily = soft(&mut ins, "daily", daily)?;
  let readers = get(ctx, "realtime/content/visitor_portrait", &one).await;
  let readers = soft(&mut ins, "reader_portrait", readers)?;

  let pin = kind == "pin";
  let obj = aggr.at(kind);
  ins.title = obj
    .first_str(&stats::TITLE)
    .map(|t| truncate(&one_line(&t), 80));
  stats::totals(&mut ins, &aggr, pin);
  ins.series = stats::series(rows(&daily), pin);
  ins.breakdowns = stats::portrait(readers.at("content.content"), "");
  ins.from = Some(from);
  ins.to = Some(to);
  if let Some(status) = aggr.str("advanced.status").filter(|s| s != "normal") {
    // `unnormal_by_level` / `unnormal_by_pv`: advanced numbers need a creator level or more reads.
    ins.extra.insert("advanced_status".into(), status.into());
  }
  for flag in ["is_collapsed", "is_suggest"] {
    if aggr.bool(&format!("content_mark.{flag}")) == Some(true) {
      ins
        .extra
        .insert(flag.trim_start_matches("is_").into(), true.into());
    }
  }
  ins.extra.insert("metric_names".into(), names());
  ins.raw = Some(json!({ "aggr": aggr, "daily": daily, "reader_portrait": readers }));
  Ok(ins)
}

/// Current public counters of any post.
async fn public(ctx: &Ctx, target: &Target, note: &str) -> Result<Insights> {
  let post = read::read(ctx, target).await?;
  let m = &post.metrics;
  let mut ins = Insights {
    kind: "post".into(),
    subject: target.id().to_owned(),
    title: post.title.clone(),
    url: post.url.clone(),
    ..Insights::default()
  };
  for (key, n) in [
    ("views", m.views),
    ("likes", m.likes),
    ("comments", m.comments),
    ("shares", m.shares),
    ("favorites", m.favorites),
  ] {
    ins.total(key, n.map(Value::from));
  }
  for (key, n) in &m.other {
    ins.total(key, Some(Value::from(*n)));
  }
  ins.extra.insert("note".into(), note.into());
  ins.raw = post.raw;
  Ok(ins)
}

/// The account's own posts of one type with their lifetime numbers (内容分析 list).
pub async fn creations(ctx: &Ctx, kind: &str, page: &PageReq) -> Result<Page<Post>> {
  let offset = page.number_or(0).to_string();
  let limit = page.size_within(20).to_string();
  let v = get(
    ctx,
    "realtime/content/list",
    &[("type", kind), ("offset", &offset), ("limit", &limit)],
  )
  .await?;
  Ok(Page::new(
    v.list("data")
      .iter()
      .map(|row| stats::creation(row, kind))
      .collect(),
    api::next_param(&v, "offset"),
  ))
}
