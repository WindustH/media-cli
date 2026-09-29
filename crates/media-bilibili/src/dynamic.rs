//! Dynamics: the following feed, a user's dynamics, one dynamic, who liked
//! and reposted it, and the dynamic writes (publish with images, repost,
//! delete, like, favorite).

use std::path::Path;
use std::time::Duration;

use media_core::file::Image;
use media_core::http::Part;
use media_core::text::from_secs;
use media_core::{
  Action, Ctx, Draft, Error, Page, PageReq, Post, Result, User, Value, ValueExt, json,
};

use crate::refs::{DYNAMIC_URL, Video};
use crate::{account, api, parse, video};

const FEED: &str = "https://api.bilibili.com/x/polymer/web-dynamic/v1/feed/all";
const SPACE: &str = "https://api.bilibili.com/x/polymer/web-dynamic/v1/feed/space";
const DETAIL: &str = "https://api.bilibili.com/x/polymer/web-dynamic/desktop/v1/detail";
/// Likes and reposts mixed, newest first (the "赞与转发" tab of a dynamic's page).
const REACTION: &str = "https://api.bilibili.com/x/polymer/web-dynamic/v1/detail/reaction";
/// Reposts with their text (dyn-home bundle `index.*.js`, `bili-dyn-forward`).
const FORWARDS: &str = "https://api.bilibili.com/x/polymer/web-dynamic/v1/detail/forward";
const CREATE: &str = "https://api.bilibili.com/x/dynamic/feed/create/dyn";
const UPLOAD: &str = "https://api.bilibili.com/x/dynamic/feed/draw/upload_bfs";
const REPOST: &str = "https://api.vc.bilibili.com/dynamic_repost/v1/dynamic_repost/repost";
const REMOVE: &str = "https://api.vc.bilibili.com/dynamic_svr/v1/dynamic_svr/rm_dynamic";
const THUMB: &str = "https://api.vc.bilibili.com/dynamic_like/v1/dynamic_like/thumb";
const COLLECT: &str = "https://api.bilibili.com/x/community/cosmo/interface/simple_action";
/// Pages of a user's dynamics searched for a video's dynamic.
const SCAN_PAGES: usize = 10;
const VIDEO_DYNAMIC_TTL: Duration = Duration::from_secs(30 * 86_400);
const FEATURES: &str = "itemOpusStyle,opusBigCover,onlyfansVote,endFooterHidden,decorationCard,onlyfansAssetsV2,ugcDelete";

/// Items of a feed page: dynamics, or for `videos` the videos they announce.
fn items(data: &Value, videos: bool) -> Vec<Post> {
  let list = data.list("items");
  if videos {
    list.iter().filter_map(parse::dynamic_video).collect()
  } else {
    list.iter().map(parse::dynamic).collect()
  }
}

fn cursor_page<T>(data: &Value, items: Vec<T>) -> Page<T> {
  let next = data
    .str("offset")
    .filter(|_| data.bool("has_more") == Some(true));
  Page::new(items, next)
}

/// Dynamics of followed accounts; `kind` is `all` or `video`.
pub async fn feed(ctx: &Ctx, kind: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
  ctx.require_login(api::LOGIN_COOKIES)?;
  let kind = kind.unwrap_or("all");
  let data = api::get(ctx, FEED)
    .arg("timezone_offset", -480)
    .arg("type", kind)
    .arg("page", 1)
    .arg("offset", page.cursor.as_deref().unwrap_or_default())
    .arg("features", "itemOpusStyle")
    .send()
    .await?;
  Ok(cursor_page(&data, items(&data, kind == "video")))
}

/// Dynamics published by `mid`.
pub async fn of_user(ctx: &Ctx, mid: &str, page: &PageReq) -> Result<Page<Post>> {
  let data = api::get(ctx, SPACE)
    .arg("host_mid", mid)
    .arg("offset", page.cursor.as_deref().unwrap_or_default())
    .arg("timezone_offset", -480)
    .arg("platform", "web")
    .arg("features", FEATURES)
    .arg("web_location", "333.1387")
    .dm()
    .wbi()
    .send()
    .await?;
  Ok(cursor_page(&data, items(&data, false)))
}

/// One dynamic as served by the desktop detail endpoint. (The web one,
/// `/v1/detail`, mostly answers -352 to anonymous visitors.)
async fn desktop(ctx: &Ctx, id: &str) -> Result<Value> {
  let data = api::get(ctx, DETAIL).arg("id", id).send().await?;
  match data.at("item") {
    Value::Null => Err(Error::not_found(format!("dynamic {id} not found"))),
    item => Ok(item.clone()),
  }
}

/// One dynamic in the web layout (see [`parse::from_desktop`]).
pub async fn detail(ctx: &Ctx, id: &str) -> Result<Value> {
  Ok(parse::from_desktop(&desktop(ctx, id).await?))
}

pub async fn read(ctx: &Ctx, id: &str) -> Result<Post> {
  let item = desktop(ctx, id).await?;
  let mut post = parse::dynamic(&parse::from_desktop(&item));
  post.raw = Some(item);
  Ok(post)
}

/// The dynamic that announced video `v`. Bilibili links none from the video,
/// so it is looked up among the uploader's dynamics down to the publication
/// time (at most [`SCAN_PAGES`] pages) and remembered.
pub async fn of_video(ctx: &Ctx, v: &Video) -> Result<String> {
  let key = format!("bili-video-dynamic-{}", v.bvid);
  if let Some(id) = ctx.store.cache_get::<String>(&key, VIDEO_DYNAMIC_TTL) {
    return Ok(id);
  }
  let view = video::view(ctx, v).await?;
  let mid = view.str("owner.mid").unwrap_or_default();
  let published = view.i64("pubdate").and_then(from_secs);
  let mut req = PageReq::default();
  for _ in 0..SCAN_PAGES {
    let page = of_user(ctx, &mid, &req).await?;
    let found = page
      .items
      .iter()
      .find(|p| p.extra.get("bvid").and_then(Value::as_str) == Some(v.bvid.as_str()));
    if let Some(p) = found {
      ctx.store.cache_put(&key, &p.id);
      return Ok(p.id.clone());
    }
    // Newest first (a pinned dynamic aside): stop once past the video's time.
    let past = page
      .items
      .iter()
      .filter(|p| !p.extra.contains_key("pinned"))
      .filter_map(|p| p.created_at)
      .min()
      .zip(published)
      .is_some_and(|(oldest, at)| oldest < at);
    match page.next_cursor {
      Some(next) if !past => req.cursor = Some(next),
      _ => break,
    }
  }
  Err(
    Error::not_found(format!("found no dynamic announcing {}", v.bvid)).with_hint(
      "pass the dynamic (t.bilibili.com/ID) instead; `media bili dynamics USER` lists them",
    ),
  )
}

/// Accounts that liked dynamic `id` (the reaction list without its reposts).
pub async fn likers(ctx: &Ctx, id: &str, page: &PageReq) -> Result<Page<User>> {
  let data = api::get(ctx, REACTION)
    .arg("id", id)
    .arg("offset", page.cursor.as_deref().unwrap_or_default())
    .arg("web_location", "333.1369")
    .send()
    .await?;
  // `action` is `赞了` for a like, `转发了` for a repost.
  let users = data
    .list("items")
    .iter()
    .filter(|r| r.str("action").is_none_or(|a| a.contains('赞')))
    .map(parse::user)
    .filter(|u| !u.id.is_empty())
    .collect();
  Ok(cursor_page(&data, users))
}

/// Reposts of dynamic `id`, newest first.
pub async fn reposts(ctx: &Ctx, id: &str, page: &PageReq) -> Result<Page<Post>> {
  let data = api::get(ctx, FORWARDS)
    .arg("id", id)
    .arg("offset", page.cursor.as_deref().unwrap_or_default())
    .send()
    .await?;
  let posts = data
    .list("items")
    .iter()
    .map(|item| parse::forward(item, id))
    .filter(|p| !p.id.is_empty())
    .collect();
  Ok(cursor_page(&data, posts))
}

// ── writes ───────────────────────────────────────────────────────────────

async fn upload(ctx: &Ctx, path: &Path) -> Result<Value> {
  let image = Image::read(path).await?;
  let parts = vec![
    Part::file("file_up", image.data, &image.name, image.mime),
    Part::text("biz", "new_dyn"),
    Part::text("category", "daily"),
  ];
  let res = api::post(ctx, UPLOAD).multipart(parts).send().await?;
  Ok(json!({
    "img_src": res.str("image_url"),
    "img_width": res.u64("image_width"),
    "img_height": res.u64("image_height"),
  }))
}

/// A text dynamic, optionally with images and a topic (numeric topic id); `quote` reposts.
pub async fn publish(ctx: &Ctx, draft: &Draft, quote: Option<&str>) -> Result<Action> {
  if draft.reply_to.is_some() {
    return Err(Error::unsupported("post --reply-to (use `comment`)"));
  }
  let text = match &draft.title {
    Some(title) => format!("{title}\n{}", draft.text),
    None => draft.text.clone(),
  };
  match quote {
    Some(_) if !draft.images.is_empty() => Err(Error::input("a repost cannot carry images")),
    Some(id) => repost(ctx, id, &text).await,
    None => create(ctx, &text, draft).await,
  }
}

async fn repost(ctx: &Ctx, id: &str, text: &str) -> Result<Action> {
  let data = api::post(ctx, REPOST)
    .arg("dynamic_id", id)
    .arg("type", 4)
    .arg("rid", 0)
    .arg("content", text)
    .arg("extension", r#"{"emoji_type":1}"#)
    .arg("at_uids", "")
    .arg("ctrl", "[]")
    .send()
    .await?;
  Ok(done("repost", id, &data))
}

async fn create(ctx: &Ctx, text: &str, draft: &Draft) -> Result<Action> {
  let topic = match draft.topics.as_slice() {
    [] => None,
    [t] => Some(
      t.parse::<u64>()
        .map_err(|_| Error::input(format!("Bilibili topics are numeric topic ids, got `{t}`")))?,
    ),
    _ => return Err(Error::input("Bilibili dynamics take one topic")),
  };
  let mut pics = Vec::new();
  for path in &draft.images {
    pics.push(upload(ctx, path).await?);
  }
  let mut req = json!({
    "content": {"contents": [{"raw_text": text, "type": 1, "biz_id": ""}]},
    "scene": if pics.is_empty() { 1 } else { 2 },
    "meta": {"app_meta": {"from": "create.dynamic.web", "mobi_app": "web"}},
    "attach_card": null,
  });
  if !pics.is_empty() {
    req["pics"] = json!(pics);
  }
  if let Some(id) = topic {
    req["topic"] = json!({ "id": id });
  }
  let data = api::post(ctx, CREATE)
    .json(json!({ "dyn_req": req }))
    .dm()
    .wbi()
    .send()
    .await?;
  Ok(done("publish", "dynamic", &data))
}

fn done(action: &str, target: &str, data: &Value) -> Action {
  let action = Action::done(action, target);
  match data.first_str(&["dyn_id_str", "dyn_id", "dynamic_id_str", "dynamic_id"]) {
    Some(id) => action.with_url(format!("{DYNAMIC_URL}{id}")).with_id(id),
    None => action,
  }
}

pub async fn delete(ctx: &Ctx, id: &str) -> Result<Action> {
  api::post(ctx, REMOVE).arg("dynamic_id", id).send().await?;
  Ok(Action::done("delete", id))
}

pub async fn like(ctx: &Ctx, id: &str, undo: bool) -> Result<Action> {
  let uid = account::my_mid(ctx).await?;
  api::post(ctx, THUMB)
    .arg("dynamic_id", id)
    .arg("up", if undo { 2 } else { 1 })
    .arg("uid", uid)
    .send()
    .await?;
  let action = if undo { "unlike" } else { "like" };
  Ok(Action::done(action, id).with_url(format!("{DYNAMIC_URL}{id}")))
}

pub async fn favorite(ctx: &Ctx, id: &str, undo: bool) -> Result<Action> {
  let body = json!({
    "meta": {"spmid": "444.42.0.0", "from_spmid": "333.1365.0.0", "from": "unknown"},
    "entity": {"object_id_str": id, "type": {"biz": 2}},
    "action": if undo { 4 } else { 3 },
  });
  api::post(ctx, COLLECT).json(body).send().await?;
  let action = if undo { "unfavorite" } else { "favorite" };
  Ok(Action::done(action, id).with_url(format!("{DYNAMIC_URL}{id}")))
}
