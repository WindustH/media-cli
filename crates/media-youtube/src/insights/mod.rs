//! Analytics.
//!
//! Creator analytics come from the official YouTube Analytics API with OAuth
//! credentials in the environment ([`oauth`]): totals, daily series and
//! distributions (traffic source, playback location, device, OS, subscribed
//! status, region, sharing service, age, gender, top videos) of the channel
//! or of one of its videos. YouTube Studio's own backend is not used: its
//! endpoints are only discoverable from bundles served to logged-in sessions.
//! Impressions and click-through rate exist only in Studio (and the
//! Reporting API's bulk "reach" reports), so they are reported as missing.
//!
//! Without OAuth, a logged-in account gets its channel's public numbers and
//! its recent uploads; other people's videos always get their public counters.

mod oauth;
mod report;

use jiff::ToSpan;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use media_core::{Breakdown, ErrorCode, Insights, PageReq, Result, Share, Value, ValueExt, json};

use self::oauth::{Credentials, SETUP_HINT};
use self::report::{BREAKDOWNS, Client, DAILY, METRICS, breakdown, label};
use crate::api::Api;
use crate::channel::{self, Tab};
use crate::{account, refs, video};

pub use self::oauth::authorize;

const NO_REACH: &str = "impressions and click-through rate are shown in YouTube Studio only; the Analytics API does not report them";

/// Days as YouTube Analytics counts them (Pacific time): `days` up to today.
fn window(days: u32) -> (Date, Date) {
  let tz = TimeZone::get("America/Los_Angeles").unwrap_or(TimeZone::UTC);
  let today = jiff::Timestamp::now().to_zoned(tz).date();
  (today.saturating_sub((i64::from(days) - 1).days()), today)
}

/// A part the insights can do without: failures other than the session, the
/// rate limit or the network become warnings.
fn soft<T>(ins: &mut Insights, what: &str, r: Result<T>) -> Result<Option<T>> {
  match r {
    Ok(v) => Ok(Some(v)),
    Err(e)
      if matches!(
        e.code,
        ErrorCode::NotAuthenticated | ErrorCode::RateLimited | ErrorCode::NetworkError
      ) =>
    {
      Err(e)
    }
    Err(e) => {
      ins.warnings.push(format!("{what}: {}", e.message));
      Ok(None)
    }
  }
}

pub async fn insights(api: &Api, post: Option<&str>, days: u32) -> Result<Insights> {
  match (post, Credentials::from_env()) {
    (Some(p), Some(c)) => video_report(&Client { api, creds: &c }, p, days).await,
    (None, Some(c)) => account_report(&Client { api, creds: &c }, days).await,
    (Some(p), None) => {
      let note = format!("public counters only: {SETUP_HINT}");
      public(api, p, vec![note]).await
    }
    (None, None) => account_snapshot(api, days).await,
  }
}

/// Totals, daily series and distributions of the channel or one video.
async fn fill(
  c: &Client<'_>,
  ins: &mut Insights,
  totals: (&str, &str),
  series: (&str, &str),
  video: Option<&str>,
) -> Result<()> {
  let mut raw = serde_json::Map::new();
  let all: Vec<&str> = METRICS.iter().map(|(m, _)| *m).collect();
  let t = c
    .report(&all.join(","), None, totals, video, None, None)
    .await;
  if let Some(t) = soft(ins, "totals", t)? {
    raw.insert("totals".into(), t.raw.clone());
    for (k, v) in t.totals() {
      ins.total(&k, Some(v));
    }
    let gained = ins.totals.get("new_followers").and_then(Value::as_i64);
    let lost = ins.totals.get("lost_followers").and_then(Value::as_i64);
    if let (Some(g), Some(l)) = (gained, lost) {
      ins.total("net_followers", Some(Value::from(g - l)));
    }
  }
  let daily = c
    .report(DAILY, Some("day"), series, video, Some("day"), None)
    .await;
  if let Some(t) = soft(ins, "daily series", daily)? {
    raw.insert("daily".into(), t.raw.clone());
    ins.series = t.series();
  }
  for (name, dim, metric) in BREAKDOWNS {
    let sort = format!("-{metric}");
    let r = c
      .report(metric, Some(dim), totals, video, Some(&sort), Some(25))
      .await;
    if let Some(t) = soft(ins, name, r)? {
      raw.insert((*name).into(), t.raw.clone());
      let pairs = t
        .pairs()
        .into_iter()
        .map(|(l, v)| (label(dim, &l), v, (*dim == "country").then_some(l)))
        .collect();
      ins.breakdowns.push(breakdown(name, pairs));
    }
  }
  let demo = c
    .report(
      "viewerPercentage",
      Some("ageGroup,gender"),
      totals,
      video,
      None,
      None,
    )
    .await;
  if let Some(t) = soft(ins, "age and gender", demo)? {
    raw.insert("age_gender".into(), t.raw.clone());
    ins.breakdowns.extend(demographics(&t.rows));
  }
  ins.warnings.push(NO_REACH.into());
  ins.raw = Some(Value::Object(raw));
  Ok(())
}

/// `ageGroup, gender, viewerPercentage` rows → an age and a gender breakdown.
fn demographics(rows: &[Vec<Value>]) -> Vec<Breakdown> {
  let mut age: Vec<(String, f64)> = Vec::new();
  let mut gender: Vec<(String, f64)> = Vec::new();
  for r in rows {
    let (Some(a), Some(g), Some(p)) = (r[0].as_str(), r[1].as_str(), r[2].as_f64()) else {
      continue;
    };
    for (list, key) in [
      (&mut age, label("ageGroup", a)),
      (&mut gender, label("gender", g)),
    ] {
      match list.iter_mut().find(|(k, _)| *k == key) {
        Some(e) => e.1 += p / 100.0,
        None => list.push((key, p / 100.0)),
      }
    }
  }
  [("age", age), ("gender", gender)]
    .into_iter()
    .filter(|(_, l)| !l.is_empty())
    .map(|(dim, l)| {
      breakdown(
        dim,
        l.into_iter()
          .map(|(k, v)| (k, Value::from(v), None))
          .collect(),
      )
    })
    .collect()
}

async fn account_report(c: &Client<'_>, days: u32) -> Result<Insights> {
  let ch = c.channel().await?;
  let id = ch.str("id").unwrap_or_default();
  let (from, to) = window(days);
  let range = (from.to_string(), to.to_string());
  let range = (range.0.as_str(), range.1.as_str());
  let mut ins = Insights {
    kind: "account".into(),
    title: ch.str("snippet.title"),
    url: Some(refs::channel_url(&id)),
    from: Some(range.0.to_owned()),
    to: Some(range.1.to_owned()),
    subject: id,
    ..Insights::default()
  };
  fill(c, &mut ins, range, range, None).await?;
  ins.total(
    "followers",
    ch.u64("statistics.subscriberCount").map(Value::from),
  );
  let top = c
    .report(
      "views,estimatedMinutesWatched",
      Some("video"),
      range,
      None,
      Some("-views"),
      Some(10),
    )
    .await;
  if let Some(t) = soft(&mut ins, "top videos", top)? {
    let pairs = t.pairs();
    let ids: Vec<String> = pairs.iter().map(|(id, _)| id.clone()).collect();
    let titles = if ids.is_empty() {
      Vec::new()
    } else {
      c.videos(&ids).await.unwrap_or_default()
    };
    let title = |id: &str| {
      titles
        .iter()
        .find(|v| v.str("id").as_deref() == Some(id))
        .and_then(|v| v.str("snippet.title"))
    };
    let items = pairs
      .into_iter()
      .map(|(id, v)| (title(&id).unwrap_or_else(|| id.clone()), v, Some(id)))
      .collect();
    ins.breakdowns.push(breakdown("top_videos", items));
  }
  ins.extra.insert(
    "channel".into(),
    json!({
      "subscribers": ch.u64("statistics.subscriberCount"),
      "views": ch.u64("statistics.viewCount"),
      "videos": ch.u64("statistics.videoCount"),
    }),
  );
  if let Some(Value::Object(m)) = ins.raw.as_mut() {
    m.insert("channel".into(), ch);
  }
  Ok(ins)
}

async fn video_report(c: &Client<'_>, arg: &str, days: u32) -> Result<Insights> {
  let id = refs::video(arg)?;
  let meta = c.videos(std::slice::from_ref(&id)).await?;
  let Some(meta) = meta.into_iter().next() else {
    return public(
      c.api,
      arg,
      vec!["the Data API does not list this video".into()],
    )
    .await;
  };
  let (from, to) = window(days);
  let published = meta
    .str("snippet.publishedAt")
    .and_then(|s| s.get(..10).map(str::to_owned))
    .unwrap_or_else(|| "2005-04-23".into());
  let start = from.to_string().max(published.clone());
  let (to, start) = (to.to_string(), start);
  let mut ins = Insights {
    kind: "post".into(),
    subject: id.clone(),
    title: meta.str("snippet.title"),
    url: Some(refs::video_url(&id)),
    from: Some(start.clone()),
    to: Some(to.clone()),
    totals_period: Some("lifetime".into()),
    ..Insights::default()
  };
  let probe = c
    .report("views", None, (&published, &to), Some(&id), None, None)
    .await;
  if let Err(e) = &probe
    && e.code == ErrorCode::PermissionDenied
  {
    let note = "not a video of the authorized channel: public counters only".to_owned();
    return public(c.api, arg, vec![note]).await;
  }
  probe?;
  fill(c, &mut ins, (&published, &to), (&start, &to), Some(&id)).await?;
  for b in &mut ins.breakdowns {
    b.period = Some("lifetime".into());
  }
  if let Some(Value::Object(m)) = ins.raw.as_mut() {
    m.insert("video".into(), meta);
  }
  Ok(ins)
}

/// Current public counters of any video or post.
async fn public(api: &Api, arg: &str, warnings: Vec<String>) -> Result<Insights> {
  let post = video::read(api, arg).await?;
  let m = &post.metrics;
  let mut ins = Insights {
    kind: "post".into(),
    subject: post.id.clone(),
    title: post.title.clone().or_else(|| {
      post
        .text
        .as_deref()
        .map(|t| media_core::text::truncate(&media_core::text::one_line(t), 80))
    }),
    url: post.url.clone(),
    totals_period: Some("lifetime".into()),
    warnings,
    ..Insights::default()
  };
  for (key, n) in [
    ("views", m.views),
    ("likes", m.likes),
    ("comments", m.comments),
  ] {
    ins.total(key, n.map(Value::from));
  }
  if let Some(f) = post.author.as_ref().and_then(|a| a.stats.followers) {
    ins.extra.insert("channel_subscribers".into(), f.into());
  }
  ins.raw = post.raw;
  Ok(ins)
}

/// Without OAuth: the logged-in channel's public numbers and the uploads of
/// the window with their current views.
async fn account_snapshot(api: &Api, days: u32) -> Result<Insights> {
  if !api.logged_in() {
    return Err(
      media_core::Error::auth("insights need a logged-in account or OAuth credentials")
        .with_hint(SETUP_HINT),
    );
  }
  let me = account::whoami(api).await?;
  let (from, to) = window(days);
  let mut ins = Insights {
    kind: "account".into(),
    subject: me.id.clone(),
    title: Some(me.name.clone()),
    url: me.url.clone(),
    from: Some(from.to_string()),
    to: Some(to.to_string()),
    totals_period: Some("lifetime".into()),
    warnings: vec![format!(
      "channel totals and recent uploads only; watch time, audience and traffic sources: {SETUP_HINT}"
    )],
    ..Insights::default()
  };
  ins.total("followers", me.stats.followers.map(Value::from));
  ins.total(
    "views",
    me.stats.other.get("views").copied().map(Value::from),
  );
  ins.total("videos", me.stats.posts.map(Value::from));
  if refs::is_channel_id(&me.id) {
    let since = jiff::Timestamp::now()
      .checked_sub(jiff::SignedDuration::from_hours(24 * i64::from(days)))
      .ok();
    let page = PageReq {
      cursor: None,
      size: 30,
    };
    let uploads = channel::tab(api, &me.id, Tab::Videos, &page).await;
    if let Some(l) = soft(&mut ins, "uploads", uploads)? {
      let recent: Vec<Share> = l
        .posts
        .into_iter()
        .filter(|p| p.created_at.zip(since).is_some_and(|(at, s)| at >= s))
        .map(|p| Share {
          label: p.title.unwrap_or_else(|| p.id.clone()),
          value: p.metrics.views.into(),
          id: Some(p.id),
          ..Share::default()
        })
        .collect();
      ins.extra.insert(
        "uploads_views".into(),
        "current views of the uploads of the window (upload times as YouTube rounds them)".into(),
      );
      ins.breakdowns.push(Breakdown {
        dimension: "uploads_views".into(),
        items: recent,
        ..Breakdown::default()
      });
    }
  }
  ins.raw = me.raw;
  Ok(ins)
}
