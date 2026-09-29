//! Notifications and unread counters.

use std::collections::BTreeMap;

use media_core::{Error, Notification, Page, PageReq, Result, ValueExt};

use crate::api::Client;
use crate::parse;

/// `notifications --type` values; the first is the default.
pub const KINDS: &[&str] = &["mentions", "likes", "connections"];

pub async fn notifications(
  c: &Client,
  kind: Option<&str>,
  page: &PageReq,
) -> Result<Page<Notification>> {
  c.require_login()?;
  let (path, fallback) = match kind.unwrap_or(KINDS[0]) {
    "mentions" => ("/api/sns/web/v1/you/mentions", "mention"),
    "likes" => ("/api/sns/web/v1/you/likes", "like"),
    "connections" => ("/api/sns/web/v1/you/connections", "follow"),
    other => return Err(Error::input(format!("unknown notification type `{other}`"))),
  };
  let num = page.size_within(20).to_string();
  let cursor = page.cursor.clone().unwrap_or_default();
  let data = c
    .get(path, &[("num", num.as_str()), ("cursor", cursor.as_str())])
    .await?;
  let items = data
    .list("message_list")
    .iter()
    .map(|m| parse::notification(m, fallback))
    .collect();
  let next = data
    .first_str(&["cursor", "str_cursor", "strCursor"])
    .filter(|_| data.bool("has_more") == Some(true));
  Ok(Page::new(items, next))
}

/// `unread_count`, `mentions`, `likes`, `connections` and any other counter.
pub async fn unread(c: &Client) -> Result<BTreeMap<String, u64>> {
  c.require_login()?;
  let data = c.get("/api/sns/web/unread_count", &[]).await?;
  Ok(
    data
      .as_object()
      .map(|m| {
        m.iter()
          .filter_map(|(k, v)| Some((k.clone(), v.as_u64()?)))
          .collect()
      })
      .unwrap_or_default(),
  )
}
