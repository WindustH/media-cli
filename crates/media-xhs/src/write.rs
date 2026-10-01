//! Interactions: like, collect, comment, follow (`InteractionEndpointsMixin`,
//! `SocialEndpointsMixin`). Each pauses briefly first, as bursts get flagged.

use std::path::Path;
use std::time::Duration;

use media_core::file::Image;
use media_core::{Action, Error, Reply, Result, Value, ValueExt, json};

use crate::api::Client;
use crate::refs::{self, NoteRef, user_url};

const COMMENT: &str = "/api/sns/web/v1/comment/post";
const DELETE_COMMENT: &str = "/api/sns/web/v1/comment/delete";
/// The web uploader's permit (`useUploader` in the PC bundle).
const UPLOAD_PERMIT: &str = "/api/sns/web/upload/permit";

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

/// Comment on a note, or reply to one of its comments, with images if given.
///
/// No web client attaches images (the app does); they go up with the web
/// uploader's `comment` scene, which stores them under `comment/` like the
/// app's, and are sent as `pictures`, the field comments come back with.
/// When the comment comes back without them it is removed again, so a
/// failed attempt leaves nothing behind.
pub async fn comment(c: &Client, arg: &str, reply: &Reply) -> Result<Action> {
  let r = prepare_note(c, arg).await?;
  let mut pictures = Vec::new();
  for path in &reply.images {
    pictures.push(comment_image(c, path).await?);
  }
  let mut body = json!({"note_id": r.id, "content": reply.text, "at_users": []});
  if let Some(target) = &reply.reply_to {
    body["target_comment_id"] = json!(target);
  }
  if !pictures.is_empty() {
    body["pictures"] = json!(pictures);
  }
  let data = c.post(COMMENT, &body).await?;
  let id = data.str("comment.id");
  if !pictures.is_empty() && data.list("comment.pictures").is_empty() {
    let id = id.unwrap_or_default();
    let removed = c
      .post(DELETE_COMMENT, &json!({"note_id": r.id, "comment_id": id}))
      .await;
    let what = match removed {
      Ok(_) => "it was removed again".to_owned(),
      Err(e) => format!("removing comment {id} failed: {}", e.message),
    };
    return Err(
      Error::upstream(format!(
        "Xiaohongshu took the comment but dropped its images; {what}"
      ))
      .with_hint("comment without --image, or attach the image in the app"),
    );
  }
  let name = if reply.reply_to.is_some() {
    "reply"
  } else {
    "comment"
  };
  let mut action = Action::done(name, &r.id).with_url(r.url());
  if let Some(id) = id {
    action = action.with_id(id);
  }
  Ok(action)
}

/// Upload one comment image; its file id and size, as comment pictures carry them.
async fn comment_image(c: &Client, path: &Path) -> Result<Value> {
  let params = [
    ("version", "1"),
    ("biz_name", "sns"),
    ("scene", "comment"),
    ("file_count", "1"),
  ];
  let permits = c.get(UPLOAD_PERMIT, &params).await?;
  let image = Image::read(path).await?;
  let (width, height) = image.size();
  let file_id = c.upload(&permits, image.data, image.mime).await?;
  Ok(json!({"file_id": file_id, "width": width, "height": height}))
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
