//! Upstream JSON → core models. Web API payloads are snake_case; the note of
//! a server-rendered page is converted with [`snake_keys`] first, so one
//! parser serves both.

use std::sync::LazyLock;

use media_core::text::{from_millis, from_unix};
use media_core::{
  Collection, Comment, Extra, Media, Metrics, Notification, Post, User, UserStats, Value, ValueExt,
};
use regex::Regex;

use crate::refs::{SOURCE_FEED, note_url, user_url};

/// Platform-only fields, skipping null / empty values.
fn extra<const N: usize>(pairs: [(&str, &Value); N]) -> Extra {
  pairs
    .into_iter()
    .filter(|(_, v)| {
      !v.is_null() && v.as_str() != Some("") && v.as_array().is_none_or(|a| !a.is_empty())
    })
    .map(|(k, v)| (k.to_owned(), v.clone()))
    .collect()
}

// ── users ───────────────────────────────────────────────────────────────

/// Author / commenter / actor objects (`user`, `user_info`).
pub fn user_brief(v: &Value) -> Option<User> {
  let id = v.first_str(&["user_id", "userid", "id"])?;
  Some(User {
    url: Some(user_url(&id)),
    name: v
      .first_str(&["nickname", "nick_name", "user_nickname"])
      .unwrap_or_default(),
    avatar: v.first_str(&["avatar", "image", "images"]),
    id,
    ..User::default()
  })
}

/// `user/me` and `user/otherinfo` payloads.
pub fn profile(v: &Value, id: Option<&str>) -> User {
  let basic = match v.at("basic_info") {
    b @ Value::Object(_) => b,
    _ => v,
  };
  let id = id
    .map(str::to_owned)
    .or_else(|| basic.str("user_id"))
    .or_else(|| v.str("user_id"))
    .unwrap_or_default();
  let stat = |kind: &str| {
    v.list("interactions")
      .iter()
      .find(|i| i.str("type").as_deref() == Some(kind))
      .and_then(|i| i.count("count"))
  };
  let followed = v
    .str("extra_info.fstatus")
    .map(|s| matches!(s.as_str(), "follows" | "both"));
  let tags: Value = v
    .list("tags")
    .iter()
    .filter_map(|t| t.str("name").map(Value::from))
    .collect();
  User {
    url: Some(user_url(&id)),
    name: basic
      .first_str(&["nickname", "nick_name"])
      .unwrap_or_default(),
    handle: basic.str("red_id"),
    avatar: basic.first_str(&["imageb", "images", "image", "avatar"]),
    bio: basic.str("desc"),
    location: basic.str("ip_location"),
    stats: UserStats {
      followers: stat("fans"),
      following: stat("follows"),
      // "获赞与收藏": likes plus collects received.
      likes: stat("interaction"),
      ..UserStats::default()
    },
    followed,
    extra: extra([("gender", basic.at("gender")), ("tags", &tags)]),
    raw: Some(v.clone()),
    id,
    ..User::default()
  }
}

/// A row of the creator user search.
pub fn search_user(v: &Value) -> Option<User> {
  let base = match v.at("user_base_dto") {
    b @ Value::Object(_) => b,
    _ => v,
  };
  let mut user = user_brief(base)?;
  user.handle = base.str("red_id");
  user.bio = base.str("desc");
  user.stats.followers = v
    .count("fans_total")
    .or_else(|| base.first_count(&["fans", "fansCount", "fans_total"]));
  user.raw = Some(v.clone());
  Some(user)
}

// ── notes ───────────────────────────────────────────────────────────────

static TOPIC_MARK: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"#([^#\[\]]+)\[话题\]#").expect("regex"));

/// A note from a listing item (`{id, xsec_token, note_card}`), a flat note
/// (`user_posted`, collect / like pages) or a detail card.
pub fn note(item: &Value, source: &str) -> Option<Post> {
  let card = match item.at("note_card") {
    c @ Value::Object(_) => c,
    _ => item,
  };
  let id = item
    .first_str(&["id", "note_id"])
    .or_else(|| card.first_str(&["note_id", "id"]))?;
  let token = item.str("xsec_token").or_else(|| card.str("xsec_token"));
  let interact = card.at("interact_info");
  let mut media = images(card);
  media.extend(video(card));
  Some(Post {
    kind: kind(card),
    title: card.first_str(&["title", "display_title"]),
    text: card
      .str("desc")
      .map(|d| TOPIC_MARK.replace_all(&d, "#$1").into_owned()),
    url: Some(note_url(&id, token.as_deref(), source)),
    author: user_brief(card.at("user")),
    created_at: card.i64("time").and_then(from_millis),
    updated_at: card.i64("last_update_time").and_then(from_millis),
    metrics: Metrics {
      likes: interact.count("liked_count"),
      comments: interact.count("comment_count"),
      shares: interact.first_count(&["share_count", "shared_count"]),
      favorites: interact.count("collected_count"),
      ..Metrics::default()
    },
    media,
    tags: card
      .list("tag_list")
      .iter()
      .filter_map(|t| t.str("name"))
      .collect(),
    extra: extra([
      ("type", card.at("type")),
      ("ip_location", card.at("ip_location")),
      ("liked", interact.at("liked")),
      ("collected", interact.at("collected")),
      ("sticky", interact.at("sticky")),
      ("cover", &cover(card)),
    ]),
    raw: Some(item.clone()),
    id,
    ..Post::default()
  })
}

/// `video` for video notes, `note` for image notes (`type: normal`).
fn kind(card: &Value) -> String {
  match card.str("type").as_deref() {
    Some("video") => "video",
    _ => "note",
  }
  .to_owned()
}

fn cover(card: &Value) -> Value {
  card
    .at("cover")
    .first_str(&["url_default", "url", "info_list.1.url", "info_list.0.url"])
    .map_or(Value::Null, Value::from)
}

/// Full-resolution images. Display URLs
/// (`http://sns-webpic-qc.xhscdn.com/<time>/<hash>/<token>!<style>`) are
/// downscaled; the file token served by the image service keeps the upload's
/// resolution. The stored original is often HEIC, so it is requested as JPEG.
fn images(card: &Value) -> Vec<Media> {
  let mut out = Vec::new();
  for img in card.list("image_list") {
    let Some(src) = img.first_str(&[
      "url_default",
      "url",
      "url_pre",
      "info_list.1.url",
      "info_list.0.url",
    ]) else {
      continue;
    };
    let mut m = Media::image(original_image(&src).unwrap_or(src));
    m.width = img.u64("width").map(|w| w as u32);
    m.height = img.u64("height").map(|h| h as u32);
    out.push(m);
    if img.bool("live_photo") == Some(true) {
      out.extend(best_stream(img.at("stream")));
    }
  }
  out
}

fn original_image(url: &str) -> Option<String> {
  let token = url.split('/').skip(5).collect::<Vec<_>>().join("/");
  let token = token.split('!').next().filter(|t| !t.is_empty())?;
  Some(format!(
    "https://ci.xiaohongshu.com/{token}?imageView2/format/jpeg"
  ))
}

fn video(card: &Value) -> Option<Media> {
  let v = card.at("video");
  let mut m = best_stream(v.at("media.stream"))?;
  if let Some(secs) = v.f64("capa.duration") {
    m.duration = Some(secs);
  }
  Some(m)
}

/// The highest-resolution stream, preferring H.264 on ties.
fn best_stream(streams: &Value) -> Option<Media> {
  let mut best: Option<(u64, &Value)> = None;
  for codec in ["h264", "h265", "av1", "h266"] {
    for s in streams.list(codec) {
      let area = s.u64("width").unwrap_or(0) * s.u64("height").unwrap_or(0);
      if s.str("master_url").is_some() && best.is_none_or(|(a, _)| area > a) {
        best = Some((area, s));
      }
    }
  }
  let (_, s) = best?;
  let mut m = Media::video(s.str("master_url")?);
  m.width = s.u64("width").map(|w| w as u32);
  m.height = s.u64("height").map(|h| h as u32);
  m.duration = s.f64("duration").map(|ms| ms / 1000.0);
  Some(m)
}

/// A note of the creator center list (`my-notes`).
pub fn creator_note(v: &Value) -> Option<Post> {
  let id = v.first_str(&["note_id", "id"])?;
  let token = v.str("xsec_token");
  Some(Post {
    kind: kind(v),
    title: v.first_str(&["title", "display_title"]),
    url: Some(note_url(&id, token.as_deref(), SOURCE_FEED)),
    metrics: Metrics {
      views: v.first_count(&["view_count"]),
      likes: v.first_count(&["liked_count", "likes", "interact_info.liked_count"]),
      comments: v.first_count(&[
        "comment_count",
        "comments_count",
        "interact_info.comment_count",
      ]),
      shares: v.first_count(&["shared_count", "share_count"]),
      favorites: v.first_count(&["collected_count", "interact_info.collected_count"]),
      ..Metrics::default()
    },
    extra: extra([("time", v.at("time")), ("status", v.at("status"))]),
    raw: Some(v.clone()),
    id,
    ..Post::default()
  })
}

// ── comments, topics, notifications ─────────────────────────────────────

pub fn comment(v: &Value) -> Comment {
  let pictures: Value = v
    .list("pictures")
    .iter()
    .filter_map(|p| p.first_str(&["url_default", "url_pre"]).map(Value::from))
    .collect();
  Comment {
    id: v.str("id").unwrap_or_default(),
    author: user_brief(v.at("user_info")),
    text: v.str("content").unwrap_or_default(),
    created_at: v.i64("create_time").and_then(from_millis),
    likes: v.count("like_count"),
    reply_count: v.count("sub_comment_count"),
    reply_to: v.str("target_comment.user_info.nickname"),
    location: v.str("ip_location"),
    replies: v.list("sub_comments").iter().map(comment).collect(),
    extra: extra([
      ("liked", v.at("liked")),
      ("show_tags", v.at("show_tags")),
      ("pictures", &pictures),
    ]),
    raw: Some(v.clone()),
  }
}

pub fn topic(v: &Value) -> Option<Collection> {
  Some(Collection {
    id: v.str("id")?,
    kind: "topic".into(),
    name: v.str("name").unwrap_or_default(),
    description: v.str("desc"),
    views: v.count("view_num"),
    raw: Some(v.clone()),
    ..Collection::default()
  })
}

/// One message of the mentions / likes / connections inboxes.
pub fn notification(v: &Value, fallback_kind: &str) -> Notification {
  let kind = match v.str("type").as_deref().and_then(|t| t.split('/').next()) {
    Some("comment") if v.str("type").is_some_and(|t| t.ends_with("/comment")) => "reply",
    Some("comment") => "comment",
    Some("mention") => "mention",
    Some("like") => "like",
    Some("collect") => "favorite",
    Some("follow" | "fans") => "follow",
    _ => fallback_kind,
  };
  let title = v.str("title").unwrap_or_default();
  let text = match v.str("comment_info.content") {
    Some(c) if !title.is_empty() => format!("{title}: {c}"),
    Some(c) => c,
    None => title,
  };
  let item = v.at("item_info");
  let url = item
    .str("id")
    .filter(|_| item.str("type").as_deref() != Some("user"))
    .map(|id| note_url(&id, item.str("xsec_token").as_deref(), SOURCE_FEED));
  Notification {
    id: v.str("id").unwrap_or_default(),
    kind: kind.to_owned(),
    text,
    actor: user_brief(v.at("user_info")),
    target: item.str("content"),
    url,
    created_at: v.i64("time").and_then(from_unix),
    unread: None,
    raw: Some(v.clone()),
  }
}

// ── server-rendered pages ───────────────────────────────────────────────

/// Rename camelCase object keys to snake_case, recursively.
pub fn snake_keys(v: Value) -> Value {
  match v {
    Value::Object(map) => Value::Object(
      map
        .into_iter()
        .map(|(k, v)| (snake(&k), snake_keys(v)))
        .collect(),
    ),
    Value::Array(items) => Value::Array(items.into_iter().map(snake_keys).collect()),
    other => other,
  }
}

fn snake(key: &str) -> String {
  let mut out = String::with_capacity(key.len() + 4);
  for c in key.chars() {
    if c.is_ascii_uppercase() {
      out.push('_');
      out.push(c.to_ascii_lowercase());
    } else {
      out.push(c);
    }
  }
  out
}
