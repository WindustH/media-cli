//! Comments of the web client. A thread lists `commentViewModel`s holding
//! keys only; text, author and counters arrive as entity payloads in
//! `frameworkUpdates.entityBatchUpdate.mutations`, keyed by those keys
//! (`commentEntityPayload`, `engagementToolbarStateEntityPayload`,
//! `engagementToolbarSurfaceEntityPayload`), as YouTube.js `CommentView` maps them.

use std::collections::HashMap;

use media_core::{Comment, User, Value, ValueExt};

use super::{ago, count, put, text};
use crate::refs::{channel_url, comment_url};

/// The entity payloads of a response, by entity key.
pub struct Entities<'a>(HashMap<String, &'a Value>);

impl<'a> Entities<'a> {
  pub fn of(v: &'a Value) -> Self {
    let map = v
      .list("frameworkUpdates.entityBatchUpdate.mutations")
      .iter()
      .filter_map(|m| {
        let payload = m.at("payload").as_object()?.values().next()?;
        Some((m.str("entityKey").or_else(|| payload.str("key"))?, payload))
      })
      .collect();
    Self(map)
  }

  pub fn get(&self, key: Option<String>) -> Option<&'a Value> {
    self.0.get(&key?).copied()
  }
}

/// A comment from its view model and the response's entities.
pub fn comment_view(cvm: &Value, e: &Entities, video: &str) -> Option<Comment> {
  let entity = e.get(cvm.str("commentKey"))?;
  let props = entity.at("properties");
  let id = props.str("commentId").or_else(|| cvm.str("commentId"))?;
  let a = entity.at("author");
  let tb = entity.at("toolbar");
  let author = a.str("channelId").map(|cid| {
    let name = a.str("displayName").unwrap_or_else(|| cid.clone());
    User {
      handle: name.starts_with('@').then(|| name.clone()),
      url: Some(channel_url(&cid)),
      avatar: a.str("avatarThumbnailUrl"),
      verified: a.bool("isVerified") == Some(true),
      name,
      id: cid,
      ..User::default()
    }
  });
  let text = text(props.at("content")).unwrap_or_default();
  let top = props.u64("replyLevel").unwrap_or(0) == 0;
  let published = props.str("publishedTime");
  let mut c = Comment {
    created_at: published.as_deref().and_then(ago),
    likes: tb
      .str("likeCountA11y")
      .and_then(|t| count(&t))
      .or_else(|| tb.str("likeCountNotliked").and_then(|t| count(&t)))
      .or(Some(0)),
    reply_count: top.then(|| tb.str("replyCount").and_then(|t| count(&t)).unwrap_or(0)),
    // Replies name whom they answer with a leading @handle.
    reply_to: (!top)
      .then(|| {
        text
          .split_whitespace()
          .next()
          .filter(|w| w.starts_with('@'))
          .map(str::to_owned)
      })
      .flatten(),
    raw: Some(entity.clone()),
    author,
    text,
    ..Comment::default()
  };
  let x = &mut c.extra;
  put(x, "url", comment_url(video, &id));
  put(x, "published", published.clone());
  if published.is_some_and(|p| p.contains("edited")) {
    put(x, "edited", true);
  }
  if cvm.str("pinnedText").is_some() {
    put(x, "pinned", true);
  }
  if a.bool("isCreator") == Some(true) {
    put(x, "by_creator", true);
  }
  if let Some(state) = e.get(cvm.str("toolbarStateKey")) {
    if state.str("heartState").as_deref() == Some("TOOLBAR_HEART_STATE_HEARTED") {
      put(x, "hearted", true);
    }
    match state.str("likeState").as_deref() {
      Some("TOOLBAR_LIKE_STATE_LIKED") => put(x, "liked", true),
      Some("TOOLBAR_LIKE_STATE_DISLIKED") => put(x, "disliked", true),
      _ => {}
    }
  }
  c.id = id;
  Some(c)
}

/// Exact number of comments from a comments page header (`1,598 Comments`).
pub fn header_count(v: &Value) -> Option<u64> {
  super::first(v, "commentsHeaderRenderer")
    .and_then(|h| text(h.at("countText")))
    .and_then(|t| count(&t))
}
