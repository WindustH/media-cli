//! Notifications (`NotificationsTimeline`): aggregated events (likes,
//! reposts, follows ...) and the tweets that mention or answer you.

use media_core::text::{from_millis, parse_time};
use media_core::{Notification, Page, PageReq, Result, Value, ValueExt, json};

use crate::api::Api;
use crate::graphql::NOTIFICATIONS;
use crate::parse;
use crate::timeline::{self, Timeline, vars, with};

const PATHS: &[&str] =
  &["data.viewer_v2.user_results.result.notification_timeline.timeline.instructions"];

/// `kind`: all (default), verified or mentions.
pub async fn list(api: &Api, kind: Option<&str>, req: &PageReq) -> Result<Page<Notification>> {
  api.require_login()?;
  let timeline_type = match kind {
    Some("verified") => "Verified",
    Some("mentions") => "Mentions",
    _ => "All",
  };
  let variables = with(vars(req), json!({ "timeline_type": timeline_type }));
  timeline::page(
    api,
    &NOTIFICATIONS,
    variables,
    PATHS,
    req,
    |tl: &Timeline| {
      tl.items()
        .filter_map(|(id, item)| notification(id, item))
        .collect()
    },
  )
  .await
}

fn notification(entry_id: &str, item: &Value) -> Option<Notification> {
  if item.at("tweet_results").is_object() {
    return mention(entry_id, item);
  }
  if item.str("__typename").as_deref() != Some("TimelineNotification") {
    return None;
  }
  let template = item.at("template");
  let target = template
    .list("target_objects")
    .iter()
    .find_map(|t| parse::post(t.at("tweet_results.result")));
  Some(Notification {
    id: item.str("id").unwrap_or_else(|| entry_id.to_owned()),
    kind: kind(item.str("notification_icon").as_deref().unwrap_or_default()).into(),
    text: item.str("rich_message.text").unwrap_or_default(),
    actor: template
      .list("from_users")
      .iter()
      .find_map(|u| parse::user(u.at("user_results.result"))),
    target: target.as_ref().and_then(|p| p.text.clone()),
    url: target.and_then(|p| p.url).or_else(|| {
      item
        .str("notification_url.url")
        .filter(|u| u.starts_with("http"))
    }),
    created_at: item.str("timestamp_ms").and_then(|t| {
      t.parse::<i64>()
        .ok()
        .and_then(from_millis)
        .or_else(|| parse_time(&t))
    }),
    raw: Some(item.clone()),
    ..Notification::default()
  })
}

/// A tweet that mentions or replies to you.
fn mention(entry_id: &str, item: &Value) -> Option<Notification> {
  let post = parse::post(item.at("tweet_results.result"))?;
  let kind = if post.extra.contains_key("in_reply_to") {
    "reply"
  } else {
    "mention"
  };
  Some(Notification {
    id: entry_id.to_owned(),
    kind: kind.into(),
    text: post.text.clone().unwrap_or_default(),
    actor: post.author.clone(),
    url: post.url.clone(),
    created_at: post.created_at,
    raw: post.raw,
    ..Notification::default()
  })
}

fn kind(icon: &str) -> &'static str {
  match icon {
    "heart_icon" => "like",
    "retweet_icon" => "repost",
    "person_icon" => "follow",
    "reply_icon" => "reply",
    "list_icon" => "list",
    "security_alert_icon" => "security",
    "bird_icon" | "milestone_icon" | "bell_icon" | "recommendation_icon" => "system",
    _ => "other",
  }
}
