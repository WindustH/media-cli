//! Creator-platform endpoints (`CreatorEndpointsMixin`): user / topic search,
//! image upload and publishing, deleting and listing your own notes, and the
//! data center's note and active-fan lists.

use std::path::Path;
use std::time::Duration;

use media_core::file::Image;
use media_core::{
  Action, Collection, Draft, Error, Page, PageReq, Post, Query, Result, User, Value, ValueExt, json,
};

use crate::api::Client;
use crate::events::Association;
use crate::refs::{self, note_url};
use crate::{parse, stats};

/// Creator-center requests go to edith but come from the creator site.
const CREATOR_PAGE: [(&str, &str); 2] = [
  ("origin", crate::api::CREATOR),
  ("referer", "https://creator.xiaohongshu.com/"),
];
const DELETE: &str = "/web_api/sns/capa/postgw/note/delete";
const PUBLISH: &str = "/web_api/sns/v2/note";
const SOURCE: &str = r#"{"type":"web","ids":"","extraInfo":"{\"subType\":\"official\"}"}"#;
// Python's default `json.dumps` spacing, as the reference sends it.
const BINDS: &str = r#"{"version": 1, "noteId": 0, "noteOrderBind": {}, "notePostTiming": {"postTime": null}, "noteCollectionBind": {"id": ""}}"#;

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
  publish_note(c, draft, None).await
}

/// Publish an image note, joined to an activity-center activity when given.
pub async fn publish_note(
  c: &Client,
  draft: &Draft,
  event: Option<&Association>,
) -> Result<Action> {
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
  let mut hash_tag: Vec<Value> = event.map(|e| e.topics.clone()).unwrap_or_default();
  for name in topic_names(draft) {
    if hash_tag
      .iter()
      .any(|t| t.str("name").as_deref() == Some(name.as_str()))
    {
      continue;
    }
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
  let (desc, source, binds) = match event {
    None => (draft.text.clone(), SOURCE.to_owned(), BINDS.to_owned()),
    Some(e) => (
      // The publish page writes the activity's topics into the text too.
      e.topics
        .iter()
        .filter_map(|t| t.str("name"))
        .fold(draft.text.clone(), |d, n| format!("{d} #{n}[话题]#")),
      e.source(),
      json!({
        "version": 1,
        "noteId": 0,
        "bizType": 0,
        "noteOrderBind": {},
        "notePostTiming": {"postTime": null},
        "noteCollectionBind": {"id": ""},
        "optionRelationList": [e.relation],
      })
      .to_string(),
    ),
  };
  let body = json!({
    "common": {
      "type": "normal",
      "title": draft.title.as_deref().unwrap_or_default(),
      "note_id": "",
      "desc": desc.trim_start(),
      "source": source,
      "business_binds": binds,
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
  let data = c.post_with(PUBLISH, &body, &CREATOR_PAGE).await?;
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
  let permits = c
    .creator_get("/api/media/v1/upload/web/permit", &params)
    .await?;
  let image = Image::read(path).await?;
  c.upload(&permits, image.data, image.mime).await
}

// ── own notes ───────────────────────────────────────────────────────────

pub async fn delete(c: &Client, arg: &str) -> Result<Action> {
  c.require_login()?;
  let id = refs::note_ref(c, arg).await?.id;
  // The note manager's DELETE_NOTE call (creator center bundle), sent like publishing.
  c.post_with(DELETE, &json!({"note_id": id}), &CREATOR_PAGE)
    .await?;
  Ok(Action::done("delete", &id))
}

/// One page (0-based) of the creator center note list.
pub async fn my_notes(c: &Client, req: &PageReq) -> Result<Page<Post>> {
  c.require_login()?;
  let page = req.number_or(0);
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
  // The list reports the next page to ask for (the note manager's `page`), or -1 at the end.
  let next = match data.i64("page") {
    Some(n) if n < 0 => None,
    _ if items.is_empty() => None,
    Some(n) if n as u64 > page => Some(n.to_string()),
    _ => Some((page + 1).to_string()),
  };
  Ok(Page::new(items, next))
}

// ── data center listings ────────────────────────────────────────────────

/// 内容分析 note list (`NOTE_ANALYZE_LIST`, called by chunk 4323).
const NOTE_STATS: &str = "/api/galaxy/creator/datacenter/note/analyze/list";
/// 我的活跃粉丝 (`ACTIVE_FANS_NEW`, called by chunk 7763).
const ACTIVE_FANS: &str = "/api/galaxy/creator/data/active_fans_new";

/// Your notes with their numbers, newest first, 10 per page like the page.
pub async fn note_stats(c: &Client, req: &PageReq) -> Result<Page<Post>> {
  c.require_login()?;
  let (page, size) = (req.number_or(1), 10);
  let page_s = page.to_string();
  let params = [
    ("type", "0"),
    ("page_size", "10"),
    ("page_num", page_s.as_str()),
  ];
  let data = parse::snake_keys(c.creator_get(NOTE_STATS, &params).await?);
  let items: Vec<Post> = data
    .list("note_infos")
    .iter()
    .filter_map(stats::note_row)
    .collect();
  let more = !items.is_empty()
    && match data.u64("total") {
      Some(total) => page * size < total,
      None => items.len() as u64 >= size,
    };
  Ok(Page::new(items, more.then(|| (page + 1).to_string())))
}

/// Fans who interacted most over the last 7 or 30 days.
pub async fn active_fans(c: &Client, days: u32) -> Result<Page<User>> {
  c.require_login()?;
  let data = parse::snake_keys(c.creator_get(ACTIVE_FANS, &[]).await?);
  let rows = data.list(crate::insights::window(days).0);
  Ok(Page::last(
    rows.iter().filter_map(stats::active_fan).collect(),
  ))
}
