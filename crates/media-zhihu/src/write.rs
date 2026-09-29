//! Interactions: votes, favorites, comments, follows and deletions.

use std::time::Duration;

use media_core::{Action, Ctx, Error, Result, ValueExt, json};

use crate::account;
use crate::api::{self, V4, ZHUANLAN};
use crate::people;
use crate::refs::Target;

/// Writes need a login and the CSRF cookie; pause a little like a person would.
pub async fn prepare(ctx: &Ctx) -> Result<()> {
  api::need_login(ctx)?;
  account::helper_cookies(ctx).await?;
  ctx
    .http
    .pause(Duration::from_millis(400), Duration::from_millis(1200))
    .await;
  Ok(())
}

/// Upvote an answer, like an article or a pin; `undo` goes back to neutral.
pub async fn like(ctx: &Ctx, target: &Target, undo: bool) -> Result<Action> {
  if matches!(target, Target::Question(_)) {
    return Err(Error::input(
      "questions cannot be liked; use `follow-question` instead",
    ));
  }
  prepare(ctx).await?;
  let req = match target {
    Target::Article(id) => api::post(ctx, &format!("{V4}/articles/{id}/voters"))
      .json(&json!({ "voting": if undo { 0 } else { 1 } })),
    Target::Pin(id) if undo => api::delete(ctx, &format!("{V4}/pins/{id}/voters/up")),
    Target::Pin(id) => api::post(ctx, &format!("{V4}/pins/{id}/voters/up"))
      .json(&json!({ "not_sync_moments": true })),
    // Answers (questions were rejected above).
    _ => api::post(ctx, &format!("{V4}/answers/{}/voters", target.id()))
      .json(&json!({ "type": if undo { "neutral" } else { "up" } })),
  };
  api::call(ctx, req).await?;
  let action = if undo { "unlike" } else { "like" };
  Ok(Action::done(action, target.url()))
}

/// Add to (or remove from) a favorites folder; defaults to the account's first folder.
pub async fn favorite(
  ctx: &Ctx,
  target: &Target,
  folder: Option<&str>,
  undo: bool,
) -> Result<Action> {
  if matches!(target, Target::Question(_)) {
    return Err(Error::input(
      "questions cannot be saved; use `follow-question` instead",
    ));
  }
  prepare(ctx).await?;
  let folder = match folder {
    Some(f) => f.to_owned(),
    None => people::first_folder(ctx, None).await?,
  };
  let (id, kind) = (target.id(), target.kind());
  let req = if undo {
    api::delete(ctx, &format!("{V4}/collections/{folder}/contents/{id}"))
      .query("content_type", kind)
  } else {
    api::post(ctx, &format!("{V4}/collections/{folder}/contents"))
      .query("content_id", id)
      .query("content_type", kind)
  };
  api::call(ctx, req).await?;
  let action = if undo { "unfavorite" } else { "favorite" };
  Ok(Action::done(action, target.url()).with_message(format!("folder {folder}")))
}

pub async fn comment(
  ctx: &Ctx,
  target: &Target,
  text: &str,
  reply_to: Option<&str>,
) -> Result<Action> {
  prepare(ctx).await?;
  let mut body = json!({
    "content": text,
    "selected_settings": [],
    "unfriendly_check": "strict",
  });
  if let Some(parent) = reply_to {
    body["reply_comment_id"] = parent.into();
  }
  let url = format!(
    "{V4}/comment_v5/{}/{}/comment",
    target.plural(),
    target.id()
  );
  let v = api::call(ctx, api::post(ctx, &url).json(&body)).await?;
  let action = if reply_to.is_some() {
    "reply"
  } else {
    "comment"
  };
  let mut done = Action::done(action, target.url());
  if let Some(id) = v.str("id") {
    done = done.with_id(id);
  }
  Ok(done)
}

pub async fn delete_comment(ctx: &Ctx, target: &Target, comment: &str) -> Result<Action> {
  prepare(ctx).await?;
  api::call(ctx, api::delete(ctx, &format!("{V4}/comments/{comment}"))).await?;
  Ok(Action::done("delete_comment", target.url()).with_id(comment))
}

/// Follow a user (`members`) or a question (`questions`).
pub async fn follow(ctx: &Ctx, resource: &str, id: &str, undo: bool) -> Result<Action> {
  prepare(ctx).await?;
  let url = format!("{V4}/{resource}/{id}/followers");
  let req = if undo {
    api::delete(ctx, &url)
  } else {
    api::post(ctx, &url)
  };
  api::call(ctx, req).await?;
  Ok(Action::done(if undo { "unfollow" } else { "follow" }, id))
}

/// Delete one of the account's own questions, answers, articles or pins.
pub async fn delete(ctx: &Ctx, target: &Target) -> Result<Action> {
  prepare(ctx).await?;
  let url = match target {
    Target::Question(id) => format!("{V4}/questions/{id}"),
    Target::Answer(id) => format!("{V4}/answers/{id}"),
    Target::Article(id) => format!("{ZHUANLAN}/articles/{id}"),
    Target::Pin(id) => format!("{V4}/pins/{id}"),
  };
  api::call(ctx, api::delete(ctx, &url)).await?;
  Ok(Action::done("delete", target.url()).with_id(target.id()))
}
