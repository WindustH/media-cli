//! Analytics of the logged-in account: daily metrics, audience and top posts
//! with X Premium; the free rollup and its recent posts without.

use std::collections::BTreeMap;

use media_core::{Breakdown, Insights, PageReq, Post, Result, Value, ValueExt, json};

use super::metrics::{self, DIMENSIONS};
use super::window::Window;
use super::{Raw, denied, finish, granted, own_id, premium_required, public_counters};
use crate::api::Api;
use crate::graphql::{ACCOUNT_AUDIENCE, ACCOUNT_SERIES, CONTENT, FREE_ROLLUP};
use crate::refs::{UserRef, tweet_url};
use crate::timeline::PAGE_MAX;
use crate::{tweets, users};

/// Timeline pages to read for the posts of the window (without Premium).
const MAX_PAGES: usize = 5;
/// Posts in the `top_posts` breakdown.
const TOP_POSTS: usize = 10;
/// Longest window of the free rollup ("max lookback cannot be greater than 691200 seconds").
const FREE_DAYS: u32 = 8;

pub async fn account(api: &Api, days: u32) -> Result<Insights> {
  let id = own_id(api).await?;
  let window = Window::last(days);
  let mut out = Insights {
    kind: "account".into(),
    subject: id.clone(),
    ..Insights::default()
  };
  window.describe(&mut out);
  let mut raw = Vec::new();
  let handle = match users::lookup(api, &UserRef::Id(id.clone())).await {
    Ok(user) => {
      out.subject = user
        .handle
        .as_deref()
        .map_or(id.clone(), |h| format!("@{h}"));
      out.title = Some(user.name.clone());
      out.url = user.url.clone();
      out.total("followers", user.stats.followers.map(Value::from));
      out.total("following", user.stats.following.map(Value::from));
      user.handle
    }
    Err(e) => {
      tracing::warn!("profile of {id}: {e:?}");
      None
    }
  };
  let variables = json!({
    "rest_id": id,
    "from_time": window.start_iso(),
    "to_time": window.end_iso(),
    "granularity": "Daily",
    "requested_metrics": metrics::ACCOUNT_METRICS,
  });
  match api
    .graphql(&ACCOUNT_SERIES, variables)
    .await
    .and_then(granted)
  {
    Ok(data) => {
      let rows = data.list("data.result.result.organic_metrics_time_series");
      let (series, sums) = metrics::series(rows);
      out.series = series;
      metrics::put_totals(&mut out, sums);
      raw.push((ACCOUNT_SERIES.name, data));
      premium(api, &id, handle.as_deref(), window, &mut out, &mut raw).await;
      out.extra.insert("source".into(), "analytics".into());
    }
    Err(e) if denied(&e) => {
      premium_required(&mut out, &e);
      free(api, &id, days, &mut out, &mut raw).await?;
    }
    Err(e) => return Err(e),
  }
  finish(&mut out, raw);
  Ok(out)
}

/// Audience breakdowns, views by hour and the top posts (X Premium).
async fn premium(
  api: &Api,
  id: &str,
  handle: Option<&str>,
  window: Window,
  out: &mut Insights,
  raw: &mut Raw,
) {
  let (from, to) = window.millis();
  let variables = json!({
    "dimensions": DIMENSIONS,
    "from_time_incl": from,
    "to_time_excl": to,
    "heatmap_from_time_incl": window.start_iso(),
    "heatmap_to_time_excl": window.end_iso(),
    "requested_metrics": ["Impressions"],
  });
  match api
    .graphql(&ACCOUNT_AUDIENCE, variables)
    .await
    .and_then(granted)
  {
    Ok(data) => {
      let user = data.at("data.viewer_v2.user_results.result");
      out.breakdowns = metrics::audience(
        user.list("uec_metrics_daily_time_series_count"),
        user.list("uec_country_metrics_daily_time_series_count"),
      );
      out
        .breakdowns
        .extend(metrics::hours(user.list("organic_metrics_time_series")));
      raw.push((ACCOUNT_AUDIENCE.name, data));
    }
    Err(e) => tracing::warn!("audience analytics: {e:?}"),
  }
  let variables = json!({
    "rest_id": id,
    "from_time": window.start_iso(),
    "to_time": window.end_iso(),
    "max_results": 1000,
    "query_page_size": 100,
    "requested_metrics": ["Impressions"],
  });
  match api.graphql(&CONTENT, variables).await.and_then(granted) {
    Ok(data) => {
      let posts = data
        .list("data.user_result_by_rest_id.result.tweets_results")
        .iter()
        .filter_map(|t| {
          let t = t.at("result");
          let id = t.str("rest_id")?;
          let views = *metrics::totals(t.list("organic_metrics_total")).get("views")?;
          Some((tweet_url(handle.unwrap_or("i"), &id), views))
        })
        .collect();
      out.breakdowns.extend(top_posts(posts));
      raw.push((CONTENT.name, data));
    }
    Err(e) => tracing::warn!("content analytics: {e:?}"),
  }
}

/// Without Premium: the free rollup (of the window when it is short enough,
/// else of the last week, in `extra`), and totals of the posts published in
/// the window (their current public counters).
async fn free(api: &Api, id: &str, days: u32, out: &mut Insights, raw: &mut Raw) -> Result<()> {
  let window = Window::last(days);
  let recent = Window::last(days.min(FREE_DAYS));
  let variables = json!({
    "rest_id": id,
    "from_time": recent.start_iso(),
    "to_time": recent.end_iso(),
    "requested_metrics": metrics::FREE_METRICS,
  });
  let rollup = api.graphql(&FREE_ROLLUP, variables).await.and_then(granted);
  let has_rollup = match rollup {
    Ok(data) => {
      let sums = metrics::totals(data.list("data.result.result.free_metrics_rollup"));
      if days <= FREE_DAYS {
        metrics::put_totals(out, sums);
      } else {
        let (from, to) = recent.days();
        let mut v = json!({ "from": from, "to": to });
        for (k, n) in sums {
          v[k.as_str()] = n.into();
        }
        out.extra.insert("free_rollup".into(), v);
      }
      raw.push((FREE_ROLLUP.name, data));
      days <= FREE_DAYS
    }
    Err(e) => {
      tracing::warn!("free analytics rollup: {e:?}");
      false
    }
  };
  let posts = recent_posts(api, id, window).await?;
  let mut sums: BTreeMap<String, u64> = BTreeMap::new();
  let mut per_day: BTreeMap<String, u64> = BTreeMap::new();
  for p in &posts {
    for (k, v) in public_counters(p) {
      *sums.entry(k).or_default() += v;
    }
    if let Some(t) = p.created_at {
      let day = t.strftime("%Y-%m-%d").to_string();
      *per_day.entry(day).or_default() += 1;
    }
  }
  out.total("posts", Value::from(posts.len()));
  let mut own = json!(sums);
  own["posts"] = posts.len().into();
  out.extra.insert("own_posts".into(), own);
  let source = if has_rollup {
    "free_rollup"
  } else {
    metrics::put_totals(out, sums);
    "own_posts"
  };
  out.extra.insert("source".into(), source.into());
  out.series.push(window.daily("posts", &per_day));
  let top = posts
    .iter()
    .filter_map(|p| Some((p.url.clone()?, p.metrics.views?)))
    .collect();
  out.breakdowns.extend(top_posts(top));
  Ok(())
}

fn top_posts(mut posts: Vec<(String, u64)>) -> Option<Breakdown> {
  posts.sort_by_key(|(_, v)| std::cmp::Reverse(*v));
  posts.truncate(TOP_POSTS);
  metrics::ranked("top_posts", posts)
}

/// The account's own posts (not retweets) published inside `window`, from its timeline.
async fn recent_posts(api: &Api, id: &str, window: Window) -> Result<Vec<Post>> {
  let mut out = Vec::new();
  let mut req = PageReq {
    cursor: None,
    size: PAGE_MAX,
  };
  for _ in 0..MAX_PAGES {
    let page = tweets::user_posts(api, id, &req).await?;
    let own: Vec<Post> = page
      .items
      .into_iter()
      .filter(|p| p.author.as_ref().is_some_and(|a| a.id == id))
      .filter(|p| !p.extra.contains_key("retweeted_by"))
      .collect();
    let at = |p: &Post| p.created_at.map_or(i64::MAX, |t| t.as_second());
    let past = !own.is_empty() && own.iter().all(|p| at(p) < window.from);
    out.extend(own.into_iter().filter(|p| window.contains(at(p))));
    match page.next_cursor {
      Some(next) if page.has_more && !past => req.cursor = Some(next),
      _ => break,
    }
  }
  Ok(out)
}
