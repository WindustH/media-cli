//! Analytics of one post (`insights POST`): the creator center's numbers for
//! the account's own videos, the public counters for everything else.
//!
//! Own videos use the data-center app's per-video endpoints
//! (`/x/web/data/archive_diagnose/{overview,play_analyze,trend}` and
//! `/x/web/data/v2/archive/analyze/graph`; bundle `data-center-web.index`,
//! chunks 5408, 5680, 5865, 7890 of 2026-09, see [`crate::creator`]).

use media_core::{Breakdown, Ctx, Insights, Result, Share, Value, ValueExt, json};

use crate::creator::{data, soft};
use crate::parse::insights::{self as pi, AGES, DEVICES, GENDERS, VIEWERS};
use crate::refs::{DYNAMIC_URL, PostRef, Video};
use crate::{account, api, dynamic, parse, video};

const ONLINE: &str = "https://api.bilibili.com/x/player/online/total";

/// Public counters of `view.stat` and their metrics.
const PUBLIC: &[(&str, &str)] = &[
  ("view", "views"),
  ("like", "likes"),
  ("reply", "comments"),
  ("share", "shares"),
  ("favorite", "favorites"),
  ("coin", "coins"),
  ("danmaku", "danmaku"),
];

/// Rates of `archive_diagnose/play_analyze` (basis points) and their metrics;
/// `*_pass_rate` next to each tells the share of similar videos it beats.
const RATES: &[(&str, &str, &str)] = &[
  ("guest_interact", "crash", "three_second_bounce_rate"),
  ("guest_interact", "interact", "interaction_rate"),
  ("guest_interact", "play_trans_fan", "follow_conversion_rate"),
  ("arc_audience", "play_viewer", "non_follower_view_rate"),
  ("arc_audience", "play_fan", "follower_watch_rate"),
];

/// Click-through rates (cover and title) whose rank among similar videos is reported.
const CTR_RANKS: &[(&str, &str)] = &[
  ("tm", "ctr"),
  ("tm_fan", "follower_ctr"),
  ("tm_viewer", "non_follower_ctr"),
];

/// Per-video trend keys (`archive_diagnose/trend?type=`) and their metrics.
const TREND: &[(&str, &str)] = &[
  ("play", "views"),
  ("fan", "new_followers"),
  ("unfollow", "lost_followers"),
  ("like", "likes"),
  ("comment", "comments"),
  ("dm", "danmaku"),
  ("fav", "favorites"),
  ("coin", "coins"),
  ("share", "shares"),
];

pub async fn insights(ctx: &Ctx, post: &PostRef, days: u32) -> Result<Insights> {
  match post {
    PostRef::Video(v) => of_video(ctx, v, days).await,
    PostRef::Dynamic(id) => {
      let item = dynamic::detail(ctx, id).await?;
      let bvid = item.str("modules.module_dynamic.major.archive.bvid");
      match bvid.as_deref().and_then(Video::from_bvid) {
        Some(v) => of_video(ctx, &v, days).await,
        None => Ok(of_dynamic(id, item)),
      }
    }
  }
}

/// Public counters of a dynamic (Bilibili keeps no analytics for them).
fn of_dynamic(id: &str, item: Value) -> Insights {
  let post = parse::dynamic(&item);
  let mut i = Insights {
    kind: "post".into(),
    subject: id.to_owned(),
    title: post.title.or(post.text),
    url: Some(format!("{DYNAMIC_URL}{id}")),
    ..Insights::default()
  };
  i.total("likes", post.metrics.likes.map(Value::from));
  i.total("comments", post.metrics.comments.map(Value::from));
  i.total("shares", post.metrics.shares.map(Value::from));
  i.extra.insert(
    "note".into(),
    json!("Bilibili keeps no analytics for dynamics: public counters only"),
  );
  i.raw = Some(item);
  i
}

async fn of_video(ctx: &Ctx, v: &Video, days: u32) -> Result<Insights> {
  let view = video::view(ctx, v).await?;
  let mut i = Insights {
    kind: "post".into(),
    subject: v.bvid.clone(),
    title: parse::plain(&view, &["title"]),
    url: Some(v.url()),
    ..Insights::default()
  };
  for (key, metric) in PUBLIC {
    i.total(metric, view.count(&format!("stat.{key}")).map(Value::from));
  }
  i.total(
    "highest_rank",
    view
      .u64("stat.his_rank")
      .filter(|r| *r > 0)
      .map(Value::from),
  );
  i.total(
    "current_rank",
    view
      .u64("stat.now_rank")
      .filter(|r| *r > 0)
      .map(Value::from),
  );
  let owner = view.str("owner.mid").unwrap_or_default();
  let me = match ctx.require_login(api::LOGIN_COOKIES) {
    Ok(()) => Some(account::my_mid(ctx).await?),
    Err(_) => None,
  };
  let mut raw = serde_json::Map::new();
  if me.as_deref() == Some(owner.as_str()) {
    own(ctx, v, &view, &owner, days, &mut i, &mut raw).await?;
  } else {
    online(ctx, v, &view, &mut i).await;
    i.extra.insert(
      "note".into(),
      json!("not a video of the logged-in account: public counters only"),
    );
  }
  raw.insert("view".into(), view);
  i.raw = Some(Value::Object(raw));
  Ok(i)
}

/// Viewers watching right now (`1000+` style above a thousand).
async fn online(ctx: &Ctx, v: &Video, view: &Value, i: &mut Insights) {
  let Ok(cid) = video::cid(view, v.page) else {
    return;
  };
  let call = api::get(ctx, ONLINE)
    .arg("aid", v.aid)
    .arg("bvid", &v.bvid)
    .arg("cid", cid);
  if let Ok(data) = call.send().await {
    i.total("watching_now", data.count("total").map(Value::from));
    let rounded = |s: &String| !s.bytes().all(|b| b.is_ascii_digit());
    if let Some(label) = data.str("total").filter(rounded) {
      i.extra.insert("watching_now".into(), json!(label));
    }
  }
}

/// The creator center's analysis of one of the account's own videos.
async fn own(
  ctx: &Ctx,
  v: &Video,
  view: &Value,
  mid: &str,
  days: u32,
  i: &mut Insights,
  raw: &mut serde_json::Map<String, Value>,
) -> Result<()> {
  let mut errors = Vec::new();
  let bvid = v.bvid.as_str();

  let mut beats = serde_json::Map::new();
  let call = data(ctx, "/archive_diagnose/overview", mid).arg("bvid", bvid);
  if let Some(o) = soft(call, "archive_overview", &mut errors).await? {
    let stat = o.at("stat");
    i.total("new_followers", stat.f64("fan").map(pi::num));
    i.total("lost_followers", stat.f64("unfollow").map(pi::num));
    // Filled only where Bilibili enabled them: watch-time index, charging (in fen).
    let positive = |key: &str| stat.f64(key).filter(|n| *n > 0.0);
    i.total("watch_minutes", positive("vt").map(pi::num));
    i.total(
      "charging_yuan",
      positive("elec").map(|f| pi::num(f / 100.0)),
    );
    audience(&o, i);
    raw.insert("archive_overview".into(), o);
  }

  let call = data(ctx, "/archive_diagnose/play_analyze", mid).arg("bvid", bvid);
  if let Some(p) = soft(call, "archive_play_analyze", &mut errors).await? {
    for (group, key, metric) in RATES {
      i.total(metric, pi::basis_points(&p, &format!("{group}.{key}_rate")));
      if let Some(pass) = pi::basis_points(&p, &format!("{group}.{key}_pass_rate")) {
        beats.insert((*metric).into(), pass);
      }
    }
    // The cover / title click-through rate itself (`tm_rate`) is not taken:
    // the web app never shows it, and it read 2.2% and 10.5% for the same
    // video within an hour (the compare table: 0.44%). Its rank is stable.
    for (key, metric) in CTR_RANKS {
      let path = format!("guest_interact.{key}_pass_rate");
      if let Some(pass) = pi::basis_points(&p, &path) {
        beats.insert((*metric).into(), pass);
      }
    }
    let tips: Vec<String> = [
      "arc_audience.tip",
      "guest_interact.tip",
      "guest_interact.suggestion",
    ]
    .iter()
    .filter_map(|path| p.str(path))
    .collect();
    if !tips.is_empty() {
      i.extra.insert("tips".into(), json!(tips));
    }
    raw.insert("archive_play_analyze".into(), p);
  }

  if let Ok(cid) = video::cid(view, v.page) {
    let call = data(ctx, "/v2/archive/analyze/graph", mid).arg("cid", cid);
    if let Some(g) = soft(call, "archive_analyze_graph", &mut errors).await? {
      retention(&g, i);
      if let Some(pass) = pi::basis_points(&g, "quit_info.pass_peer") {
        beats.insert("avg_watch_ratio".into(), pass);
      }
      raw.insert("archive_analyze_graph".into(), g);
    }
  }
  if !beats.is_empty() {
    i.extra.insert("beats_similar".into(), Value::Object(beats));
  }

  let keys: Vec<&str> = TREND.iter().map(|(k, _)| *k).collect();
  let call = data(ctx, "/archive_diagnose/trend", mid)
    .arg("bvid", bvid)
    .arg("type", keys.join(","));
  if let Some(t) = soft(call, "archive_trend", &mut errors).await? {
    trend(&t, days, i);
    raw.insert("archive_trend".into(), t);
  }
  if !errors.is_empty() {
    i.extra.insert("errors".into(), json!(errors));
  }
  Ok(())
}

/// Who watched: age, gender, region, interests, terminal, followers or not.
fn audience(o: &Value, i: &mut Insights) {
  let mut add = |dim: &str, items: Vec<(String, f64)>, sort: bool| {
    i.breakdowns.extend(pi::breakdown(dim, items, sort));
  };
  add(
    "viewer_type",
    pi::fields(o.at("audience_proportion"), VIEWERS),
    true,
  );
  add("device", pi::fields(o.at("play_proportion"), DEVICES), true);
  add("gender", pi::fields(o.at("gender"), GENDERS), false);
  add("age", pi::fields(o.at("viewer_age"), AGES), false);
  add(
    "region",
    pi::pairs(o.list("viewer_area"), "location", "count"),
    true,
  );
  let mut interest = pi::pairs(o.list("viewer_ty"), "tag_name", "count");
  interest.sort_by(|a, b| b.1.total_cmp(&a.1));
  interest.truncate(10);
  add("interest", interest, true);
}

/// Average watch time and share of the video, and the retention curve. (Its
/// last point, at the very end, is far below the one before it, so no
/// completion rate is derived from it.)
fn retention(g: &Value, i: &mut Insights) {
  let duration = g.f64("quit_info.duration").filter(|d| *d > 0.0);
  let positive = |path: &str| g.f64(path).filter(|n| *n > 0.0);
  // "平均播放进度": seconds, and the same as a share of the duration.
  let seconds =
    positive("quit_info.avg_play_progress").or_else(|| positive("duration_info.avg_play_time_int"));
  i.total("avg_watch_seconds", seconds.map(pi::num));
  let ratio = positive("quit_info.full_play_ratio")
    .map(|bp| bp / 10_000.0)
    .or_else(|| Some(seconds? / duration?));
  i.total("avg_watch_ratio", ratio.map(|r| json!(pi::round4(r))));
  // Share of viewers still watching at each offset (10000 = everyone).
  let curve: Vec<(u64, f64)> = g
    .list("viewer_quit")
    .iter()
    .filter_map(|p| Some((p.u64("duration_key")?, p.f64("num")? / 10_000.0)))
    .collect();
  if !curve.is_empty() {
    let items = curve
      .into_iter()
      .map(|(at, share)| Share {
        label: format!("{}:{:02}", at / 60, at % 60),
        value: json!(pi::round4(share)),
        ratio: Some(pi::round4(share)),
      })
      .collect();
    i.breakdowns.push(Breakdown {
      dimension: "retention".into(),
      items,
    });
  }
}

/// Daily trends of the last `days` days (the data center keeps the first 30
/// days after publication and the last 30 days), and hourly views of the
/// first 48 hours in `extra.views_hourly`.
fn trend(t: &Value, days: u32, i: &mut Insights) {
  let mut to: Option<String> = None;
  let mut daily = Vec::new();
  for (key, metric) in TREND {
    let mut list = t.list(&format!("data_tendency.{key}")).to_vec();
    list.extend_from_slice(t.list(&format!("last_30_day_data_tendency.{key}")));
    if let Some(s) = pi::series(metric, &list, None) {
      to = to.max(s.points.last().map(|p| p.date.clone()));
      daily.push(s);
    }
  }
  let Some(to) = to else { return };
  let from = pi::days_before(&to, days.saturating_sub(1)).unwrap_or_default();
  for mut s in daily {
    s.points.retain(|p| p.date >= from);
    if !s.points.is_empty() {
      i.series.push(s);
    }
  }
  // Hourly views of the first 48 hours, kept aside so daily tables stay daily.
  let hourly: Vec<Value> = t
    .list("hour_data_tendency.play")
    .iter()
    .filter_map(|p| {
      let at = jiff::Timestamp::from_second(p.i64("date_key")?).ok()?;
      let day = pi::day(p.at("date_key"))?;
      (day >= from).then(|| json!({ "at": at.to_string(), "views": p.at("total_inc") }))
    })
    .collect();
  if !hourly.is_empty() {
    i.extra.insert("views_hourly".into(), json!(hourly));
  }
  i.from = i
    .series
    .iter()
    .filter_map(|s| s.points.first())
    .map(|p| p.date.get(..10).unwrap_or(&p.date).to_owned())
    .min();
  i.to = Some(to);
}
