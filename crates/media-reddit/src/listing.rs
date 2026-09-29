//! Reddit listings (`{kind: Listing, data: {children, after}}`): one page per
//! call, `limit` up to 100, the `after` fullname as cursor.

use media_core::{Page, PageReq, Post, Result, Value, ValueExt};

use crate::api::{Api, Params};
use crate::parse;

const MAX: usize = 100;

pub async fn page<T>(
  api: &Api,
  path: &str,
  mut query: Params<'_>,
  req: &PageReq,
  map: impl Fn(&Value) -> Option<T>,
) -> Result<Page<T>> {
  query.push(("limit", req.size_within(MAX).to_string()));
  if let Some(after) = &req.cursor {
    query.push(("after", after.clone()));
  }
  let v = api.get(path, &query).await?;
  let items = v.list("data.children").iter().filter_map(map).collect();
  Ok(Page::new(items, v.str("data.after")))
}

/// A listing of `t3` posts.
pub async fn posts(api: &Api, path: &str, query: Params<'_>, req: &PageReq) -> Result<Page<Post>> {
  page(api, path, query, req, |t| {
    parse::data(t, "t3").and_then(parse::post)
  })
  .await
}
