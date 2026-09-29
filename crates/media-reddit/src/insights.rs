//! Analytics. Reddit has no creator dashboard for ordinary accounts, so the
//! account's insights are what it reports natively (karma, also per
//! community, trophies, followers, age) plus daily trends computed from its
//! own posts and comments. A post's insights are its public counters, and
//! for the author also the post insights page ([`crate::poststats`]).

use std::collections::BTreeMap;

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use media_core::{
  Breakdown, ErrorCode, Insights, PageReq, Point, Result, Series, Share, Value, ValueExt, json,
};

use crate::api::Api;
use crate::refs::user_url;
use crate::{listing, posts, poststats};

/// Pages of 100 own posts / comments read for the trends.
const MAX_PAGES: usize = 10;

fn day(secs: i64, tz: &TimeZone) -> Option<Date> {
  Timestamp::from_second(secs)
    .ok()
    .map(|t| t.to_zoned(tz.clone()).date())
}

/// Posts (`submitted`) or comments of `name` created since `since`, newest first.
async fn own(api: &Api, name: &str, which: &str, since: i64) -> Result<Vec<Value>> {
  let mut out = Vec::new();
  let mut req = PageReq {
    cursor: None,
    size: 100,
  };
  for _ in 0..MAX_PAGES {
    let query = listing::paged(vec![("sort", "new".into())], &req);
    let v = api.get(&format!("/user/{name}/{which}"), &query).await?;
    let items = v.list("data.children");
    out.extend(
      items
        .iter()
        .map(|t| t.at("data"))
        .filter(|d| d.i64("created_utc").unwrap_or(0) >= since)
        .cloned(),
    );
    let past = items
      .last()
      .is_some_and(|t| t.i64("data.created_utc").unwrap_or(0) < since);
    req.cursor = v.str("data.after");
    if past || req.cursor.is_none() {
      break;
    }
  }
  Ok(out)
}

/// A part the insights can do without: `null` and a note when it fails for
/// another reason than the session or the rate limit.
fn optional(r: Result<Value>, what: &str, missing: &mut Vec<String>) -> Result<Value> {
  match r {
    Err(e) if matches!(e.code, ErrorCode::NotAuthenticated | ErrorCode::RateLimited) => Err(e),
    Err(e) => {
      missing.push(format!("{what}: {}", e.message));
      Ok(Value::Null)
    }
    ok => ok,
  }
}

/// Daily sums of `field` (`None`: count items) per day of `dates`.
fn daily(items: &[Value], field: Option<&str>, dates: &[Date], tz: &TimeZone) -> Vec<i64> {
  let mut sums = vec![0; dates.len()];
  for d in items {
    let Some(date) = d.i64("created_utc").and_then(|s| day(s, tz)) else {
      continue;
    };
    if let Ok(i) = dates.binary_search(&date) {
      sums[i] += field.map_or(1, |f| d.i64(f).unwrap_or(0));
    }
  }
  sums
}

/// Slices sorted by value, largest first, with their share of the whole.
fn breakdown(dimension: &str, counts: BTreeMap<String, i64>) -> Option<Breakdown> {
  let whole: i64 = counts.values().filter(|n| **n > 0).sum();
  let mut items: Vec<(String, i64)> = counts.into_iter().filter(|(_, n)| *n != 0).collect();
  items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
  let items: Vec<Share> = items
    .into_iter()
    .map(|(label, n)| Share {
      label,
      value: n.into(),
      ratio: (whole > 0 && n > 0).then(|| n as f64 / whole as f64),
      ..Default::default()
    })
    .collect();
  (!items.is_empty()).then(|| Breakdown {
    dimension: dimension.into(),
    items,
    ..Default::default()
  })
}

/// The logged-in account over the last `days` days.
pub async fn account(api: &Api, days: u32) -> Result<Insights> {
  let me = api.me().await?;
  let name = me.str("name").unwrap_or_default();
  let tz = TimeZone::system();
  let today = Timestamp::now().to_zoned(tz.clone()).date();
  let dates: Vec<Date> = (0..days as i64)
    .rev()
    .filter_map(|i| today.checked_sub(jiff::Span::new().days(i)).ok())
    .collect();
  let since = dates
    .first()
    .and_then(|d| d.to_zoned(tz.clone()).ok())
    .map_or(0, |z| z.timestamp().as_second());
  let mut ins = Insights {
    kind: "account".into(),
    subject: name.clone(),
    title: Some(format!("u/{name}")),
    url: Some(user_url(&name)),
    from: dates.first().map(ToString::to_string),
    to: dates.last().map(ToString::to_string),
    ..Insights::default()
  };
  // Native: the account, its karma per community and its trophies.
  ins.total("karma", me.i64("total_karma").map(Value::from));
  ins.total("post_karma", me.i64("link_karma").map(Value::from));
  ins.total("comment_karma", me.i64("comment_karma").map(Value::from));
  ins.total("awarder_karma", me.i64("awarder_karma").map(Value::from));
  ins.total("awardee_karma", me.i64("awardee_karma").map(Value::from));
  ins.total(
    "followers",
    me.u64("subreddit.subscribers").map(Value::from),
  );
  let age = me
    .i64("created_utc")
    .map(|c| (Timestamp::now().as_second() - c) / 86_400);
  ins.total("account_age_days", age.map(Value::from));
  let mut missing = Vec::new();
  let karma = optional(
    api.get_oauth("/api/v1/me/karma", &[]).await,
    "karma",
    &mut missing,
  )?;
  for (dimension, keys) in [
    ("karma_by_subreddit", &["link_karma", "comment_karma"][..]),
    ("post_karma_by_subreddit", &["link_karma"][..]),
    ("comment_karma_by_subreddit", &["comment_karma"][..]),
  ] {
    let counts = karma
      .list("data")
      .iter()
      .filter_map(|k| {
        let sum = keys.iter().map(|f| k.i64(f).unwrap_or(0)).sum();
        Some((k.str("sr")?, sum))
      })
      .collect();
    ins.breakdowns.extend(breakdown(dimension, counts));
  }
  let trophies = optional(
    api.get_oauth("/api/v1/me/trophies", &[]).await,
    "trophies",
    &mut missing,
  )?;
  let list: Vec<Value> = trophies
    .list("data.trophies")
    .iter()
    .map(|t| t.at("data"))
    .map(|t| {
      let granted = t.i64("granted_at").and_then(|s| day(s, &tz));
      json!({ "name": t.str("name"), "granted_at": granted.map(|d| d.to_string()),
        "description": t.str("description") })
    })
    .collect();
  if !trophies.is_null() {
    ins.total("trophies", Some(Value::from(list.len())));
    ins.extra.insert("trophies".into(), list.into());
  }
  // Computed: the account's own posts and comments created in the window.
  let posts = own(api, &name, "submitted", since).await?;
  let comments = own(api, &name, "comments", since).await?;
  let metrics = [
    ("posts", &posts, None),
    ("likes", &posts, Some("score")),
    ("comments", &posts, Some("num_comments")),
    ("crossposts", &posts, Some("num_crossposts")),
    ("comments_written", &comments, None),
    ("comment_likes", &comments, Some("score")),
  ];
  for (metric, items, field) in metrics {
    let sums = daily(items, field, &dates, &tz);
    ins.total(metric, Some(sums.iter().sum::<i64>().into()));
    let points = dates
      .iter()
      .zip(sums)
      .map(|(d, v)| Point {
        date: d.to_string(),
        value: v.into(),
      })
      .collect();
    ins.series.push(Series {
      metric: metric.into(),
      points,
    });
  }
  let ratios: Vec<f64> = posts.iter().filter_map(|p| p.f64("upvote_ratio")).collect();
  if !ratios.is_empty() {
    let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
    ins.total("upvote_ratio", Some(mean.into()));
  }
  for (dimension, items) in [
    ("posts_by_subreddit", &posts),
    ("comments_by_subreddit", &comments),
  ] {
    let mut counts = BTreeMap::new();
    for d in items {
      *counts
        .entry(d.str("subreddit").unwrap_or_default())
        .or_default() += 1;
    }
    ins.breakdowns.extend(breakdown(dimension, counts));
  }
  let x = &mut ins.extra;
  x.insert(
    "native".into(),
    json!([
      "karma",
      "post_karma",
      "comment_karma",
      "awarder_karma",
      "awardee_karma",
      "followers",
      "account_age_days",
      "trophies",
      "*karma_by_subreddit"
    ]),
  );
  x.insert(
    "computed".into(),
    "posts, comments and upvote_ratio over the window (and their series and \
     *_by_subreddit breakdowns) are computed from the account's own posts and comments \
     created in it: likes is the score, comments the comments received, both as they are \
     now, counted on the day the post or comment was created"
      .into(),
  );
  ins.warnings.extend(missing);
  ins.raw = Some(json!({ "me": me, "karma": karma, "trophies": trophies }));
  Ok(ins)
}

/// One post: its public counters, and the post insights page for the author.
pub async fn post(api: &Api, arg: &str) -> Result<Insights> {
  let post = posts::fetch(api, arg).await?;
  let d = post.raw.clone().unwrap_or_default();
  let tz = TimeZone::system();
  let mut ins = Insights {
    kind: "post".into(),
    subject: post.id.clone(),
    title: post.title.clone(),
    url: post.url.clone(),
    from: d
      .i64("created_utc")
      .and_then(|s| day(s, &tz))
      .map(|d| d.to_string()),
    to: Some(Timestamp::now().to_zoned(tz).date().to_string()),
    ..Insights::default()
  };
  ins.total("views", d.u64("view_count").map(Value::from));
  ins.total("likes", d.i64("score").map(Value::from));
  ins.total("upvote_ratio", d.f64("upvote_ratio").map(Value::from));
  ins.total("comments", d.u64("num_comments").map(Value::from));
  ins.total("crossposts", d.u64("num_crossposts").map(Value::from));
  ins.total("awards", d.u64("total_awards_received").map(Value::from));
  let x = &mut ins.extra;
  x.insert("likes".into(), "score (upvotes minus downvotes)".into());
  x.insert(
    "subreddit_subscribers".into(),
    d.u64("subreddit_subscribers").into(),
  );
  let author = post.author.as_ref().and_then(|a| a.handle.clone());
  let mine = match (api.logged_in(), author) {
    (true, Some(author)) => author.eq_ignore_ascii_case(&api.username().await?),
    _ => false,
  };
  if !mine {
    x.insert(
      "source".into(),
      "public counters: Reddit shows views, shares and traffic of a post to its author only".into(),
    );
    ins.raw = Some(d);
    return Ok(ins);
  }
  match poststats::fetch(api, &post.id).await {
    Ok(Some(stats)) => {
      x.insert(
        "source".into(),
        "public counters and the author's post insights page".into(),
      );
      stats.merge_into(&mut ins);
    }
    Ok(None) => {
      x.insert(
        "source".into(),
        "public counters: Reddit has no insights for this post (it keeps them for about \
         90 days, and not for removed or deleted posts)"
          .into(),
      );
    }
    Err(e) if e.code == ErrorCode::NotAuthenticated => return Err(e),
    Err(e) => {
      x.insert(
        "source".into(),
        format!(
          "public counters: the post insights page failed: {}",
          e.message
        )
        .into(),
      );
    }
  }
  ins.raw = Some(d);
  Ok(ins)
}
