//! Note search and the home / category feeds.

use std::collections::BTreeMap;
use std::time::Duration;

use media_core::{Error, Page, PageReq, Post, Query, Result, Value, ValueExt, json};
use serde::{Deserialize, Serialize};

use crate::api::Client;
use crate::refs::{self, SOURCE_FEED, SOURCE_SEARCH};
use crate::{parse, sign};

const SEARCH_NOTES: &str = "/api/sns/web/v1/search/notes";
const HOMEFEED: &str = "/api/sns/web/v1/homefeed";

/// `hot --category` values.
pub const HOT_CATEGORIES: &[&str] = &[
  "fashion",
  "food",
  "cosmetics",
  "movie",
  "career",
  "love",
  "home",
  "gaming",
  "travel",
  "fitness",
];
const DEFAULT_HOT: &str = "food";

/// The homefeed channel of a hot category.
fn channel(category: &str) -> Option<&'static str> {
  Some(match category {
    "fashion" => "homefeed.fashion_v3",
    "food" => "homefeed.food_v3",
    "cosmetics" => "homefeed.cosmetics_v3",
    "movie" => "homefeed.movie_and_tv_v3",
    "career" => "homefeed.career_v3",
    "love" => "homefeed.love_v3",
    "home" => "homefeed.household_product_v3",
    "gaming" => "homefeed.gaming_v3",
    "travel" => "homefeed.travel_v3",
    "fitness" => "homefeed.fitness_v3",
    _ => return None,
  })
}

// ── search ──────────────────────────────────────────────────────────────

pub async fn search(c: &Client, q: &Query, page: &PageReq) -> Result<Page<Post>> {
  c.require_login()?;
  let sort = match q.sort.as_deref().unwrap_or("general") {
    "general" => "general",
    "popular" => "popularity_descending",
    "latest" => "time_descending",
    other => return Err(Error::input(format!("unknown sort `{other}`"))),
  };
  let note_type = match q.filter.as_deref() {
    None | Some("all") => 0,
    Some("video") => 1,
    Some("image") => 2,
    Some(other) => return Err(Error::input(format!("unknown filter `{other}`"))),
  };
  let keyword = q.keyword.trim();
  let page_no = page.number_or(1);
  let (search_id, fresh) = search_session(c, keyword, sort, note_type);
  if fresh {
    prewarm(c, keyword, &search_id).await;
  }
  let body = json!({
    "keyword": keyword,
    "page": page_no,
    "page_size": page.size_within(20),
    "search_id": search_id,
    "sort": sort,
    "note_type": note_type,
    "ext_flags": [],
    "filters": [
      {"tags": ["general"], "type": "sort_type"},
      {"tags": ["不限"], "type": "filter_note_type"},
      {"tags": ["不限"], "type": "filter_note_time"},
      {"tags": ["不限"], "type": "filter_note_range"},
      {"tags": ["不限"], "type": "filter_pos_distance"},
    ],
    "geo": "",
    "image_formats": ["jpg", "webp", "avif"],
  });
  let data = c.post(SEARCH_NOTES, &body).await?;
  if fresh {
    let _ = c
      .get("/api/sns/web/v1/search/recommend", &[("keyword", keyword)])
      .await;
  }
  let items = notes(c, &data, SOURCE_SEARCH);
  let next = (data.bool("has_more") == Some(true)).then(|| (page_no + 1).to_string());
  Ok(Page::new(items, next))
}

/// What the web client sends before the first page of a new search. Errors are ignored.
async fn prewarm(c: &Client, keyword: &str, search_id: &str) {
  let request_id = sign::random::search_request_id(sign::now_ms());
  let onebox = json!({
    "keyword": keyword,
    "search_id": search_id,
    "biz_type": "web_search_user",
    "request_id": request_id,
  });
  if let Err(e) = c.post("/api/sns/web/v1/search/onebox", &onebox).await {
    tracing::debug!("search prewarm failed: {e}");
  }
  let params = [("keyword", keyword), ("search_id", search_id)];
  if let Err(e) = c.get("/api/sns/web/v1/search/filter", &params).await {
    tracing::debug!("search prewarm failed: {e}");
  }
}

#[derive(Serialize, Deserialize)]
struct SearchSession {
  id: String,
  used: u64,
}

const SEARCH_SESSION_TTL: u64 = 600;

/// One `search_id` per (keyword, sort, type) for ten minutes, so later pages
/// continue the same search; `true` when it was just created.
fn search_session(c: &Client, keyword: &str, sort: &str, note_type: u8) -> (String, bool) {
  let now = sign::now_ms() / 1000;
  let key = format!("{keyword}\u{1}{sort}\u{1}{note_type}");
  let store = &c.ctx.store;
  let mut map: BTreeMap<String, SearchSession> = store
    .cache_get("search-sessions", Duration::from_secs(SEARCH_SESSION_TTL))
    .unwrap_or_default();
  map.retain(|_, s| now.saturating_sub(s.used) <= SEARCH_SESSION_TTL);
  let fresh = !map.contains_key(&key);
  let session = map.entry(key).or_insert_with(|| SearchSession {
    id: sign::random::search_id(sign::now_ms()),
    used: now,
  });
  session.used = now;
  let id = session.id.clone();
  store.cache_put("search-sessions", &map);
  (id, fresh)
}

// ── feeds ───────────────────────────────────────────────────────────────

pub async fn feed(c: &Client, kind: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
  match kind {
    None | Some("recommend") => homefeed(c, "homefeed_recommend", page).await,
    Some(other) => hot(c, Some(other), page).await,
  }
}

pub async fn hot(c: &Client, category: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
  let name = category.unwrap_or(DEFAULT_HOT);
  let channel = channel(name).ok_or_else(|| Error::input(format!("unknown category `{name}`")))?;
  homefeed(c, channel, page).await
}

/// One homefeed page; the cursor is `<cursor_score>|<note_index>`.
async fn homefeed(c: &Client, category: &str, page: &PageReq) -> Result<Page<Post>> {
  c.require_login()?;
  let cursor = page.cursor.as_deref().unwrap_or_default();
  let (score, index) = cursor.split_once('|').unwrap_or((cursor, "0"));
  let index: u64 = index.parse().unwrap_or(0);
  let num = page.size_within(40);
  let body = json!({
    "cursor_score": score,
    "num": num,
    "refresh_type": 1,
    "note_index": index,
    "unread_begin_note_id": "",
    "unread_end_note_id": "",
    "unread_note_count": 0,
    "category": category,
    "search_key": "",
    "need_num": num,
    "image_scenes": ["FD_PRV_WEBP", "FD_WM_WEBP"],
  });
  let data = c.post(HOMEFEED, &body).await?;
  let items = notes(c, &data, SOURCE_FEED);
  let next = data
    .str("cursor_score")
    .filter(|_| !items.is_empty())
    .map(|s| format!("{s}|{}", index + items.len() as u64));
  Ok(Page::new(items, next))
}

/// Notes of a listing (`items[].note_card`), remembering their tokens.
fn notes(c: &Client, data: &Value, source: &str) -> Vec<Post> {
  let items: Vec<&Value> = data
    .list("items")
    .iter()
    .filter(|i| i.at("note_card").is_object())
    .collect();
  let tokens: Vec<(String, String)> = items
    .iter()
    .filter_map(|i| Some((i.str("id")?, i.str("xsec_token")?)))
    .collect();
  refs::remember(
    &c.ctx,
    tokens
      .iter()
      .map(|(id, t)| (id.as_str(), t.as_str(), source)),
  );
  items
    .into_iter()
    .filter_map(|i| parse::note(i, source))
    .collect()
}
