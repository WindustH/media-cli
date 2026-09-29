//! Message center: replies / mentions / likes received, and unread counters.

use std::collections::BTreeMap;

use media_core::{Ctx, Notification, Page, PageReq, Result, Value, ValueExt};

use crate::{api, parse};

const UNREAD: &str = "https://api.bilibili.com/x/msgfeed/unread";

/// `kind`: `reply` (default), `at` or `like`. The cursor is `id:time` of the last item.
pub async fn notifications(
  ctx: &Ctx,
  kind: Option<&str>,
  page: &PageReq,
) -> Result<Page<Notification>> {
  ctx.require_login(api::LOGIN_COOKIES)?;
  let (path, time_key, label, list) = match kind.unwrap_or("reply") {
    "at" => ("at", "at_time", "mention", "items"),
    "like" => ("like", "like_time", "like", "total.items"),
    _ => ("reply", "reply_time", "reply", "items"),
  };
  let mut call = api::get(ctx, &format!("https://api.bilibili.com/x/msgfeed/{path}"))
    .arg("platform", "web")
    .arg("build", 0)
    .arg("mobi_app", "web");
  if let Some((id, time)) = page.cursor.as_deref().and_then(|c| c.split_once(':')) {
    call = call.arg("id", id).arg(time_key, time);
  }
  let data = call.send().await?;
  let cursor = if label == "like" {
    data.at("total.cursor")
  } else {
    data.at("cursor")
  };
  let items = data
    .list(list)
    .iter()
    .map(|v| parse::notification(v, label))
    .collect();
  Ok(Page::new(items, next(cursor)))
}

fn next(cursor: &Value) -> Option<String> {
  if cursor.bool("is_end") != Some(false) {
    return None;
  }
  Some(format!("{}:{}", cursor.str("id")?, cursor.str("time")?))
}

/// Counters of unread replies, mentions, likes, system and chat messages.
pub async fn unread(ctx: &Ctx) -> Result<BTreeMap<String, u64>> {
  ctx.require_login(api::LOGIN_COOKIES)?;
  let data = api::get(ctx, UNREAD).send().await?;
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
