//! `insights`: creator analytics from the web client's analytics pages
//! (x.com/i/account_analytics). Daily trends, audience and per-post content
//! analytics need X Premium. Without it the account gets the free rollup (the
//! numbers X shows every account) and totals of its recent posts; its own
//! posts still get their lifetime analytics. Other posts get public counters.

mod account;
mod metrics;
mod post;
mod window;

use std::collections::BTreeMap;

use media_core::{Error, ErrorCode, Insights, Post, Result, Value, ValueExt, json};

pub use account::account;
pub use post::post;

use crate::api::Api;
use crate::users;

const PREMIUM_HINT: &str = "X shows daily trends, audience and content analytics to X Premium \
  subscribers only; without it you get the lifetime numbers of your own posts \
  (`media x insights POST`) and the account's rollup of the last 8 days";

/// Upstream answers kept for `raw`, by operation name.
type Raw = Vec<(&'static str, Value)>;

/// The answer of an analytics query, or the error X put next to the refused field.
fn granted(data: Value) -> Result<Value> {
  let Some(e) = data.list("errors").first() else {
    return Ok(data);
  };
  let message = e.str("message").unwrap_or_default();
  if e.str("kind").as_deref() == Some("Permissions") || e.i64("code") == Some(37) {
    tracing::debug!("analytics refused: {message}");
    return Err(
      Error::new(
        ErrorCode::PermissionDenied,
        "these analytics need X Premium",
      )
      .with_hint(PREMIUM_HINT),
    );
  }
  Err(Error::upstream(message))
}

fn denied(e: &Error) -> bool {
  e.code == ErrorCode::PermissionDenied
}

/// Record that the Premium part was refused, in `extra` and on stderr.
fn premium_required(out: &mut Insights, e: &Error) {
  tracing::warn!("{}: {}", e.message, e.hint.as_deref().unwrap_or_default());
  out.extra.insert(
    "premium_required".into(),
    json!({ "code": e.code.as_str(), "message": e.message, "hint": e.hint }),
  );
}

async fn own_id(api: &Api) -> Result<String> {
  api.require_login()?;
  users::user_id_or_self(api, None).await
}

/// Finish: derived rates, X's names of renamed analytics metrics, the upstream answers.
fn finish(out: &mut Insights, raw: Raw) {
  metrics::derive(out);
  let source = out.extra.get("source").and_then(Value::as_str);
  if matches!(source, Some("analytics" | "free_rollup")) {
    metrics::upstream_names(out);
  }
  out.raw = Some(json!(raw.into_iter().collect::<BTreeMap<_, _>>()));
}

/// A post's public counters under the insights keys.
fn public_counters(p: &Post) -> BTreeMap<String, u64> {
  let m = &p.metrics;
  [
    ("views", m.views),
    ("likes", m.likes),
    ("comments", m.comments),
    ("shares", m.shares),
    ("favorites", m.favorites),
    ("quotes", m.other.get("quotes").copied()),
  ]
  .into_iter()
  .filter_map(|(k, v)| Some((k.to_owned(), v?)))
  .collect()
}
