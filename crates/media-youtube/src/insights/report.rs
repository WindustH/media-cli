//! The official YouTube Analytics API (`youtubeanalytics.googleapis.com/v2/reports`)
//! and the Data API's channel / video statistics, with an OAuth bearer.
//!
//! A report is a table: `columnHeaders` (dimensions, then metrics) and `rows`.
//! Metric names (`estimatedMinutesWatched`, `subscribersGained`, ...) and
//! dimensions (`day`, `insightTrafficSourceType`, `ageGroup` ...) are the
//! API's; [`METRICS`] maps them to the shared snake_case keys.

use media_core::{Breakdown, Error, ErrorCode, Point, Result, Series, Share, Value, ValueExt};

use super::oauth::{Credentials, SETUP_HINT};
use crate::api::Api;

const REPORTS: &str = "https://youtubeanalytics.googleapis.com/v2/reports";
const DATA: &str = "https://www.googleapis.com/youtube/v3";

/// API metric → shared key; `averageViewPercentage` becomes a 0..1 ratio.
pub const METRICS: &[(&str, &str)] = &[
  ("views", "views"),
  ("estimatedMinutesWatched", "watch_minutes"),
  ("averageViewDuration", "avg_watch_seconds"),
  ("averageViewPercentage", "avg_view_ratio"),
  ("subscribersGained", "new_followers"),
  ("subscribersLost", "lost_followers"),
  ("likes", "likes"),
  ("dislikes", "dislikes"),
  ("comments", "comments"),
  ("shares", "shares"),
  ("videosAddedToPlaylists", "playlist_adds"),
  ("videosRemovedFromPlaylists", "playlist_removals"),
];

/// Metrics of the daily series.
pub const DAILY: &str = "views,estimatedMinutesWatched,averageViewDuration,subscribersGained,subscribersLost,likes,comments,shares";

/// Distributions: shared name, API dimension, metric.
pub const BREAKDOWNS: &[(&str, &str, &str)] = &[
  ("traffic_source", "insightTrafficSourceType", "views"),
  ("playback_location", "insightPlaybackLocationType", "views"),
  ("device", "deviceType", "views"),
  ("os", "operatingSystem", "views"),
  ("subscribed", "subscribedStatus", "views"),
  ("region", "country", "views"),
  ("sharing_service", "sharingService", "shares"),
];

fn key(metric: &str) -> &str {
  METRICS
    .iter()
    .find(|(m, _)| *m == metric)
    .map_or(metric, |(_, k)| k)
}

/// A cell in shared units (percentages as fractions).
fn cell(metric: &str, v: &Value) -> Value {
  match (metric, v.as_f64()) {
    ("averageViewPercentage" | "viewerPercentage", Some(p)) => Value::from(p / 100.0),
    _ => v.clone(),
  }
}

/// One report table.
pub struct Table {
  headers: Vec<String>,
  pub rows: Vec<Vec<Value>>,
  pub raw: Value,
}

impl Table {
  fn col(&self, name: &str) -> Option<usize> {
    self.headers.iter().position(|h| h == name)
  }

  fn metrics(&self) -> impl Iterator<Item = (usize, &str)> {
    self
      .headers
      .iter()
      .enumerate()
      .filter(|(_, h)| METRICS.iter().any(|(m, _)| m == h) || *h == "viewerPercentage")
      .map(|(i, h)| (i, h.as_str()))
  }

  /// The single row of a report without dimensions, as shared keys.
  pub fn totals(&self) -> Vec<(String, Value)> {
    let Some(row) = self.rows.first() else {
      return Vec::new();
    };
    self
      .metrics()
      .filter_map(|(i, m)| Some((key(m).to_owned(), cell(m, row.get(i)?))))
      .collect()
  }

  /// One series per metric of a `day` report.
  pub fn series(&self) -> Vec<Series> {
    let Some(day) = self.col("day") else {
      return Vec::new();
    };
    self
      .metrics()
      .map(|(i, m)| Series {
        metric: key(m).to_owned(),
        points: self
          .rows
          .iter()
          .filter_map(|r| {
            Some(Point {
              date: r.get(day)?.as_str()?.to_owned(),
              value: cell(m, r.get(i)?),
            })
          })
          .collect(),
      })
      .collect()
  }

  /// `(label, value)` of a one-dimension report, first metric.
  pub fn pairs(&self) -> Vec<(String, Value)> {
    let metric = self.metrics().next();
    self
      .rows
      .iter()
      .filter_map(|r| {
        let (i, m) = metric?;
        Some((r.first()?.str("")?, cell(m, r.get(i)?)))
      })
      .collect()
  }
}

/// Slices with ratios of their sum.
pub fn breakdown(dimension: &str, pairs: Vec<(String, Value, Option<String>)>) -> Breakdown {
  let sum: f64 = pairs.iter().filter_map(|(_, v, _)| v.as_f64()).sum();
  Breakdown {
    dimension: dimension.into(),
    items: pairs
      .into_iter()
      .map(|(label, value, id)| Share {
        ratio: value.as_f64().filter(|_| sum > 0.0).map(|v| v / sum),
        label,
        value,
        id,
      })
      .collect(),
    ..Breakdown::default()
  }
}

/// API labels (`YT_SEARCH`, `age18-24`, `MOBILE`) in the shared style.
pub fn label(dimension: &str, raw: &str) -> String {
  match dimension {
    "country" => raw.to_owned(),
    "ageGroup" => raw.trim_start_matches("age").to_owned(),
    _ => raw.to_lowercase(),
  }
}

pub struct Client<'a> {
  pub api: &'a Api,
  pub creds: &'a Credentials,
}

impl Client<'_> {
  async fn get(&self, url: &str, query: &[(&str, String)]) -> Result<Value> {
    let ctx = &self.api.ctx;
    let token = self.creds.token(ctx).await?;
    let resp = ctx
      .http
      .get(url)
      .no_cookies()
      .header("authorization", format!("Bearer {token}"))
      .queries(query.iter().map(|(k, v)| (*k, v)))
      .send()
      .await?;
    let v = resp.value().unwrap_or_default();
    if resp.status.is_success() {
      return Ok(v);
    }
    let message = v
      .str("error.message")
      .unwrap_or_else(|| format!("HTTP {}", resp.status));
    Err(match resp.status.as_u16() {
      401 => {
        self.creds.forget(ctx);
        Error::auth(format!("the OAuth token was refused: {message}")).with_hint(SETUP_HINT)
      }
      403 => Error::new(ErrorCode::PermissionDenied, message),
      400 => Error::input(format!("YouTube Analytics: {message}")),
      429 => Error::new(ErrorCode::RateLimited, message),
      _ => Error::upstream(format!("YouTube Analytics: {message}")),
    })
  }

  /// A report of the authorized channel (`video` restricts it to one video).
  pub async fn report(
    &self,
    metrics: &str,
    dims: Option<&str>,
    range: (&str, &str),
    video: Option<&str>,
    sort: Option<&str>,
    max: Option<u32>,
  ) -> Result<Table> {
    let mut q = vec![
      ("ids", "channel==MINE".to_owned()),
      ("startDate", range.0.to_owned()),
      ("endDate", range.1.to_owned()),
      ("metrics", metrics.to_owned()),
    ];
    if let Some(d) = dims {
      q.push(("dimensions", d.to_owned()));
    }
    if let Some(v) = video {
      q.push(("filters", format!("video=={v}")));
    }
    if let Some(s) = sort {
      q.push(("sort", s.to_owned()));
    }
    if let Some(m) = max {
      q.push(("maxResults", m.to_string()));
    }
    let v = self.get(REPORTS, &q).await?;
    Ok(Table {
      headers: v
        .list("columnHeaders")
        .iter()
        .filter_map(|h| h.str("name"))
        .collect(),
      rows: v
        .list("rows")
        .iter()
        .map(|r| r.as_array().cloned().unwrap_or_default())
        .collect(),
      raw: v,
    })
  }

  /// The authorized channel (`snippet`, `statistics`).
  pub async fn channel(&self) -> Result<Value> {
    let q = [
      ("part", "snippet,statistics".to_owned()),
      ("mine", "true".to_owned()),
    ];
    let v = self.get(&format!("{DATA}/channels"), &q).await?;
    Ok(v.at("items.0").clone())
  }

  /// Videos by id (`snippet`, `statistics`), at most 50.
  pub async fn videos(&self, ids: &[String]) -> Result<Vec<Value>> {
    let q = [
      ("part", "snippet,statistics".to_owned()),
      ("id", ids.join(",")),
    ];
    let v = self.get(&format!("{DATA}/videos"), &q).await?;
    Ok(v.list("items").to_vec())
  }
}
