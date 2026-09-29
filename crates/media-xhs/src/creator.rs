//! Creator-platform endpoints (`CreatorEndpointsMixin`): user / topic search,
//! image upload and publishing, deleting and listing your own notes.

use std::path::Path;
use std::time::Duration;

use media_core::{
  Action, Collection, Draft, Error, ErrorCode, Page, PageReq, Post, Query, Result, User, Value,
  ValueExt, json,
};

use crate::api::Client;
use crate::parse;
use crate::refs::{self, note_url};

const PUBLISH: &str = "/web_api/sns/v2/note";

// ── search ──────────────────────────────────────────────────────────────

pub async fn search_users(c: &Client, q: &Query, page: &PageReq) -> Result<Page<User>> {
  c.require_login()?;
  let (no, size) = (page.number_or(1), page.size_within(20));
  let body = json!({
    "keyword": q.keyword,
    "search_id": crate::sign::now_ms().to_string(),
    "page": {"page_size": size, "page": no},
  });
  let data = c
    .creator_post("/web_api/sns/v1/search/user_info", &body)
    .await?;
  let rows = match &data {
    Value::Array(a) => a.as_slice(),
    _ => ["user_info_dtos", "users", "items"]
      .iter()
      .map(|k| data.list(k))
      .find(|l| !l.is_empty())
      .unwrap_or_default(),
  };
  let items: Vec<User> = rows.iter().filter_map(parse::search_user).collect();
  Ok(numbered(items, no, size))
}

pub async fn search_topics(c: &Client, q: &Query, page: &PageReq) -> Result<Page<Collection>> {
  c.require_login()?;
  let (no, size) = (page.number_or(1), page.size_within(20));
  let data = topic_search(c, &q.keyword, no, size).await?;
  let items: Vec<Collection> = topic_rows(&data).iter().filter_map(parse::topic).collect();
  Ok(numbered(items, no, size))
}

async fn topic_search(c: &Client, keyword: &str, page: u64, size: usize) -> Result<Value> {
  let body = json!({
    "keyword": keyword,
    "suggest_topic_request": {"title": "", "desc": ""},
    "page": {"page_size": size, "page": page},
  });
  c.creator_post("/web_api/sns/v1/search/topic", &body).await
}

fn topic_rows(data: &Value) -> &[Value] {
  match data {
    Value::Array(a) => a,
    _ => data.list("topic_info_dtos"),
  }
}

/// Page-numbered listing without a `has_more` flag: a full page may have more.
fn numbered<T>(items: Vec<T>, page: u64, size: usize) -> Page<T> {
  let next = (items.len() >= size).then(|| (page + 1).to_string());
  Page::new(items, next)
}

// ── publishing ──────────────────────────────────────────────────────────

/// Publish an image note: upload each image, resolve topics, post.
pub async fn publish(c: &Client, draft: &Draft) -> Result<Action> {
  if draft.images.is_empty() {
    return Err(Error::input(
      "a Xiaohongshu note needs at least one image (--image)",
    ));
  }
  if draft.reply_to.is_some() || draft.quote.is_some() {
    return Err(Error::unsupported("post --reply-to / --quote"));
  }
  c.require_login()?;
  let mut file_ids = Vec::new();
  for path in &draft.images {
    file_ids.push(upload_image(c, path).await?);
  }
  let mut hash_tag = Vec::new();
  for name in topic_names(draft) {
    let data = topic_search(c, &name, 1, 20).await?;
    if let Some(first) = topic_rows(&data).first() {
      hash_tag.push(json!({
        "id": first.str("id").unwrap_or_default(),
        "name": first.str("name").unwrap_or(name),
        "type": "topic",
      }));
    }
  }
  let images: Vec<Value> = file_ids
    .iter()
    .map(|id| json!({"file_id": id, "metadata": {"source": -1}}))
    .collect();
  let body = json!({
    "common": {
      "type": "normal",
      "title": draft.title.as_deref().unwrap_or_default(),
      "note_id": "",
      "desc": draft.text,
      "source": r#"{"type":"web","ids":"","extraInfo":"{\"subType\":\"official\"}"}"#,
      // Python's default `json.dumps` spacing, as the reference sends it.
      "business_binds": r#"{"version": 1, "noteId": 0, "noteOrderBind": {}, "notePostTiming": {"postTime": null}, "noteCollectionBind": {"id": ""}}"#,
      "ats": [],
      "hash_tag": hash_tag,
      "post_loc": {},
      "privacy_info": {"op_type": 1, "type": 0},
    },
    "image_info": {"images": images},
    "video_info": null,
  });
  c.ctx
    .http
    .pause(Duration::from_millis(1000), Duration::from_millis(2500))
    .await;
  let creator = [
    ("origin", crate::api::CREATOR),
    ("referer", "https://creator.xiaohongshu.com/"),
  ];
  let data = c.post_with(PUBLISH, &body, &creator).await?;
  let mut action = Action::done("publish", draft.title.as_deref().unwrap_or("note"));
  if let Some(id) = data.first_str(&["id", "note_id"]) {
    action = action
      .with_url(note_url(&id, None, refs::SOURCE_FEED))
      .with_id(id);
  }
  Ok(action)
}

/// `--topic` values plus `#tags` of the text, deduplicated, at most 10.
fn topic_names(draft: &Draft) -> Vec<String> {
  let from_text = draft.text.split_whitespace().filter_map(|w| {
    let tag = w.strip_prefix('#')?.split('#').next()?;
    (!tag.is_empty()).then(|| tag.to_owned())
  });
  let mut names: Vec<String> = Vec::new();
  for name in draft.topics.iter().cloned().chain(from_text) {
    if !names.contains(&name) {
      names.push(name);
    }
  }
  names.truncate(10);
  names
}

async fn upload_image(c: &Client, path: &Path) -> Result<String> {
  let params = [
    ("biz_name", "spectrum"),
    ("scene", "image"),
    ("file_count", "1"),
    ("version", "1"),
    ("source", "web"),
  ];
  let data = c
    .creator_get("/api/media/v1/upload/web/permit", &params)
    .await?;
  let permit = data.at("uploadTempPermits.0");
  let (Some(file_id), Some(token)) = (permit.str("fileIds.0"), permit.str("token")) else {
    return Err(Error::upstream("upload permit without file id or token"));
  };
  let bytes = tokio::fs::read(path).await?;
  c.upload(&file_id, &token, bytes, mime(path)).await?;
  Ok(file_id)
}

fn mime(path: &Path) -> &'static str {
  let ext = path
    .extension()
    .map(|e| e.to_string_lossy().to_ascii_lowercase())
    .unwrap_or_default();
  match ext.as_str() {
    "jpg" | "jpeg" => "image/jpeg",
    "png" => "image/png",
    "webp" => "image/webp",
    "gif" => "image/gif",
    "heic" => "image/heic",
    _ => "application/octet-stream",
  }
}

// ── own notes ───────────────────────────────────────────────────────────

pub async fn delete(c: &Client, arg: &str) -> Result<Action> {
  c.require_login()?;
  let id = refs::note_ref(c, arg).await?.id;
  let body = json!({"note_id": id});
  match c
    .creator_post("/api/galaxy/creator/note/delete", &body)
    .await
  {
    Ok(_) => Ok(Action::done("delete", &id)),
    Err(e) if e.code == ErrorCode::NotFound || e.message.contains("404") => Err(
      Error::unsupported("delete")
        .with_hint("the web endpoint for deleting notes is currently unavailable"),
    ),
    Err(e) => Err(e),
  }
}

/// One page (0-based) of the creator center note list.
pub async fn my_notes(c: &Client, page: u32) -> Result<Page<Post>> {
  c.require_login()?;
  let page_s = page.to_string();
  let data = c
    .creator_get(
      "/api/galaxy/v2/creator/note/user/posted",
      &[("tab", "0"), ("page", &page_s)],
    )
    .await?;
  let rows = match data.list("notes") {
    [] => data.list("note_list"),
    rows => rows,
  };
  let items: Vec<Post> = rows.iter().filter_map(parse::creator_note).collect();
  // The list reports the next page, or -1 at the end.
  let next = match data.i64("page") {
    Some(n) if n < 0 => None,
    _ if items.is_empty() => None,
    _ => Some((page + 1).to_string()),
  };
  Ok(Page::new(items, next))
}
