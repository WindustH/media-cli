//! Upstream JSON -> core models.
//!
//! Bilibili serves the same entities in several shapes (view, search, space,
//! favorites, history ...); the mappers read each field from the list of
//! paths it can appear under instead of keeping one mapper per endpoint.

mod dynamic;
pub mod insights;
mod video;

pub use dynamic::{dynamic, dynamic_video, forward, from_desktop};
pub use video::video;

use media_core::text::{from_secs, html_to_text};
use media_core::{Collection, Comment, Notification, User, Value, ValueExt, json};

use crate::refs;

/// Protocol-relative / plain-http image links -> https.
pub fn https(url: String) -> String {
  if let Some(rest) = url.strip_prefix("//") {
    format!("https://{rest}")
  } else if let Some(rest) = url.strip_prefix("http://") {
    format!("https://{rest}")
  } else {
    url
  }
}

/// Plain text of a field that may carry `<em class="keyword">` highlights or entities.
pub fn plain(v: &Value, paths: &[&str]) -> Option<String> {
  v.first_str(paths)
    .map(|s| html_to_text(&s))
    .filter(|s| !s.is_empty())
}

fn secs(v: &Value, paths: &[&str]) -> Option<jiff::Timestamp> {
  paths.iter().find_map(|p| v.i64(p)).and_then(from_secs)
}

/// A user from any of the upstream shapes (card, space, search, member, author ...).
pub fn user(v: &Value) -> User {
  let id = v.first_str(&["mid", "uid"]).unwrap_or_default();
  let verified = ["official_verify.type", "Official.type", "official.type"]
    .iter()
    .find_map(|p| v.i64(p))
    .is_some_and(|t| t == 0 || t == 1);
  let mut u = User {
    url: (!id.is_empty()).then(|| refs::space_url(&id)),
    id,
    name: v
      .first_str(&["name", "uname", "nickname"])
      .unwrap_or_default(),
    avatar: v.first_str(&["face", "upic", "avatar"]).map(https),
    bio: v
      .first_str(&["sign", "usign"])
      .map(|s| s.trim().to_owned())
      .filter(|s| !s.is_empty()),
    verified,
    raw: Some(v.clone()),
    ..User::default()
  };
  u.stats.followers = v.first_count(&["fans", "follower"]);
  u.stats.following = v.count("attention");
  u.stats.posts = v.first_count(&["videos", "archive_count"]);
  if let Some(level) = v.first_count(&["level_info.current_level", "level"]) {
    u.stats.other.insert("level".into(), level);
  }
  if let Some(title) = v
    .first_str(&["official_verify.desc", "Official.title", "official.title"])
    .filter(|t| !t.is_empty())
  {
    u.extra.insert("official".into(), json!(title));
  }
  u
}

/// A user from separate id / name / avatar fields (videos, history ...).
pub fn author(v: &Value, mid: &[&str], name: &[&str], face: &[&str]) -> Option<User> {
  let id = v.first_str(mid)?;
  Some(User {
    url: Some(refs::space_url(&id)),
    id,
    name: v.first_str(name).unwrap_or_default(),
    avatar: v.first_str(face).map(https),
    ..User::default()
  })
}

/// A reply (`/x/v2/reply*`), including its preview sub-replies.
pub fn comment(v: &Value) -> Comment {
  let text = v.str("content.message").unwrap_or_default();
  let root = v.str("root_str").filter(|r| r != "0");
  let parent = v.str("parent_str").filter(|p| p != "0");
  // Inside a thread, answers to another reply start with `回复 @name :`.
  let reply_to = (parent.is_some() && parent != root)
    .then(|| text.strip_prefix("回复 @"))
    .flatten()
    .and_then(|rest| rest.split_once(':'))
    .map(|(name, _)| name.trim().to_owned());
  let mut c = Comment {
    id: v.first_str(&["rpid_str", "rpid"]).unwrap_or_default(),
    author: Some(user(v.at("member"))).filter(|u| !u.id.is_empty()),
    text,
    created_at: secs(v, &["ctime"]),
    likes: v.count("like"),
    reply_count: v.count("rcount"),
    reply_to,
    location: v
      .str("reply_control.location")
      .map(|l| l.trim_start_matches("IP属地：").to_owned()),
    replies: v.list("replies").iter().map(comment).collect(),
    raw: Some(v.clone()),
    ..Comment::default()
  };
  if let Some(root) = root {
    c.extra.insert("root".into(), json!(root));
  }
  if let Some(parent) = parent {
    c.extra.insert("parent".into(), json!(parent));
  }
  // The uploader's own reactions to the comment.
  for (key, name) in [
    ("up_action.like", "up_liked"),
    ("up_action.reply", "up_replied"),
  ] {
    if v.bool(key) == Some(true) {
      c.extra.insert(name.into(), json!(true));
    }
  }
  c
}

/// A favorites folder (`/x/v3/fav/folder/created/list-all`) of user `owner`.
pub fn folder(v: &Value, owner: &str) -> Collection {
  let id = v.str("id").unwrap_or_default();
  let mid = v.str("mid").unwrap_or_else(|| owner.to_owned());
  let attr = v.u64("attr").unwrap_or(0);
  let mut c = Collection {
    url: Some(format!("{}/favlist?fid={id}", refs::space_url(&mid))),
    id,
    kind: "folder".into(),
    name: v.str("title").unwrap_or_default(),
    description: v.str("intro"),
    items: v.count("media_count"),
    raw: Some(v.clone()),
    ..Collection::default()
  };
  c.extra.insert("private".into(), json!(attr & 1 == 1));
  c.extra.insert("default".into(), json!(attr & 2 == 0));
  c
}

/// One entry of `/x/msgfeed/{reply,at,like}`; `kind` is `reply`, `mention` or `like`.
pub fn notification(v: &Value, kind: &str) -> Notification {
  let actor = if kind == "like" {
    v.list("users").first()
  } else {
    Some(v.at("user"))
  }
  .map(user)
  .filter(|u| !u.id.is_empty());
  let text = match kind {
    "like" => {
      let n = v.u64("counts").unwrap_or(1);
      let what = v.str("item.business").unwrap_or_default();
      if n > 1 {
        format!("{n} people liked your {what}")
      } else {
        format!("liked your {what}")
      }
    }
    _ => v.str("item.source_content").unwrap_or_default(),
  };
  Notification {
    id: v.str("id").unwrap_or_default(),
    kind: kind.into(),
    text,
    actor,
    target: v
      .first_str(&["item.title", "item.target_reply_content", "item.desc"])
      .filter(|t| !t.is_empty()),
    url: v.first_str(&["item.uri"]).map(https),
    created_at: secs(v, &["reply_time", "at_time", "like_time"]),
    unread: None,
    raw: Some(v.clone()),
  }
}
