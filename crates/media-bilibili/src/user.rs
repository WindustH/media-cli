//! Users: profiles, name lookup, followers / following and (un)follow.

use media_core::{Action, Ctx, Error, Page, PageReq, Query, Result, User, ValueExt};

use crate::api;
use crate::page::Pn;
use crate::refs::{self, UserRef};
use crate::{parse, video};

const CARD: &str = "https://api.bilibili.com/x/web-interface/card";
const FOLLOWERS: &str = "https://api.bilibili.com/x/relation/followers";
const FOLLOWINGS: &str = "https://api.bilibili.com/x/relation/followings";
const MODIFY: &str = "https://api.bilibili.com/x/relation/modify";

/// A user argument as a mid; names are looked up with user search (best match).
pub async fn mid(ctx: &Ctx, input: &str) -> Result<String> {
  let name = match refs::user(input) {
    UserRef::Mid(mid) => return Ok(mid.to_string()),
    UserRef::Name(name) => name,
  };
  let query = Query {
    keyword: name.clone(),
    ..Query::default()
  };
  let first = PageReq {
    cursor: None,
    size: 20,
  };
  let found = video::search_users(ctx, &query, &first).await?;
  let best = found
    .items
    .iter()
    .find(|u| u.name.eq_ignore_ascii_case(&name))
    .or(found.items.first());
  best
    .map(|u| u.id.clone())
    .ok_or_else(|| Error::not_found(format!("no user named {name}")))
}

pub async fn profile(ctx: &Ctx, input: &str) -> Result<User> {
  let mid = mid(ctx, input).await?;
  let data = api::get(ctx, CARD)
    .arg("mid", &mid)
    .arg("photo", "false")
    .send()
    .await?;
  let mut user = parse::user(data.at("card"));
  user.stats.followers = data.count("follower").or(user.stats.followers);
  user.stats.posts = data.count("archive_count");
  user.stats.likes = data.count("like_num");
  if ctx.http.has_cookie("SESSDATA") {
    user.followed = data.bool("following");
  }
  user.raw = Some(data);
  Ok(user)
}

/// Relation lists; Bilibili answers -101 / -352 to anonymous visitors.
async fn relations(ctx: &Ctx, url: &str, input: &str, page: &PageReq) -> Result<Page<User>> {
  ctx.require_login(api::LOGIN_COOKIES)?;
  let mid = mid(ctx, input).await?;
  let pn = Pn::of(page, 50);
  let data = api::get(ctx, url)
    .arg("vmid", mid)
    .arg("pn", pn.pn)
    .arg("ps", pn.ps)
    .arg("order", "desc")
    .send()
    .await?;
  let users = data.list("list").iter().map(parse::user).collect();
  Ok(pn.page(users, pn.before(data.u64("total"))))
}

/// Followers (others' lists are capped by Bilibili at 5 pages).
pub async fn followers(ctx: &Ctx, input: &str, page: &PageReq) -> Result<Page<User>> {
  relations(ctx, FOLLOWERS, input, page).await
}

pub async fn following(ctx: &Ctx, input: &str, page: &PageReq) -> Result<Page<User>> {
  relations(ctx, FOLLOWINGS, input, page).await
}

pub async fn follow(ctx: &Ctx, input: &str, undo: bool) -> Result<Action> {
  let mid = mid(ctx, input).await?;
  api::post(ctx, MODIFY)
    .arg("fid", &mid)
    .arg("act", if undo { 2 } else { 1 })
    .arg("re_src", 11)
    .send()
    .await?;
  let action = if undo { "unfollow" } else { "follow" };
  Ok(Action::done(action, &mid).with_url(refs::space_url(&mid)))
}
