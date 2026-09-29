//! Writing: votes, saves, comments, edits, deletes and new posts (text,
//! link, crosspost; images in [`crate::upload`]). Every write goes through
//! [`Api::post`], which checks the login, pauses and adds the CSRF token.

use media_core::{Action, Draft, Error, Result, Value, ValueExt};

use crate::api::{Api, WWW};
use crate::{refs, upload};

/// Where a fullname can be opened (posts only; comments need their post).
fn thing_url(fullname: &str) -> Option<String> {
  fullname
    .strip_prefix("t3_")
    .map(|id| format!("{WWW}/comments/{id}/"))
}

fn done(name: &str, fullname: &str) -> Action {
  let action = Action::done(name, fullname);
  match thing_url(fullname) {
    Some(url) => action.with_url(url),
    None => action,
  }
}

/// `dir`: 1 up, -1 down, 0 clears the vote.
pub async fn vote(api: &Api, arg: &str, dir: i8, name: &str) -> Result<Action> {
  let id = refs::thing(&api.ctx, arg).await?;
  let form = vec![("id", id.clone()), ("dir", dir.to_string())];
  api.post("/api/vote", form).await?;
  Ok(done(name, &id))
}

pub async fn save(api: &Api, arg: &str, folder: Option<&str>, undo: bool) -> Result<Action> {
  if folder.is_some() {
    return Err(Error::input("Reddit has no folders for saved posts"));
  }
  let id = refs::thing(&api.ctx, arg).await?;
  let (path, name) = if undo {
    ("/api/unsave", "unsave")
  } else {
    ("/api/save", "save")
  };
  api.post(path, vec![("id", id.clone())]).await?;
  Ok(done(name, &id))
}

/// The comment in a `/api/comment` or `/api/editusertext` answer.
fn written(v: &Value, name: &str, target: &str) -> Action {
  let c = v.at("json.data.things.0.data");
  let mut action = Action::done(name, target);
  if let Some(id) = c.str("id") {
    action = action.with_id(id);
  }
  if let Some(link) = c.str("permalink") {
    action = action.with_url(format!("{WWW}{link}"));
  }
  action
}

/// Comment on a post, or reply to one of its comments.
pub async fn comment(api: &Api, post: &str, text: &str, reply_to: Option<&str>) -> Result<Action> {
  api.require_login()?;
  let parent = match reply_to {
    Some(c) => format!("t1_{}", refs::comment_id(c)?),
    None => format!("t3_{}", refs::post(&api.ctx, post).await?),
  };
  let form = vec![("thing_id", parent.clone()), ("text", text.to_owned())];
  let v = api.post("/api/comment", form).await?;
  Ok(written(&v, "comment", &parent))
}

/// Replace the text of your own post or comment.
pub async fn edit(api: &Api, arg: &str, text: &str) -> Result<Action> {
  let id = refs::thing(&api.ctx, arg).await?;
  let form = vec![("thing_id", id.clone()), ("text", text.to_owned())];
  let v = api.post("/api/editusertext", form).await?;
  Ok(written(&v, "edit", &id))
}

/// Delete your post (`t3_…`) or comment (`t1_…`).
pub async fn delete(api: &Api, fullname: String, name: &str) -> Result<Action> {
  api.post("/api/del", vec![("id", fullname.clone())]).await?;
  Ok(Action::done(name, fullname))
}

pub async fn delete_post(api: &Api, arg: &str) -> Result<Action> {
  let id = refs::post(&api.ctx, arg).await?;
  delete(api, format!("t3_{id}"), "delete").await
}

pub async fn delete_comment(api: &Api, arg: &str) -> Result<Action> {
  delete(
    api,
    format!("t1_{}", refs::comment_id(arg)?),
    "delete-comment",
  )
  .await
}

/// A text post, a link post (the text is a single URL), a crosspost
/// (`--quote`), or an image / gallery post (`--image`), in the subreddit of
/// the first `--topic`. `--reply-to` comments instead.
pub async fn publish(api: &Api, draft: &Draft) -> Result<Action> {
  if let Some(post) = &draft.reply_to {
    return comment(api, post, &draft.text, None).await;
  }
  api.require_login()?;
  let title = draft
    .title
    .as_deref()
    .map(str::trim)
    .filter(|t| !t.is_empty())
    .ok_or_else(|| Error::input("a Reddit post needs a --title"))?;
  let sr = match draft.topics.first() {
    Some(topic) => refs::subreddit(topic)?,
    None => return Err(Error::input("choose the subreddit with --topic r/<name>")),
  };
  let text = draft.text.trim();
  if (draft.quote.is_some() || !draft.images.is_empty()) && !text.is_empty() {
    return Err(Error::input(
      "crossposts and image posts take no text; comment on the post instead",
    ));
  }
  if !draft.images.is_empty() {
    return upload::submit(api, &sr, title, &draft.images).await;
  }
  let mut form = vec![
    ("sr", sr.clone()),
    ("title", title.to_owned()),
    ("resubmit", "true".into()),
    ("sendreplies", "true".into()),
  ];
  if let Some(quote) = &draft.quote {
    let id = refs::post(&api.ctx, quote).await?;
    form.push(("kind", "crosspost".into()));
    form.push(("crosspost_fullname", format!("t3_{id}")));
  } else if is_url(text) {
    form.push(("kind", "link".into()));
    form.push(("url", text.to_owned()));
  } else {
    form.push(("kind", "self".into()));
    form.push(("text", text.to_owned()));
  }
  let v = api.post("/api/submit", form).await?;
  Ok(submitted(&v, &sr))
}

fn is_url(text: &str) -> bool {
  (text.starts_with("https://") || text.starts_with("http://"))
    && !text.contains(char::is_whitespace)
}

/// The new post in a `/api/submit` (or gallery) answer.
pub fn submitted(v: &Value, sr: &str) -> Action {
  let d = v.at("json.data");
  let mut action = Action::done("publish", format!("r/{sr}"));
  if let Some(id) = d.str("id") {
    action = action.with_id(id.trim_start_matches("t3_"));
  }
  if let Some(url) = d.str("url") {
    action = action.with_url(url);
  }
  action
}
