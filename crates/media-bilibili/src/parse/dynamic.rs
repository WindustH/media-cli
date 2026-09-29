//! Dynamics (`/x/polymer/web-dynamic/v1/*`): text, image, video, forward, article ...

use media_core::{Media, Post, Value, ValueExt, json};

use super::{https, secs, user, video};
use crate::refs::DYNAMIC_URL;

/// A dynamic as a post; forwards carry the original in `quoted`.
pub fn dynamic(v: &Value) -> Post {
  let modules = v.at("modules");
  let body = modules.at("module_dynamic");
  let major = body.at("major");
  let stat = modules.at("module_stat");
  let id = v.str("id_str").unwrap_or_default();
  let mut p = Post {
    url: (!id.is_empty()).then(|| format!("{DYNAMIC_URL}{id}")),
    id,
    kind: "dynamic".into(),
    title: major
      .first_str(&[
        "opus.title",
        "archive.title",
        "article.title",
        "pgc.title",
        "ugc_season.title",
        "common.title",
      ])
      .filter(|t| !t.is_empty()),
    text: body
      .first_str(&[
        "desc.text",
        "major.opus.summary.text",
        "major.archive.desc",
        "major.article.desc",
      ])
      .map(|t| t.trim().to_owned())
      .filter(|t| !t.is_empty()),
    author: Some(user(modules.at("module_author"))).filter(|u| !u.id.is_empty()),
    created_at: secs(modules, &["module_author.pub_ts"]),
    tags: body.str("topic.name").into_iter().collect(),
    raw: Some(v.clone()),
    ..Post::default()
  };
  p.metrics.likes = stat.count("like.count");
  p.metrics.comments = stat.count("comment.count");
  p.metrics.shares = stat.count("forward.count");
  let images = major
    .list("draw.items")
    .iter()
    .map(|i| (i, "src"))
    .chain(major.list("opus.pics").iter().map(|i| (i, "url")));
  for (img, key) in images {
    if let Some(src) = img.str(key) {
      p.media.push(Media {
        width: img.u64("width").map(|w| w as u32),
        height: img.u64("height").map(|h| h as u32),
        ..Media::image(https(src))
      });
    }
  }
  if let Some(cover) = major.first_str(&["archive.cover", "article.covers.0"]) {
    p.media.push(Media {
      alt: Some("cover".into()),
      ..Media::image(https(cover))
    });
  }
  if let Some(orig) = Some(v.at("orig")).filter(|o| o.str("id_str").is_some()) {
    p.quoted = Some(Box::new(dynamic(orig)));
  }
  if let Some(kind) = v.str("type") {
    let kind = kind
      .trim_start_matches("DYNAMIC_TYPE_")
      .to_ascii_lowercase();
    p.extra.insert("type".into(), json!(kind));
  }
  if let Some(bvid) = major.str("archive.bvid") {
    p.extra.insert("bvid".into(), json!(bvid));
  }
  if modules.str("module_tag.text").as_deref() == Some("置顶") {
    p.extra.insert("pinned".into(), json!(true));
  }
  p
}

/// The video a video dynamic announces, with the dynamic's author, time and counters.
pub fn dynamic_video(v: &Value) -> Option<Post> {
  let modules = v.at("modules");
  let archive = modules.at("module_dynamic.major.archive");
  archive.str("bvid")?;
  let mut p = video(archive);
  p.author = Some(user(modules.at("module_author"))).filter(|u| !u.id.is_empty());
  p.created_at = secs(modules, &["module_author.pub_ts"]);
  p.metrics.likes = modules.count("module_stat.like.count");
  p.metrics.comments = modules.count("module_stat.comment.count");
  p.raw = Some(v.clone());
  Some(p)
}

/// One dynamic from the desktop detail endpoint (the one anonymous visitors
/// get reliably). It lists modules as an array under other names; this
/// rebuilds the web layout so [`dynamic`] and the comment lookup read both.
pub fn from_desktop(item: &Value) -> Value {
  let (mut author, mut desc, mut stat) = (Value::Null, Value::Null, Value::Null);
  let (mut major, mut orig) = (serde_json::Map::new(), Value::Null);
  for m in item.list("modules") {
    let user = m.at("module_author.user");
    if !user.is_null() {
      author = json!({
        "mid": user.at("mid"),
        "name": user.at("name"),
        "face": user.at("face"),
        "official_verify": {"type": user.at("official.type")},
        "pub_ts": m.at("module_author.pub_ts"),
      });
    }
    if let Some(text) = m.str("module_desc.text") {
      desc = json!({ "text": text });
    }
    for (k, v) in m.at("module_dynamic").as_object().into_iter().flatten() {
      let slot = match k.as_str() {
        "dyn_forward" => {
          orig = from_desktop(v.at("item"));
          continue;
        }
        "dyn_archive" => "archive",
        "dyn_draw" => "draw",
        "dyn_article" => "article",
        k if k.starts_with("dyn_") => "common",
        _ => continue,
      };
      major.insert(slot.into(), v.clone());
    }
    if m.at("module_stat").is_object() {
      stat = m.at("module_stat").clone();
    }
  }
  json!({
    "id_str": item.at("id_str"),
    "type": item.at("type"),
    "basic": {
      "comment_id_str": stat.at("comment.comment_id"),
      "comment_type": stat.at("comment.comment_type"),
    },
    "modules": {
      "module_author": author,
      "module_dynamic": {"desc": desc, "major": major},
      "module_stat": stat,
    },
    "orig": orig,
  })
}
