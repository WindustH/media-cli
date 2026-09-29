//! One note: detail, comments and replies (`xhs_cli/client_mixins.py::ReadingEndpointsMixin`).

use media_core::{Comment, Error, Page, PageReq, Post, Result, Value, ValueExt, json};

use crate::api::Client;
use crate::refs::{self, NoteRef};
use crate::{page, parse};

const FEED: &str = "/api/sns/web/v1/feed";
const COMMENTS: &str = "/api/sns/web/v2/comment/page";
const SUB_COMMENTS: &str = "/api/sns/web/v2/comment/sub/page";

/// Note detail: the feed API when logged in and a token is known, else (or
/// when it fails) the server-rendered page, which also serves visitors.
pub async fn read(c: &Client, arg: &str) -> Result<Post> {
  let mut r = refs::note_ref(c, arg).await?;
  let mut api_error = None;
  let logged_in = c.ctx.http.has_cookie("web_session");
  if let Some(token) = r.token.clone().filter(|_| logged_in) {
    match detail(c, &r, &token).await {
      Ok(post) => return Ok(post),
      Err(e) => {
        tracing::debug!("feed API failed ({e}), falling back to the note page");
        if r.cached {
          refs::forget(&c.ctx, &r.id);
          r.token = None;
        }
        api_error = Some(e);
      }
    }
  }
  let from_page = async {
    let html = page::fetch(c, &r).await?;
    let note = page::note(&html, &r.id)?;
    let item = json!({"id": r.id, "xsec_token": r.token, "note_card": note});
    parse::note(&item, r.source()).ok_or_else(|| Error::not_found(format!("note {}", r.id)))
  };
  match from_page.await {
    Ok(post) => Ok(post),
    Err(e) => Err(api_error.unwrap_or(e)),
  }
}

async fn detail(c: &Client, r: &NoteRef, token: &str) -> Result<Post> {
  let body = json!({
    "source_note_id": r.id,
    "image_formats": ["jpg", "webp", "avif"],
    "extra": {"need_body_topic": "1"},
    "xsec_source": r.source(),
    "xsec_token": token,
  });
  let data = c.post(FEED, &body).await?;
  data
    .list("items")
    .first()
    .and_then(|item| parse::note(item, r.source()))
    .ok_or_else(|| Error::not_found(format!("note {} not found", r.id)))
}

/// A token for `r`: given, cached, or scraped from the note page.
async fn token_for(c: &Client, r: &NoteRef) -> Result<String> {
  if let Some(t) = &r.token {
    return Ok(t.clone());
  }
  let html = page::fetch(c, r).await?;
  let (token, source) = page::token(&html, &r.id).ok_or_else(|| {
    Error::input(format!("no xsec_token for note {}", r.id))
      .with_hint("pass the full note URL from a listing (search, feed, user-posts ...)")
  })?;
  let source = source.unwrap_or_else(|| r.source().to_owned());
  refs::remember(&c.ctx, [(r.id.as_str(), token.as_str(), source.as_str())]);
  Ok(token)
}

pub async fn comments(c: &Client, arg: &str, page: &PageReq) -> Result<Page<Comment>> {
  c.require_login()?;
  let mut r = refs::note_ref(c, arg).await?;
  let cursor = page.cursor.clone().unwrap_or_default();
  let token = token_for(c, &r).await?;
  let data = match comment_page(c, &r.id, &cursor, &token).await {
    // A cached token may have expired: scrape a fresh one once.
    Err(_) if r.cached => {
      refs::forget(&c.ctx, &r.id);
      r.token = None;
      let token = token_for(c, &r).await?;
      comment_page(c, &r.id, &cursor, &token).await?
    }
    other => other?,
  };
  Ok(comment_list(&data))
}

async fn comment_page(c: &Client, id: &str, cursor: &str, token: &str) -> Result<Value> {
  let params = [
    ("note_id", id),
    ("cursor", cursor),
    ("top_comment_id", ""),
    ("image_formats", "jpg,webp,avif"),
    ("xsec_token", token),
  ];
  c.get(COMMENTS, &params).await
}

/// One page of the replies under a root comment, with the web app's
/// parameters; the first page (empty cursor) starts at the oldest reply.
pub async fn replies(
  c: &Client,
  arg: &str,
  comment: &str,
  page: &PageReq,
) -> Result<Page<Comment>> {
  c.require_login()?;
  let r = refs::note_ref(c, arg).await?;
  let token = token_for(c, &r).await?;
  let num = page.size_within(30).to_string();
  let cursor = page.cursor.clone().unwrap_or_default();
  let params = [
    ("note_id", r.id.as_str()),
    ("root_comment_id", comment),
    ("num", num.as_str()),
    ("cursor", cursor.as_str()),
    ("image_formats", "jpg,webp,avif"),
    ("top_comment_id", ""),
    ("xsec_token", token.as_str()),
  ];
  Ok(comment_list(&c.get(SUB_COMMENTS, &params).await?))
}

fn comment_list(data: &Value) -> Page<Comment> {
  let items = data.list("comments").iter().map(parse::comment).collect();
  let next = data
    .str("cursor")
    .filter(|_| data.bool("has_more") == Some(true));
  Page::new(items, next)
}
