//! Interactions: like, collect, comment, follow (`InteractionEndpointsMixin`,
//! `SocialEndpointsMixin`). Each pauses briefly first, as bursts get flagged.

use std::time::Duration;

use media_core::{Action, Error, Result, ValueExt, json};

use crate::api::Client;
use crate::refs::{self, NoteRef, user_url};

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

pub async fn comment(c: &Client, arg: &str, text: &str, reply_to: Option<&str>) -> Result<Action> {
  let r = prepare_note(c, arg).await?;
  let body = match reply_to {
    None => json!({"note_id": r.id, "content": text, "at_users": []}),
    Some(target) => json!({
      "note_id": r.id,
      "content": text,
      "target_comment_id": target,
      "at_users": [],
    }),
  };
  let data = c.post("/api/sns/web/v1/comment/post", &body).await?;
  let mut action = Action::done(
    if reply_to.is_some() {
      "reply"
    } else {
      "comment"
    },
    &r.id,
  )
  .with_url(r.url());
  if let Some(id) = data.str("comment.id") {
    action = action.with_id(id);
  }
  Ok(action)
}

pub async fn delete_comment(c: &Client, arg: &str, comment: &str) -> Result<Action> {
  let r = prepare_note(c, arg).await?;
  let body = json!({"note_id": r.id, "comment_id": comment});
  c.post("/api/sns/web/v1/comment/delete", &body).await?;
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
