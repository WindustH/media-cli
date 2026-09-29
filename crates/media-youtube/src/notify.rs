//! The bell: `notification/get_notification_menu` (inbox, paged by `ctoken`)
//! and `notification/get_unseen_count`, as YouTube.js calls them.

use std::collections::BTreeMap;

use media_core::{Notification, Page, PageReq, Result, Value, ValueExt, json};

use crate::api::Api;
use crate::parse;
use crate::refs::{comment_url, video_url};

const INBOX: &str = "NOTIFICATIONS_MENU_REQUEST_TYPE_INBOX";

/// What a notification is about, from its wording.
fn kind(text: &str) -> &'static str {
  let t = text.to_lowercase();
  if t.contains("replied") {
    "reply"
  } else if t.contains("mentioned") {
    "mention"
  } else if t.contains("liked") || t.contains("hearted") {
    "like"
  } else if t.contains("commented") {
    "comment"
  } else if t.contains("subscribed") {
    "follow"
  } else if t.contains(" is live") || t.contains("premiering") || t.contains("live now") {
    "live"
  } else if t.contains("uploaded") || t.contains("posted") {
    "upload"
  } else {
    "system"
  }
}

fn notification(r: &Value) -> Option<Notification> {
  let text = parse::text(r.at("shortMessage")).unwrap_or_default();
  let sent = parse::text(r.at("sentTimeText"));
  let nav = r.at("navigationEndpoint");
  let video = nav
    .str("watchEndpoint.videoId")
    .or_else(|| nav.str("reelWatchEndpoint.videoId"));
  let comment = parse::find(nav, "linkedCommentId")
    .first()
    .and_then(|c| c.as_str().map(str::to_owned));
  let url = match (&video, &comment) {
    (Some(v), Some(c)) => Some(comment_url(v, c)),
    (Some(v), None) => Some(video_url(v)),
    _ => nav
      .str("commandMetadata.webCommandMetadata.url")
      .map(|u| format!("{}{u}", crate::api::WWW)),
  };
  Some(Notification {
    id: r.str("notificationId")?,
    kind: kind(&text).into(),
    // `Channel uploaded: Title` / `@x replied: "…"`.
    target: text.split_once(": ").map(|(_, t)| t.to_owned()),
    created_at: sent.as_deref().and_then(parse::ago),
    unread: r.bool("read").map(|read| !read),
    raw: Some(r.clone()),
    url,
    text,
    ..Notification::default()
  })
}

pub async fn list(api: &Api, req: &PageReq) -> Result<Page<Notification>> {
  api.require_login()?;
  let mut body = json!({ "notificationsMenuRequestType": INBOX });
  if let Some(token) = &req.cursor {
    body["ctoken"] = token.clone().into();
  }
  let v = api.call("notification/get_notification_menu", body).await?;
  let items = parse::find(&v, "notificationRenderer")
    .into_iter()
    .filter_map(notification)
    .collect();
  let next = parse::find(&v, "continuationItemRenderer")
    .into_iter()
    .find_map(parse::token);
  Ok(Page::new(items, next))
}

pub async fn unread(api: &Api) -> Result<BTreeMap<String, u64>> {
  api.require_login()?;
  let v = api.call("notification/get_unseen_count", json!({})).await?;
  let n = v
    .u64("unseenCount")
    .or_else(|| v.u64("actions.0.updateNotificationsUnseenCountAction.unseenCount"))
    .unwrap_or(0);
  Ok(BTreeMap::from([("notifications".to_owned(), n)]))
}
