//! Content objects: answers, questions, articles and pins.

use std::sync::LazyLock;

use media_core::text::{html_to_text, parse_count};
use media_core::{Media, Metrics, Post, Value, ValueExt};
use regex::Regex;

use super::{author, text, time};
use crate::api::WWW;
use crate::refs::{answer_url, article_url, question_url};

/// Any supported content object (`answer`, `article`, `question`, `pin`).
pub fn post(v: &Value) -> Option<Post> {
  match v.str("type")?.as_str() {
    "answer" => Some(answer(v)),
    "article" => Some(article(v)),
    "question" => Some(question(v)),
    "pin" => Some(pin(v)),
    _ => None,
  }
}

fn base(v: &Value, kind: &str) -> Post {
  Post {
    id: v.str("id").unwrap_or_default(),
    kind: kind.into(),
    author: author(v),
    created_at: time(v, &["created_time", "created"]),
    updated_at: time(v, &["updated_time", "updated"]),
    raw: Some(v.clone()),
    ..Post::default()
  }
}

pub fn answer(v: &Value) -> Post {
  let mut p = base(v, "answer");
  let html = v.first_str(&["content", "excerpt"]).unwrap_or_default();
  let question = v.str("question.id");
  p.title = v
    .first_str(&["question.title", "question.name"])
    .map(|t| html_to_text(&t));
  p.url = Some(answer_url(&p.id, question.as_deref()));
  p.text = text(&html);
  p.media = images(&html);
  p.metrics = Metrics {
    likes: v.count("voteup_count"),
    comments: v.count("comment_count"),
    favorites: v.count("favlists_count"),
    ..Metrics::default()
  };
  other(&mut p.metrics, v, &[("thanks", "thanks_count")]);
  if let Some(q) = question {
    p.extra.insert("question_id".into(), q.into());
  }
  p
}

pub fn question(v: &Value) -> Post {
  let mut p = base(v, "question");
  p.title = v.first_str(&["title", "name"]).map(|t| html_to_text(&t));
  p.url = Some(question_url(&p.id));
  let html = v.first_str(&["detail", "excerpt"]).unwrap_or_default();
  p.text = text(&html);
  p.media = images(&html);
  p.tags = topics(v);
  p.metrics = Metrics {
    views: v.count("visit_count"),
    comments: v.count("comment_count"),
    ..Metrics::default()
  };
  other(
    &mut p.metrics,
    v,
    &[("answers", "answer_count"), ("followers", "follower_count")],
  );
  p
}

pub fn article(v: &Value) -> Post {
  let mut p = base(v, "article");
  p.title = v.str("title").map(|t| html_to_text(&t));
  p.url = Some(article_url(&p.id));
  let html = v.first_str(&["content", "excerpt"]).unwrap_or_default();
  p.text = text(&html);
  p.media = v
    .str("image_url")
    .map(Media::image)
    .into_iter()
    .chain(images(&html))
    .collect();
  p.tags = topics(v);
  p.metrics = Metrics {
    likes: v.count("voteup_count"),
    comments: v.count("comment_count"),
    favorites: v.count("favlists_count"),
    ..Metrics::default()
  };
  if let Some(column) = v.str("column.title") {
    p.extra.insert("column".into(), column.into());
  }
  p
}

/// A pin (想法): text, images and links in `content`; a repin quotes its origin.
pub fn pin(v: &Value) -> Post {
  let mut p = base(v, "pin");
  p.title = v.str("title").filter(|t| !t.trim().is_empty());
  p.url = Some(format!("{WWW}/pin/{}", p.id));
  let mut html = String::new();
  for item in v.list("content") {
    match item.str("type").as_deref() {
      Some("text") => html.push_str(&item.str("content").unwrap_or_default()),
      Some("image") => {
        if let Some(url) = item.first_str(&["original_url", "url"]) {
          let mut m = Media::image(url);
          m.width = item.u64("width").map(|w| w as u32);
          m.height = item.u64("height").map(|h| h as u32);
          p.media.push(m);
        }
      }
      Some("link") => {
        if let Some(url) = item.str("url") {
          html.push_str(&format!("<p>{url}</p>"));
        }
      }
      _ => {}
    }
  }
  if html.is_empty() {
    html = v
      .first_str(&["content_html", "excerpt_title"])
      .unwrap_or_default();
  }
  p.text = text(&html);
  p.tags = topics(v);
  p.metrics = Metrics {
    likes: v.first_count(&["reaction_count", "like_count"]),
    comments: v.count("comment_count"),
    shares: v.count("repin_count"),
    favorites: v.count("favlists_count"),
    ..Metrics::default()
  };
  if v.at("origin_pin").is_object() {
    p.quoted = Some(Box::new(pin(v.at("origin_pin"))));
  }
  p
}

/// An entry of the hot list: the question with its heat (`1874 万热度`).
pub fn hot_item(v: &Value) -> Option<Post> {
  let mut p = post(v.at("target"))?;
  if let Some(heat) = v.str("detail_text") {
    if let Some(n) = parse_count(heat.trim_end_matches("热度")) {
      p.metrics.other.insert("heat".into(), n);
    }
    p.extra.insert("heat".into(), heat.into());
  }
  Some(p)
}

fn topics(v: &Value) -> Vec<String> {
  v.list("topics")
    .iter()
    .filter_map(|t| t.str("name"))
    .collect()
}

fn other(m: &mut Metrics, v: &Value, fields: &[(&str, &str)]) {
  for (key, field) in fields {
    if let Some(n) = v.count(field) {
      m.other.insert((*key).into(), n);
    }
  }
}

/// Images embedded in a body, preferring the original resolution.
fn images(html: &str) -> Vec<Media> {
  static IMG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<img\b[^>]*>").unwrap());
  static ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"([\w-]+)\s*=\s*"([^"]*)""#).unwrap());
  let mut out: Vec<Media> = Vec::new();
  for tag in IMG.find_iter(html) {
    let attr = |name: &str| {
      ATTR
        .captures_iter(tag.as_str())
        .find(|c| &c[1] == name)
        .map(|c| c[2].replace("&amp;", "&"))
    };
    let Some(url) = ["data-original", "data-actualsrc", "src"]
      .iter()
      .filter_map(|a| attr(a))
      .find(|u| u.starts_with("http") && !u.contains("/equation?"))
    else {
      continue;
    };
    if out.iter().any(|m| m.url == url) {
      continue;
    }
    let mut m = Media::image(url);
    m.width = attr("data-rawwidth").and_then(|w| w.parse().ok());
    m.height = attr("data-rawheight").and_then(|h| h.parse().ok());
    out.push(m);
  }
  out
}
