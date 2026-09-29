//! Videos in their many shapes: `view`, popular / ranking / related lists,
//! search results, space uploads, favorites, history and watch-later items.

use media_core::{Media, Metrics, Post, Value, ValueExt, json};

use super::{author, https, plain, secs};
use crate::refs::Video;

/// Seconds from a number or a `m:ss` / `h:mm:ss` string.
pub fn duration(v: &Value, paths: &[&str]) -> Option<u64> {
  paths.iter().find_map(|p| match v.at(p) {
    Value::String(s) if s.contains(':') => s.split(':').try_fold(0u64, |acc, part| {
      Some(acc * 60 + part.trim().parse::<u64>().ok()?)
    }),
    _ => v.u64(p),
  })
}

fn metrics(v: &Value) -> Metrics {
  let mut m = Metrics {
    views: v.first_count(&["stat.view", "stat.play", "play", "cnt_info.play"]),
    likes: v.first_count(&["stat.like", "like", "cnt_info.thumb_up"]),
    comments: v.first_count(&["stat.reply", "comment", "review"]),
    shares: v.first_count(&["stat.share", "cnt_info.share"]),
    favorites: v.first_count(&["stat.favorite", "favorites", "cnt_info.collect"]),
    ..Metrics::default()
  };
  if let Some(n) = v.first_count(&["stat.coin", "cnt_info.coin"]) {
    m.other.insert("coins".into(), n);
  }
  if let Some(n) = v.first_count(&["stat.danmaku", "video_review", "cnt_info.danmaku"]) {
    m.other.insert("danmaku".into(), n);
  }
  m
}

/// A video from any listing or from `/x/web-interface/view`.
pub fn video(v: &Value) -> Post {
  let ids = v
    .first_str(&["bvid", "history.bvid"])
    .and_then(|b| Video::from_bvid(&b))
    .or_else(|| {
      v.first_str(&["aid", "history.oid", "id"])
        .and_then(|a| a.parse().ok())
        .map(Video::from_aid)
    });
  let mut p = Post {
    kind: "video".into(),
    title: plain(v, &["title"]),
    text: v
      .first_str(&["desc", "description", "intro"])
      .map(|s| s.trim().to_owned())
      .filter(|s| !s.is_empty() && s != "-"),
    author: author(
      v,
      &["owner.mid", "upper.mid", "author_mid", "mid"],
      &["owner.name", "upper.name", "author_name", "author"],
      &["owner.face", "upper.face", "author_face", "upic"],
    ),
    created_at: secs(v, &["pubdate", "created", "pubtime", "ctime"]),
    metrics: metrics(v),
    tags: v
      .str("tag")
      .map(|t| {
        t.split(',')
          .filter(|s| !s.is_empty())
          .map(String::from)
          .collect()
      })
      .unwrap_or_default(),
    raw: Some(v.clone()),
    ..Post::default()
  };
  if let Some(cover) = v.first_str(&["pic", "cover"]) {
    p.media.push(Media {
      alt: Some("cover".into()),
      ..Media::image(https(cover))
    });
  }
  if let Some(ids) = ids {
    p.id = ids.bvid.clone();
    p.url = Some(ids.url());
    p.extra.insert("bvid".into(), json!(ids.bvid));
    p.extra.insert("aid".into(), json!(ids.aid));
  }
  if let Some(cid) = ["cid", "history.cid"]
    .iter()
    .find_map(|p| v.u64(p))
    .filter(|c| *c > 0)
  {
    p.extra.insert("cid".into(), json!(cid));
  }
  if let Some(d) = duration(v, &["duration", "length", "duration_text"]) {
    p.extra.insert("duration".into(), json!(d));
  }
  if let Some(t) = v.first_str(&["tname", "typename", "tag_name"]) {
    p.extra.insert("tname".into(), json!(t));
  }
  let pages = v.list("pages").len();
  if pages > 1 {
    p.extra.insert("pages".into(), json!(pages));
  }
  p
}
