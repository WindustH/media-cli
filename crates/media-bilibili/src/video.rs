//! Video endpoints: details, search, popular / ranking, related, uploads of a
//! user, watch later, and the video-only writes (like, coin, triple).

use media_core::{Action, Ctx, Error, Page, PageReq, Post, Query, Result, User, Value, ValueExt};

use crate::api;
use crate::page::Pn;
use crate::parse;
use crate::refs::Video;

const VIEW: &str = "https://api.bilibili.com/x/web-interface/view";
const SEARCH: &str = "https://api.bilibili.com/x/web-interface/wbi/search/type";
const POPULAR: &str = "https://api.bilibili.com/x/web-interface/popular";
const RANKING: &str = "https://api.bilibili.com/x/web-interface/ranking/v2";
const RELATED: &str = "https://api.bilibili.com/x/web-interface/archive/related";
const UPLOADS: &str = "https://api.bilibili.com/x/space/wbi/arc/search";
const TOVIEW: &str = "https://api.bilibili.com/x/v2/history/toview";

/// `hot --category`: `popular` (default) and the ranking regions below.
pub const HOT_CATEGORIES: &[&str] = &[
  "popular",
  "douga",
  "music",
  "dance",
  "game",
  "knowledge",
  "tech",
  "sports",
  "car",
  "life",
  "food",
  "animal",
  "kichiku",
  "fashion",
  "ent",
  "cinephile",
  "guochuang",
  "origin",
  "rookie",
];

/// Ranking regions `(category, rid, type)` behind `hot --category`.
const REGIONS: &[(&str, u32, &str)] = &[
  ("douga", 1005, "all"),
  ("music", 1003, "all"),
  ("dance", 1004, "all"),
  ("game", 1008, "all"),
  ("knowledge", 1010, "all"),
  ("tech", 1012, "all"),
  ("sports", 1018, "all"),
  ("car", 1013, "all"),
  ("life", 160, "all"),
  ("food", 1020, "all"),
  ("animal", 1024, "all"),
  ("kichiku", 1007, "all"),
  ("fashion", 1014, "all"),
  ("ent", 1002, "all"),
  ("cinephile", 1001, "all"),
  ("guochuang", 168, "all"),
  ("origin", 0, "origin"),
  ("rookie", 0, "rookie"),
];

fn videos(list: &[Value]) -> Vec<Post> {
  list
    .iter()
    .map(parse::video)
    .filter(|p| !p.id.is_empty())
    .collect()
}

/// Raw `view` data (title, owner, stat, pages ...).
pub async fn view(ctx: &Ctx, v: &Video) -> Result<Value> {
  api::get(ctx, VIEW).arg("bvid", &v.bvid).send().await
}

pub async fn read(ctx: &Ctx, v: &Video) -> Result<Post> {
  Ok(parse::video(&view(ctx, v).await?))
}

/// `cid` of part `page` (1-based) from `view` data.
pub fn cid(view: &Value, page: usize) -> Result<u64> {
  let pages = view.list("pages");
  pages
    .get(page.saturating_sub(1))
    .and_then(|p| p.u64("cid"))
    .or_else(|| (page == 1).then(|| view.u64("cid")).flatten())
    .ok_or_else(|| Error::not_found(format!("part {page} not found ({} parts)", pages.len())))
}

/// One page of `search/type`; `order` only applies to videos.
async fn search_type(
  ctx: &Ctx,
  kind: &str,
  keyword: &str,
  order: Option<&str>,
  page: &PageReq,
) -> Result<(Value, Pn)> {
  let pn = Pn::of(page, 50);
  let mut call = api::get(ctx, SEARCH)
    .arg("search_type", kind)
    .arg("keyword", keyword)
    .arg("page", pn.pn)
    .arg("page_size", pn.ps);
  if let Some(order) = order {
    call = call.arg("order", order);
  }
  Ok((call.wbi().send().await?, pn))
}

pub async fn search(ctx: &Ctx, q: &Query, page: &PageReq) -> Result<Page<Post>> {
  let (data, pn) = search_type(ctx, "video", &q.keyword, q.sort.as_deref(), page).await?;
  let more = pn.pn < data.u64("numPages").unwrap_or(0);
  Ok(pn.page(videos(data.list("result")), more))
}

pub async fn search_users(ctx: &Ctx, q: &Query, page: &PageReq) -> Result<Page<User>> {
  let (data, pn) = search_type(ctx, "bili_user", &q.keyword, None, page).await?;
  let more = pn.pn < data.u64("numPages").unwrap_or(0);
  let users = data.list("result").iter().map(parse::user).collect();
  Ok(pn.page(users, more))
}

/// `popular` (paged) or one ranking region (single page).
pub async fn hot(ctx: &Ctx, category: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
  let category = category.unwrap_or("popular");
  if category == "popular" {
    let pn = Pn::of(page, 50);
    let data = api::get(ctx, POPULAR)
      .arg("pn", pn.pn)
      .arg("ps", pn.ps)
      .arg("web_location", "333.934")
      .wbi()
      .send()
      .await?;
    let more = data.bool("no_more") == Some(false);
    return Ok(pn.page(videos(data.list("list")), more));
  }
  let (_, rid, kind) = REGIONS
    .iter()
    .find(|(name, ..)| *name == category)
    .ok_or_else(|| Error::input(format!("unknown category: {category}")))?;
  ranking(ctx, *rid, kind).await
}

/// One ranking board (`rid` 0 = whole site).
pub async fn ranking(ctx: &Ctx, rid: u32, kind: &str) -> Result<Page<Post>> {
  let data = api::get(ctx, RANKING)
    .arg("rid", rid)
    .arg("type", kind)
    .arg("web_location", "333.934")
    .wbi()
    .send()
    .await?;
  Ok(Page::last(videos(data.list("list"))))
}

pub async fn related(ctx: &Ctx, v: &Video) -> Result<Page<Post>> {
  let data = api::get(ctx, RELATED).arg("bvid", &v.bvid).send().await?;
  Ok(Page::last(videos(data.list(""))))
}

/// Videos uploaded by `mid`, newest first.
pub async fn uploads(ctx: &Ctx, mid: &str, page: &PageReq) -> Result<Page<Post>> {
  let pn = Pn::of(page, 50);
  let data = api::get(ctx, UPLOADS)
    .arg("mid", mid)
    .arg("pn", pn.pn)
    .arg("ps", pn.ps)
    .arg("order", "pubdate")
    .arg("platform", "web")
    .arg("web_location", "333.1387")
    .dm()
    .wbi()
    .send()
    .await?;
  let more = pn.before(data.u64("page.count"));
  Ok(pn.page(videos(data.list("list.vlist")), more))
}

pub async fn watch_later(ctx: &Ctx) -> Result<Page<Post>> {
  ctx.require_login(api::LOGIN_COOKIES)?;
  let data = api::get(ctx, TOVIEW).send().await?;
  Ok(Page::last(videos(data.list("list"))))
}

// ── writes ───────────────────────────────────────────────────────────────

pub async fn like(ctx: &Ctx, v: &Video, undo: bool) -> Result<Action> {
  api::post(ctx, "https://api.bilibili.com/x/web-interface/archive/like")
    .arg("aid", v.aid)
    .arg("like", if undo { 2 } else { 1 })
    .send()
    .await?;
  let action = if undo { "unlike" } else { "like" };
  Ok(Action::done(action, &v.bvid).with_url(v.url()))
}

pub async fn coin(ctx: &Ctx, v: &Video, count: u8, like: bool) -> Result<Action> {
  api::post(ctx, "https://api.bilibili.com/x/web-interface/coin/add")
    .arg("aid", v.aid)
    .arg("multiply", count)
    .arg("select_like", u8::from(like))
    .send()
    .await?;
  Ok(
    Action::done("coin", &v.bvid)
      .with_url(v.url())
      .with_message(format!(
        "{count} coin(s){}",
        if like { " + like" } else { "" }
      )),
  )
}

/// Like + coin + favorite in one request.
pub async fn triple(ctx: &Ctx, v: &Video) -> Result<Action> {
  let data = api::post(
    ctx,
    "https://api.bilibili.com/x/web-interface/archive/like/triple",
  )
  .arg("aid", v.aid)
  .send()
  .await?;
  let done: Vec<&str> = [("like", "like"), ("coin", "coin"), ("fav", "favorite")]
    .into_iter()
    .filter(|(k, _)| data.bool(k) == Some(true))
    .map(|(_, label)| label)
    .collect();
  Ok(
    Action::done("triple", &v.bvid)
      .with_url(v.url())
      .with_message(format!("done: {}", done.join(", "))),
  )
}
