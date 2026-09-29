//! Comment threads: the tree of a post (one page, nested replies up to a
//! depth) and the subtree under one comment.

use media_core::{Comment, Error, Page, PageReq, Result, ValueExt};

use crate::api::Api;
use crate::{parse, refs};

pub const SORTS: &[&str] = &["confidence", "top", "new", "controversial", "old", "qa"];
/// Levels of replies loaded under each top-level comment.
const DEPTH: usize = 4;

/// `limit` counts nested replies too, so ask for several per wanted thread;
/// the pager trims what is not needed.
fn query(sort: Option<&str>, req: &PageReq) -> Vec<(&'static str, String)> {
  let limit = req.size.saturating_mul(5).clamp(20, 500);
  let mut query = vec![("limit", limit.to_string()), ("depth", DEPTH.to_string())];
  if let Some(sort) = sort {
    query.push(("sort", sort.to_owned()));
  }
  query
}

pub async fn list(
  api: &Api,
  post: &str,
  sort: Option<&str>,
  req: &PageReq,
) -> Result<Page<Comment>> {
  let id = refs::post(&api.ctx, post).await?;
  let v = api
    .get(&format!("/comments/{id}"), &query(sort, req))
    .await?;
  Ok(Page::last(parse::comments(v.at("1"), None)))
}

/// Replies under one comment (`/comments/<post>/_/<comment>`).
pub async fn replies(api: &Api, post: &str, comment: &str, req: &PageReq) -> Result<Page<Comment>> {
  let id = refs::post(&api.ctx, post).await?;
  let cid = refs::comment_id(comment)?;
  let path = format!("/comments/{id}/_/{cid}");
  let v = api.get(&path, &query(None, req)).await?;
  let root = parse::comments(v.at("1"), None)
    .into_iter()
    .find(|c| c.id == cid)
    .ok_or_else(|| Error::not_found(format!("comment {cid} not found under post {id}")))?;
  Ok(Page::last(root.replies))
}
