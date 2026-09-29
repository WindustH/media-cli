//! Comments: threads of a video, replies of a thread, and writing.
//!
//! Tokens are built the way YouTube.js `getComments` builds them
//! (`GetCommentsSectionParams`): the comments section of a video in `top` or
//! `new` order, optionally with one "linked" comment first. A reply thread
//! token carries the thread id and the video's channel id, as the web app's
//! own reply tokens do (without the channel YouTube answers 400).
//!
//! Replying and deleting use the commands YouTube attaches to a comment for
//! the logged-in account (its reply dialog and its "Delete" menu item), read
//! from the comment's toolbar surface entity, as YouTube.js `CommentView` does.

use std::time::Duration;

use media_core::{Action, Comment, Error, ErrorCode, Page, PageReq, Result, Value, ValueExt, json};

use crate::api::Api;
use crate::parse::{self, Entities};
use crate::proto::Msg;
use crate::refs;
use crate::video;

pub const SORTS: &[&str] = &["top", "new"];

/// Videos do not change owner.
const OWNER_TTL: Duration = Duration::from_secs(365 * 86400);

fn section_token(video: &str, sort: u64, linked: Option<&str>) -> String {
  let mut opts = Msg::new().str(4, video).int(6, sort).int(15, 2);
  if let Some(c) = linked {
    opts = opts.str(16, c);
  }
  Msg::new()
    .msg(2, Msg::new().str(2, video))
    .int(3, 6)
    .msg(6, Msg::new().msg(4, opts).str(8, "comments-section"))
    .encode()
}

fn replies_token(video: &str, thread: &str, channel: &str) -> String {
  let opts = Msg::new()
    .str(2, thread)
    .msg(4, Msg::new().int(1, 0))
    .str(5, channel)
    .str(6, video)
    .int(8, 1)
    .int(9, 10);
  let target = format!("comment-replies-item-{thread}");
  Msg::new()
    .msg(2, Msg::new().str(2, video))
    .int(3, 6)
    .msg(6, Msg::new().msg(3, opts).str(8, &target))
    .encode()
}

async fn next(api: &Api, token: &str) -> Result<Value> {
  api.call("next", json!({ "continuation": token })).await
}

/// Items of a comments response (first page, more threads or replies).
fn items(v: &Value) -> impl Iterator<Item = &Value> {
  v.list("onResponseReceivedEndpoints").iter().flat_map(|e| {
    let reload = e.list("reloadContinuationItemsCommand.continuationItems");
    let append = e.list("appendContinuationItemsAction.continuationItems");
    reload.iter().chain(append)
  })
}

/// Comments of one page and the token of the next.
fn page(v: &Value, video: &str) -> Result<Page<Comment>> {
  let e = Entities::of(v);
  let mut out = Vec::new();
  let mut next = None;
  let mut message = None;
  for item in items(v) {
    let cvm = item
      .get("commentThreadRenderer")
      .map(|t| t.at("commentViewModel"))
      .or_else(|| item.get("commentViewModel"));
    if let Some(cvm) = cvm {
      let cvm = cvm.get("commentViewModel").unwrap_or(cvm);
      out.extend(parse::comment_view(cvm, &e, video));
    } else if let Some(cir) = item.get("continuationItemRenderer") {
      next = parse::token(cir);
    } else if message.is_none() {
      message = parse::first(item, "messageRenderer").and_then(|m| parse::text(m.at("text")));
    }
  }
  match message {
    // "Restricted Mode has hidden comments for this video."
    Some(m) if out.is_empty() && !m.contains("turned off") => {
      Err(Error::new(ErrorCode::PermissionDenied, m))
    }
    _ => Ok(Page::new(out, next)),
  }
}

pub async fn list(
  api: &Api,
  post: &str,
  sort: Option<&str>,
  req: &PageReq,
) -> Result<Page<Comment>> {
  let id = refs::video(post)?;
  let token = match &req.cursor {
    Some(c) => c.clone(),
    None => section_token(&id, u64::from(sort == Some("new")), None),
  };
  page(&next(api, &token).await?, &id)
}

/// Exact number of comments (from the header of the first page).
pub async fn count(api: &Api, video: &str) -> Result<Option<u64>> {
  let v = next(api, &section_token(video, 0, None)).await?;
  Ok(parse::header_count(&v))
}

/// Channel of a video (needed in reply tokens), cached.
async fn owner(api: &Api, video: &str) -> Result<String> {
  let key = format!("owner-{video}");
  if let Some(id) = api.ctx.store.cache_get::<String>(&key, OWNER_TTL) {
    return Ok(id);
  }
  let player = video::player(api, video).await?;
  let id = player
    .str("videoDetails.channelId")
    .ok_or_else(|| Error::not_found(format!("video {video} not found")))?;
  api.ctx.store.cache_put(&key, &id);
  Ok(id)
}

/// Every reply of a thread, page by page.
pub async fn replies(api: &Api, post: &str, comment: &str, req: &PageReq) -> Result<Page<Comment>> {
  let id = refs::video(post)?;
  let token = match &req.cursor {
    Some(c) => c.clone(),
    None => {
      let thread = refs::thread_of(&refs::comment(comment)?).to_owned();
      replies_token(&id, &thread, &owner(api, &id).await?)
    }
  };
  page(&next(api, &token).await?, &id)
}

/// The toolbar surface of one comment as the logged-in account sees it:
/// its reply dialog and its menu.
async fn surface(api: &Api, video: &str, comment: &str) -> Result<Value> {
  api.require_login()?;
  let v = next(api, &section_token(video, 0, Some(comment))).await?;
  let e = Entities::of(&v);
  let cvm = parse::find(&v, "commentViewModel")
    .into_iter()
    .map(|c| c.get("commentViewModel").unwrap_or(c))
    .find(|c| c.str("commentId").as_deref() == Some(comment))
    .ok_or_else(|| Error::not_found(format!("comment {comment} not found under {video}")))?;
  e.get(cvm.str("toolbarSurfaceKey"))
    .cloned()
    .ok_or_else(|| Error::upstream("YouTube sent no actions for this comment"))
}

/// API path of an endpoint from its command metadata (`/youtubei/v1/<path>`).
fn api_path(cmd: &Value, default: &str) -> String {
  cmd
    .str("commandMetadata.webCommandMetadata.apiUrl")
    .and_then(|u| u.strip_prefix("/youtubei/v1/").map(str::to_owned))
    .unwrap_or_else(|| default.to_owned())
}

/// The command object holding `endpoint` (next to its `commandMetadata`) inside `v`.
fn command<'a>(v: &'a Value, endpoint: &str) -> Option<&'a Value> {
  match v {
    Value::Object(m) if m.contains_key(endpoint) => Some(v),
    Value::Object(m) => m.values().find_map(|c| command(c, endpoint)),
    Value::Array(a) => a.iter().find_map(|c| command(c, endpoint)),
    _ => None,
  }
}

/// New comment id in a create response.
fn created_id(v: &Value) -> Option<String> {
  parse::find(v, "commentId")
    .into_iter()
    .find_map(|c| c.as_str().map(str::to_owned))
}

pub async fn add(api: &Api, post: &str, text: &str, reply_to: Option<&str>) -> Result<Action> {
  let id = refs::video(post)?;
  let (path, mut body, action) = match reply_to {
    None => {
      // YouTube.js `CreateCommentParams` as ts-proto writes it: {2: video id,
      // 5: {} (index 0 is the default and left out), 10: 7}.
      let params = Msg::new()
        .str(2, &id)
        .msg(5, Msg::new())
        .int(10, 7)
        .encode();
      let body = json!({ "createCommentParams": params });
      ("comment/create_comment".to_owned(), body, "comment")
    }
    Some(target) => {
      let target = refs::thread_of(&refs::comment(target)?).to_owned();
      let s = surface(api, &id, &target).await?;
      let cmd = command(s.at("replyCommand"), "createCommentReplyEndpoint").ok_or_else(|| {
        Error::new(
          ErrorCode::PermissionDenied,
          "YouTube offers no reply to this comment",
        )
      })?;
      let body = cmd.at("createCommentReplyEndpoint").clone();
      (api_path(cmd, "comment/create_comment_reply"), body, "reply")
    }
  };
  body["commentText"] = text.into();
  let v = api.write(&path, body).await?;
  if v.str("actionResult.status").as_deref() == Some("STATUS_FAILED") {
    let msg = parse::first(&v, "feedbackText").and_then(parse::text);
    return Err(Error::upstream(
      msg.unwrap_or_else(|| "YouTube refused the comment".into()),
    ));
  }
  let mut a = Action::done(action, &id);
  if let Some(cid) = created_id(&v) {
    a = a.with_url(refs::comment_url(&id, &cid)).with_id(cid);
  }
  Ok(a)
}

pub async fn delete(api: &Api, post: &str, comment: &str) -> Result<Action> {
  let id = refs::video(post)?;
  let comment = refs::comment(comment)?;
  let s = surface(api, &id, &comment).await?;
  // The menu's "Delete" item confirms, then performs a comment action.
  let item = parse::find(s.at("menuCommand"), "menuServiceItemRenderer")
    .into_iter()
    .chain(parse::find(
      s.at("menuCommand"),
      "menuNavigationItemRenderer",
    ))
    .find(|i| i.str("icon.iconType").as_deref() == Some("DELETE"))
    .ok_or_else(|| {
      Error::new(
        ErrorCode::PermissionDenied,
        "this comment has no Delete action for your account (not yours, not on your video)",
      )
    })?;
  let cmd = command(item, "performCommentActionEndpoint")
    .ok_or_else(|| Error::upstream("YouTube sent no delete command for this comment"))?;
  let action = cmd
    .str("performCommentActionEndpoint.action")
    .ok_or_else(|| Error::upstream("YouTube sent an empty delete command"))?;
  let path = api_path(cmd, "comment/perform_comment_action");
  let v = api.write(&path, json!({ "actions": [action] })).await?;
  if v.str("actionResults.0.status").as_deref() == Some("STATUS_FAILED") {
    return Err(Error::upstream("YouTube refused to delete the comment"));
  }
  Ok(Action::done("delete_comment", comment))
}
