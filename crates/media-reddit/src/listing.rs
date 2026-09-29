//! Reddit listings (`{kind: Listing, data: {children, after}}`): one page per
//! call, `limit` up to 100, the `after` fullname as cursor.

use media_core::{Page, PageReq, Post, Result, Value, ValueExt};

use crate::api::{Api, Params};
use crate::parse;

const MAX: usize = 100;

pub async fn page<T>(
  api: &Api,
  path: &str,
  query: Params<'_>,
  req: &PageReq,
  map: impl Fn(&Value) -> Option<T>,
) -> Result<Page<T>> {
  let v = api.get(path, &paged(query, req)).await?;
  Ok(of(&v, map))
}

/// `query` plus the page size and cursor of `req`.
pub fn paged<'a>(mut query: Params<'a>, req: &PageReq) -> Params<'a> {
  query.push(("limit", req.size_within(MAX).to_string()));
  if let Some(after) = &req.cursor {
    query.push(("after", after.clone()));
  }
  query
}

/// The items of a listing, continued at its `after`.
pub fn of<T>(listing: &Value, map: impl Fn(&Value) -> Option<T>) -> Page<T> {
  let items = listing
    .list("data.children")
    .iter()
    .filter_map(map)
    .collect();
  Page::new(items, listing.str("data.after"))
}

/// A `t3` thing as a post.
pub fn post(thing: &Value) -> Option<Post> {
  parse::data(thing, "t3").and_then(parse::post)
}

/// A listing of `t3` posts.
pub async fn posts(api: &Api, path: &str, query: Params<'_>, req: &PageReq) -> Result<Page<Post>> {
  page(api, path, query, req, post).await
}
