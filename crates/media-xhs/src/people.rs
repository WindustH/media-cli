//! Accounts: the logged-in user, profiles and their note lists.

use media_core::{Error, Page, PageReq, Post, Result, User, Value, ValueExt};

use crate::api::Client;
use crate::parse;
use crate::refs::{self, SOURCE_FEED};

pub const ME: &str = "/api/sns/web/v2/user/me";

/// The logged-in account; a guest session counts as not logged in.
pub async fn whoami(c: &Client) -> Result<User> {
  c.require_login()?;
  let me = c.get(ME, &[]).await?;
  if me.bool("guest") == Some(true) {
    return Err(Error::auth("not logged in (guest session)").with_hint("run `media xhs login`"));
  }
  Ok(parse::profile(&me, None))
}

pub async fn user(c: &Client, arg: &str) -> Result<User> {
  c.require_login()?;
  let id = refs::user_ref(c, arg).await?;
  let data = c
    .get("/api/sns/web/v1/user/otherinfo", &[("target_user_id", &id)])
    .await?;
  Ok(parse::profile(&data, Some(&id)))
}

pub async fn user_posts(c: &Client, arg: &str, page: &PageReq) -> Result<Page<Post>> {
  c.require_login()?;
  let id = refs::user_ref(c, arg).await?;
  let num = page.size_within(30).to_string();
  let cursor = page.cursor.clone().unwrap_or_default();
  let params = [
    ("num", num.as_str()),
    ("cursor", cursor.as_str()),
    ("user_id", id.as_str()),
    ("image_scenes", "FD_WM_WEBP"),
  ];
  Ok(note_page(
    c,
    &c.get("/api/sns/web/v1/user_posted", &params).await?,
  ))
}

/// Notes a user collected (default: the logged-in account).
pub async fn favorites(c: &Client, user: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
  saved(c, "/api/sns/web/v2/note/collect/page", user, page).await
}

/// Notes a user liked (default: the logged-in account).
pub async fn likes(c: &Client, user: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
  saved(c, "/api/sns/web/v1/note/like/page", user, page).await
}

async fn saved(c: &Client, path: &str, user: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
  c.require_login()?;
  let id = match user {
    Some(arg) => refs::user_ref(c, arg).await?,
    None => whoami(c).await?.id,
  };
  let num = page.size_within(30).to_string();
  let cursor = page.cursor.clone().unwrap_or_default();
  let params = [
    ("user_id", id.as_str()),
    ("cursor", cursor.as_str()),
    ("num", num.as_str()),
  ];
  Ok(note_page(c, &c.get(path, &params).await?))
}

/// `{notes, cursor, has_more}` pages, remembering the note tokens.
fn note_page(c: &Client, data: &Value) -> Page<Post> {
  let notes = data.list("notes");
  let tokens: Vec<(String, String)> = notes
    .iter()
    .filter_map(|n| Some((n.str("note_id")?, n.str("xsec_token")?)))
    .collect();
  refs::remember(
    &c.ctx,
    tokens
      .iter()
      .map(|(id, t)| (id.as_str(), t.as_str(), SOURCE_FEED)),
  );
  let items = notes
    .iter()
    .filter_map(|n| parse::note(n, SOURCE_FEED))
    .collect();
  let next = data
    .str("cursor")
    .filter(|_| data.bool("has_more") == Some(true));
  Page::new(items, next)
}
