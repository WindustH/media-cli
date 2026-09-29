//! Analytics of one post: lifetime numbers of the account's own posts (free),
//! daily series and audience (X Premium), public counters of other posts.

use media_core::text::{one_line, truncate};
use media_core::{Insights, Result, ValueExt, json};

use super::metrics::{self, DIMENSIONS};
use super::window::{self, Window};
use super::{Raw, denied, finish, granted, own_id, premium_required, public_counters};
use crate::api::Api;
use crate::graphql::{POST_AUDIENCE, POST_SERIES, POST_TOTALS};
use crate::tweets;

pub async fn post(api: &Api, arg: &str, days: u32) -> Result<Insights> {
  let post = tweets::read(api, arg).await?;
  let text = post.title.clone().or_else(|| post.text.clone());
  let mut out = Insights {
    kind: "post".into(),
    subject: post.id.clone(),
    title: text.map(|t| truncate(&one_line(&t), 80)),
    url: post.url.clone(),
    ..Insights::default()
  };
  let public = public_counters(&post);
  out
    .extra
    .insert("public_metrics".into(), json!(post.metrics));
  let author = post.author.as_ref().map(|a| a.id.as_str());
  let mine = api.logged_in() && author == Some(own_id(api).await?.as_str());
  let created = post.created_at.map_or_else(window::now, |t| t.as_second());
  let lifetime = Window::since(created);
  lifetime.describe(&mut out);
  if !mine {
    metrics::put_totals(&mut out, public);
    out.extra.insert("source".into(), "public".into());
    let note =
      "not your post: X shows post analytics only to its author; these are its public counters";
    out.extra.insert("note".into(), note.into());
    return Ok(out);
  }
  let mut raw = Vec::new();
  let variables = json!({
    "rest_id": post.id,
    "from_time": lifetime.start_iso(),
    "to_time": lifetime.end_iso(),
    "requested_metrics": metrics::POST_METRICS,
  });
  let analytics = api.graphql(&POST_TOTALS, variables).await.and_then(granted);
  match analytics {
    Ok(data) => {
      let list = data.list("data.tweet_result_by_rest_id.result.organic_metrics_total");
      metrics::put_totals(&mut out, metrics::totals(list));
      raw.push((POST_TOTALS.name, data));
      out.extra.insert("source".into(), "analytics".into());
    }
    Err(e) if denied(&e) => {
      premium_required(&mut out, &e);
      out.extra.insert("source".into(), "public".into());
    }
    Err(e) => return Err(e),
  }
  // Counters the analytics do not have (quotes), or all of them without analytics.
  for (k, v) in public {
    out.totals.entry(k).or_insert(v.into());
  }
  if !out.extra.contains_key("premium_required") {
    let recent = Window::last(days).clip(created);
    premium(api, &post.id, recent, lifetime, &mut out, &mut raw).await;
  }
  finish(&mut out, raw);
  Ok(out)
}

/// Daily series over `recent` and the audience since publication (X Premium).
async fn premium(
  api: &Api,
  id: &str,
  recent: Window,
  lifetime: Window,
  out: &mut Insights,
  raw: &mut Raw,
) {
  let variables = json!({
    "rest_id": id,
    "from_time": recent.start_iso(),
    "to_time": recent.end_iso(),
    "granularity": "Daily",
    "requested_metrics": metrics::POST_SERIES_METRICS,
  });
  match api.graphql(&POST_SERIES, variables).await.and_then(granted) {
    Ok(data) => {
      let rows = data.list("data.result.result.organic_metrics_time_series");
      out.series = metrics::series(rows).0;
      raw.push((POST_SERIES.name, data));
    }
    Err(e) if denied(&e) => return premium_required(out, &e),
    Err(e) => return tracing::warn!("daily post analytics: {e:?}"),
  }
  let (from, to) = lifetime.millis();
  let variables = json!({
    "rest_id": id,
    "dimensions": DIMENSIONS,
    "from_time_incl": from,
    "to_time_excl": to,
  });
  match api
    .graphql(&POST_AUDIENCE, variables)
    .await
    .and_then(granted)
  {
    Ok(data) => {
      let tweet = data.at("data.tweet_result_by_rest_id.result");
      out.breakdowns = metrics::audience(
        tweet.list("uec_metrics_daily_time_series_count"),
        tweet.list("uec_country_metrics_daily_time_series_count"),
      );
      raw.push((POST_AUDIENCE.name, data));
    }
    Err(e) => tracing::warn!("post audience analytics: {e:?}"),
  }
}
