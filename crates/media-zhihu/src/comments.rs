//! Comments (comment_v5): root comments of a post and replies under one comment.

use media_core::{Comment, Ctx, Page, PageReq, Result, Value, ValueExt};

use crate::api::{self, V4};
use crate::parse;
use crate::refs::Target;

/// `score` (hot first, the default) or `ts` (newest first).
pub async fn roots(
  ctx: &Ctx,
  target: &Target,
  sort: Option<&str>,
  page: &PageReq,
) -> Result<Page<Comment>> {
  let url = format!(
    "{V4}/comment_v5/{}/{}/root_comment",
    target.plural(),
    target.id()
  );
  let req = api::get(ctx, &url).query("order_by", sort.unwrap_or("score"));
  list(ctx, req, page).await
}

pub async fn children(ctx: &Ctx, comment: &str, page: &PageReq) -> Result<Page<Comment>> {
  let url = format!("{V4}/comment_v5/comment/{comment}/child_comment");
  list(ctx, api::get(ctx, &url).query("order_by", "ts"), page).await
}

/// The cursor is the upstream `offset` token of `paging.next`.
async fn list(ctx: &Ctx, req: media_core::http::Req<'_>, page: &PageReq) -> Result<Page<Comment>> {
  let req = req
    .query("limit", page.size_within(20))
    .query("offset", page.cursor.as_deref().unwrap_or(""));
  let v: Value = api::call(ctx, req).await?;
  Ok(Page::new(
    v.list("data").iter().map(parse::comment).collect(),
    api::next_param(&v, "offset"),
  ))
}
