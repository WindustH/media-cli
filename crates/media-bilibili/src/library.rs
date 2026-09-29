//! Favorites folders and their videos, adding / removing videos, and the watch history.

use media_core::{
  Action, Collection, Ctx, Error, Page, PageReq, Post, Result, Value, ValueExt, json,
};

use crate::page::Pn;
use crate::refs::{self, Video};
use crate::{account, api, parse, user};

const FOLDERS: &str = "https://api.bilibili.com/x/v3/fav/folder/created/list-all";
const ITEMS: &str = "https://api.bilibili.com/x/v3/fav/resource/list";
const DEAL: &str = "https://api.bilibili.com/x/v3/fav/resource/deal";
const HISTORY: &str = "https://api.bilibili.com/x/web-interface/history/cursor";

/// `user` as a mid, or the logged-in account's mid when omitted.
async fn owner(ctx: &Ctx, user: Option<&str>) -> Result<String> {
  match user {
    Some(u) => user::mid(ctx, u).await,
    None => account::my_mid(ctx).await,
  }
}

async fn folder_list(ctx: &Ctx, mid: &str) -> Result<Vec<Value>> {
  let data = api::get(ctx, FOLDERS)
    .arg("up_mid", mid)
    .arg("type", 2)
    .arg("web_location", "333.1387")
    .send()
    .await?;
  Ok(data.list("list").to_vec())
}

/// Favorites folders of a user (all in one page).
pub async fn collections(ctx: &Ctx, user: Option<&str>) -> Result<Page<Collection>> {
  let mid = owner(ctx, user).await?;
  let folders = folder_list(ctx, &mid).await?;
  Ok(Page::last(
    folders.iter().map(|f| parse::folder(f, &mid)).collect(),
  ))
}

/// The default folder (attr bit 1 clear), else the first one.
async fn default_folder(ctx: &Ctx, mid: &str) -> Result<u64> {
  let folders = folder_list(ctx, mid).await?;
  folders
    .iter()
    .find(|f| f.u64("attr").is_some_and(|a| a & 2 == 0))
    .or(folders.first())
    .and_then(|f| f.u64("id"))
    .ok_or_else(|| Error::not_found("this account has no favorites folder"))
}

/// Videos saved in a folder (the user's default folder when none is given).
pub async fn favorites(
  ctx: &Ctx,
  user: Option<&str>,
  folder: Option<&str>,
  page: &PageReq,
) -> Result<Page<Post>> {
  let media_id = match folder {
    Some(f) => refs::folder(f)?,
    None => default_folder(ctx, &owner(ctx, user).await?).await?,
  };
  let pn = Pn::of(page, 20);
  let data = api::get(ctx, ITEMS)
    .arg("media_id", media_id)
    .arg("pn", pn.pn)
    .arg("ps", pn.ps)
    .arg("order", "mtime")
    .arg("type", 0)
    .arg("tid", 0)
    .arg("platform", "web")
    .arg("web_location", "333.1387")
    .send()
    .await?;
  let posts = data
    .list("medias")
    .iter()
    .filter(|m| m.u64("type") == Some(2))
    .map(|m| {
      let mut p = parse::video(m);
      if let Some(at) = m.i64("fav_time") {
        p.extra.insert("favorited_at".into(), json!(at));
      }
      p
    })
    .collect();
  Ok(pn.page(posts, data.bool("has_more") == Some(true)))
}

/// Add a video to a folder (default folder when none is given) or remove it.
pub async fn favorite(ctx: &Ctx, v: &Video, folder: Option<&str>, undo: bool) -> Result<Action> {
  let media_id = match folder {
    Some(f) => refs::folder(f)?,
    None => default_folder(ctx, &account::my_mid(ctx).await?).await?,
  };
  let id = media_id.to_string();
  let (add, del) = if undo {
    (String::new(), id)
  } else {
    (id, String::new())
  };
  api::post(ctx, DEAL)
    .arg("rid", v.aid)
    .arg("type", 2)
    .arg("add_media_ids", add)
    .arg("del_media_ids", del)
    .send()
    .await?;
  let action = if undo { "unfavorite" } else { "favorite" };
  Ok(
    Action::done(action, &v.bvid)
      .with_url(v.url())
      .with_message(format!("folder {media_id}")),
  )
}

/// Watched videos, newest first; the cursor is `max:view_at`.
pub async fn history(ctx: &Ctx, page: &PageReq) -> Result<Page<Post>> {
  ctx.require_login(api::LOGIN_COOKIES)?;
  let (max, view_at) = page
    .cursor
    .as_deref()
    .and_then(|c| c.split_once(':'))
    .unwrap_or(("0", "0"));
  let data = api::get(ctx, HISTORY)
    .arg("type", "archive")
    .arg("ps", page.size_within(30))
    .arg("max", max)
    .arg("view_at", view_at)
    .arg("business", "archive")
    .send()
    .await?;
  let posts: Vec<Post> = data
    .list("list")
    .iter()
    .map(|item| {
      let mut p = parse::video(item);
      if let Some(at) = item.i64("view_at") {
        p.extra.insert("viewed_at".into(), json!(at));
      }
      if let Some(progress) = item.i64("progress") {
        p.extra.insert("progress".into(), json!(progress));
      }
      p
    })
    .collect();
  let next = match (data.u64("cursor.max"), data.u64("cursor.view_at")) {
    (Some(m), Some(at)) if m > 0 && !posts.is_empty() => Some(format!("{m}:{at}")),
    _ => None,
  };
  Ok(Page::new(posts, next))
}
