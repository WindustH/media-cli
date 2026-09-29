//! Dynamics: the following feed, a user's dynamics, one dynamic, and the
//! dynamic writes (publish with images, repost, delete, like, favorite).

use std::path::Path;

use media_core::file::Image;
use media_core::http::Part;
use media_core::{Action, Ctx, Draft, Error, Page, PageReq, Post, Result, Value, ValueExt, json};

use crate::refs::DYNAMIC_URL;
use crate::{account, api, parse};

const FEED: &str = "https://api.bilibili.com/x/polymer/web-dynamic/v1/feed/all";
const SPACE: &str = "https://api.bilibili.com/x/polymer/web-dynamic/v1/feed/space";
const DETAIL: &str = "https://api.bilibili.com/x/polymer/web-dynamic/desktop/v1/detail";
const CREATE: &str = "https://api.bilibili.com/x/dynamic/feed/create/dyn";
const UPLOAD: &str = "https://api.bilibili.com/x/dynamic/feed/draw/upload_bfs";
const REPOST: &str = "https://api.vc.bilibili.com/dynamic_repost/v1/dynamic_repost/repost";
const REMOVE: &str = "https://api.vc.bilibili.com/dynamic_svr/v1/dynamic_svr/rm_dynamic";
const THUMB: &str = "https://api.vc.bilibili.com/dynamic_like/v1/dynamic_like/thumb";
const COLLECT: &str = "https://api.bilibili.com/x/community/cosmo/interface/simple_action";
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

fn cursor_page(data: &Value, posts: Vec<Post>) -> Page<Post> {
  let next = data
    .str("offset")
    .filter(|_| data.bool("has_more") == Some(true));
  Page::new(posts, next)
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
