//! Accounts: the logged-in user, profiles, people search and a user's comments.

use media_core::{Comment, Error, Page, PageReq, Query, Result, User};

use crate::api::Api;
use crate::{listing, parse, refs};

pub async fn whoami(api: &Api) -> Result<User> {
  let me = api.me().await?;
  parse::user(&me).ok_or_else(|| Error::auth("Reddit did not return the account"))
}

pub async fn user(api: &Api, arg: &str) -> Result<User> {
  let name = refs::user(arg)?;
  let v = api.get(&format!("/user/{name}/about"), &[]).await?;
  let mut user = parse::data(&v, "t2")
    .and_then(parse::user)
    .ok_or_else(|| Error::not_found(format!("user u/{name} not found")))?;
  if !api.logged_in() {
    user.followed = None;
  }
  Ok(user)
}

pub async fn search(api: &Api, q: &Query, req: &PageReq) -> Result<Page<User>> {
  let query = vec![("q", q.keyword.clone())];
  listing::page(api, "/users/search", query, req, |t| {
    parse::data(t, "t2").and_then(parse::user)
  })
  .await
}

/// Comments a user wrote, newest first.
pub async fn comments(api: &Api, arg: &str, req: &PageReq) -> Result<Page<Comment>> {
  let name = refs::user(arg)?;
  let query = vec![("sort", "new".into())];
  listing::page(api, &format!("/user/{name}/comments"), query, req, |t| {
    parse::comment(t, None)
  })
  .await
}
