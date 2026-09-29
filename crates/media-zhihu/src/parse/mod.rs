//! Zhihu JSON → core models.
//!
//! Bodies arrive as HTML (answers, articles, question details, comments) and
//! are rendered with `html_to_text`; images inside them become `media`.

mod post;
mod social;

use media_core::text::{from_unix, html_to_text};
use media_core::{User, UserStats, Value, ValueExt};

use crate::refs::people_url;

pub use post::{answer, article, hot_item, pin, post, question};
pub use social::{comment, folder, notification, topic};

pub fn user(v: &Value) -> User {
  let token = v.str("url_token");
  let mut u = User {
    id: v.str("id").unwrap_or_default(),
    name: v.str("name").map(|n| html_to_text(&n)).unwrap_or_default(),
    url: token.as_deref().map(people_url),
    handle: token,
    avatar: v.str("avatar_url"),
    bio: v
      .str("headline")
      .map(|h| html_to_text(&h))
      .filter(|h| !h.is_empty()),
    verified: !v.list("badge").is_empty() || v.str("badge_v2.title").is_some(),
    location: v.str("locations.0.name"),
    stats: UserStats {
      followers: v.count("follower_count"),
      following: v.count("following_count"),
      posts: v.count("answer_count"),
      likes: v.count("voteup_count"),
      ..UserStats::default()
    },
    followed: v.bool("is_following"),
    raw: Some(v.clone()),
    ..User::default()
  };
  for (key, field) in [
    ("articles", "articles_count"),
    ("pins", "pins_count"),
    ("questions", "question_count"),
    ("thanked", "thanked_count"),
    ("favorited", "favorited_count"),
  ] {
    if let Some(n) = v.count(field) {
      u.stats.other.insert(key.into(), n);
    }
  }
  if let Some(d) = v.str("description").map(|d| html_to_text(&d)) {
    u.extra.insert("description".into(), d.into());
  }
  if let Some(g) = v.i64("gender").filter(|g| *g >= 0) {
    let gender = if g == 1 { "male" } else { "female" };
    u.extra.insert("gender".into(), gender.into());
  }
  if v.bool("is_org") == Some(true) {
    u.extra.insert("is_org".into(), true.into());
  }
  u
}

/// Author of a post or comment; anonymous authors keep only their name.
fn author(v: &Value) -> Option<User> {
  let v = [v.at("author.member"), v.at("author")]
    .into_iter()
    .find(|a| a.str("name").is_some())?;
  let mut u = user(v);
  if u.id == "0" || u.id.is_empty() {
    u.url = None;
    u.handle = None;
  }
  Some(u)
}

/// Rendered text of an HTML body, `None` when empty.
fn text(html: &str) -> Option<String> {
  Some(html_to_text(html)).filter(|t| !t.is_empty())
}

/// The first of several unix-time fields.
fn time(v: &Value, keys: &[&str]) -> Option<jiff::Timestamp> {
  keys.iter().find_map(|k| v.i64(k)).and_then(from_unix)
}
