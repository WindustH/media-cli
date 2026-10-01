//! Interactions: like, collect, comment, follow (`InteractionEndpointsMixin`,
//! `SocialEndpointsMixin`). Each pauses briefly first, as bursts get flagged.

use std::time::Duration;

use media_core::{Action, Error, Reply, Result, ValueExt, json};

use crate::api::Client;
use crate::refs::{self, NoteRef, user_url};

const COMMENT: &str = "/api/sns/web/v1/comment/post";
const DELETE_COMMENT: &str = "/api/sns/web/v1/comment/delete";

async fn prepare_note(c: &Client, arg: &str) -> Result<NoteRef> {
  c.require_login()?;
  let r = refs::note_ref(c, arg).await?;
  pause(c).await;
  Ok(r)
}

async fn pause(c: &Client) {
  c.ctx
    .http
    .pause(Duration::from_millis(800), Duration::from_millis(2000))
    .await;
}

pub async fn like(c: &Client, arg: &str, undo: bool) -> Result<Action> {
  let r = prepare_note(c, arg).await?;
  let (path, action) = match undo {
    false => ("/api/sns/web/v1/note/like", "like"),
    true => ("/api/sns/web/v1/note/dislike", "unlike"),
  };
  c.post(path, &json!({"note_oid": r.id})).await?;
  Ok(Action::done(action, &r.id).with_url(r.url()))
}

pub async fn favorite(c: &Client, arg: &str, folder: Option<&str>, undo: bool) -> Result<Action> {
  if folder.is_some() {
    return Err(Error::unsupported("favorite --folder"));
  }
  let r = prepare_note(c, arg).await?;
  let (path, body, action) = match undo {
    false => (
      "/api/sns/web/v1/note/collect",
      json!({"note_id": r.id}),
      "favorite",
    ),
    true => (
      "/api/sns/web/v1/note/uncollect",
      json!({"note_ids": r.id}),
      "unfavorite",
    ),
  };
  c.post(path, &body).await?;
  Ok(Action::done(action, &r.id).with_url(r.url()))
}

/// Comment on a note, or reply to one of its comments.
///
/// Text only: comment images are an app feature, and the web endpoint drops
/// any it is sent.
pub async fn comment(c: &Client, arg: &str, reply: &Reply) -> Result<Action> {
  let r = prepare_note(c, arg).await?;
  let mut body = json!({"note_id": r.id, "content": reply.text, "at_users": []});
  if let Some(target) = &reply.reply_to {
    body["target_comment_id"] = json!(target);
  }
  let data = c.post(COMMENT, &body).await?;
  let name = if reply.reply_to.is_some() {
    "reply"
  } else {
    "comment"
  };
  let mut action = Action::done(name, &r.id).with_url(r.url());
  if let Some(id) = data.str("comment.id") {
    action = action.with_id(id);
  }
  Ok(action)
}

pub async fn delete_comment(c: &Client, arg: &str, comment: &str) -> Result<Action> {
  let r = prepare_note(c, arg).await?;
  let body = json!({"note_id": r.id, "comment_id": comment});
  c.post(DELETE_COMMENT, &body).await?;
  Ok(Action::done("delete-comment", comment).with_url(r.url()))
}

pub async fn follow(c: &Client, arg: &str, undo: bool) -> Result<Action> {
  c.require_login()?;
  let id = refs::user_ref(c, arg).await?;
  pause(c).await;
  let (path, action) = match undo {
    false => ("/api/sns/web/v1/user/follow", "follow"),
    true => ("/api/sns/web/v1/user/unfollow", "unfollow"),
  };
  c.post(path, &json!({"target_user_id": id})).await?;
  Ok(Action::done(action, &id).with_url(user_url(&id)))
}
