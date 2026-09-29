//! Users: profiles, their posts, follow lists, favorites folders and notifications.

use media_core::{
  Collection, Ctx, Error, Notification, Page, PageReq, Post, Result, User, Value, ValueExt,
};

use crate::account;
use crate::api::{self, V4};
use crate::parse;

const MEMBER_INCLUDE: &str = "answer_count,articles_count,pins_count,question_count,follower_count,following_count,voteup_count,thanked_count,favorited_count,is_following,is_followed,gender,badge,description,business,educations,employments,locations";
const LIST_INCLUDE: &str =
  "data[*].answer_count,articles_count,follower_count,gender,is_followed,is_following,badge";

pub async fn user(ctx: &Ctx, token: &str) -> Result<User> {
  let v = api::call(
    ctx,
    api::get(ctx, &format!("{V4}/members/{token}")).query("include", MEMBER_INCLUDE),
  )
  .await?;
  Ok(parse::user(&v))
}

/// One page of an offset-paged v4 listing.
async fn list(ctx: &Ctx, path: &str, include: Option<&str>, page: &PageReq) -> Result<Value> {
  let mut req = api::get(ctx, &format!("{V4}/{path}")).queries([
    ("offset", page.number_or(0)),
    ("limit", page.size_within(20) as u64),
  ]);
  if let Some(include) = include {
    req = req.query("include", include);
  }
  api::call(ctx, req).await
}

fn posts(v: &Value, map: fn(&Value) -> Post) -> Page<Post> {
  Page::new(
    v.list("data").iter().map(map).collect(),
    api::next_param(v, "offset"),
  )
}

pub async fn answers(ctx: &Ctx, token: &str, page: &PageReq) -> Result<Page<Post>> {
  let include = "data[*].content,excerpt,voteup_count,comment_count,favlists_count,created_time,updated_time,question";
  let path = format!("members/{token}/answers?sort_by=created");
  Ok(posts(
    &list(ctx, &path, Some(include), page).await?,
    parse::answer,
  ))
}

pub async fn articles(ctx: &Ctx, token: &str, page: &PageReq) -> Result<Page<Post>> {
  let include =
    "data[*].content,excerpt,voteup_count,comment_count,created,updated,image_url,topics";
  let path = format!("members/{token}/articles?sort_by=created");
  Ok(posts(
    &list(ctx, &path, Some(include), page).await?,
    parse::article,
  ))
}

pub async fn pins(ctx: &Ctx, token: &str, page: &PageReq) -> Result<Page<Post>> {
  let path = format!("members/{token}/pins");
  Ok(posts(&list(ctx, &path, None, page).await?, parse::pin))
}

/// Followers (`followers`) or followees (`followees`) of a user.
pub async fn follows(ctx: &Ctx, token: &str, which: &str, page: &PageReq) -> Result<Page<User>> {
  let v = list(
    ctx,
    &format!("members/{token}/{which}"),
    Some(LIST_INCLUDE),
    page,
  )
  .await?;
  Ok(Page::new(
    v.list("data").iter().map(parse::user).collect(),
    api::next_param(&v, "offset"),
  ))
}

async fn owner(ctx: &Ctx, user: Option<&str>) -> Result<String> {
  match user {
    Some(u) => Ok(u.to_owned()),
    None => account::my_token(ctx).await,
  }
}

/// Favorites folders (收藏夹) of a user, or of the logged-in account.
pub async fn folders(ctx: &Ctx, user: Option<&str>, page: &PageReq) -> Result<Page<Collection>> {
  let token = owner(ctx, user).await?;
  let v = list(ctx, &format!("members/{token}/favlists"), None, page).await?;
  Ok(Page::new(
    v.list("data").iter().map(parse::folder).collect(),
    api::next_param(&v, "offset"),
  ))
}

/// The first folder of a user (the default one for the logged-in account).
pub async fn first_folder(ctx: &Ctx, user: Option<&str>) -> Result<String> {
  let first = folders(
    ctx,
    user,
    &PageReq {
      cursor: None,
      size: 1,
    },
  )
  .await?;
  first
    .items
    .into_iter()
    .next()
    .map(|c| c.id)
    .ok_or_else(|| Error::not_found("no favorites folder found; pass --folder <id>"))
}

/// Items saved in a folder.
pub async fn folder_items(ctx: &Ctx, folder: &str, page: &PageReq) -> Result<Page<Post>> {
  let v = list(ctx, &format!("collections/{folder}/items"), None, page).await?;
  let items = v
    .list("data")
    .iter()
    .filter_map(|item| parse::post(item.at("content")))
    .collect();
  Ok(Page::new(items, api::next_param(&v, "offset")))
}

pub async fn notifications(ctx: &Ctx, page: &PageReq) -> Result<Page<Notification>> {
  ctx.require_login(&["z_c0"])?;
  let v = list(ctx, "notifications/v2/recent?entry_name=all", None, page).await?;
  Ok(Page::new(
    v.list("data").iter().map(parse::notification).collect(),
    api::next_param(&v, "offset"),
  ))
}
