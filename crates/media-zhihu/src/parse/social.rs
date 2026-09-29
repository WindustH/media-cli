//! Comments, favorites folders, topics and notifications.

use media_core::text::html_to_text;
use media_core::{Collection, Comment, Notification, User, Value, ValueExt};

use super::{author, time, user};
use crate::api::WWW;
use crate::refs::people_url;

/// A comment_v5 comment with its inline child comments.
pub fn comment(v: &Value) -> Comment {
  let location = v
    .list("comment_tag")
    .iter()
    .find(|t| t.str("type").as_deref() == Some("ip_info"))
    .and_then(|t| t.str("text"))
    .map(|t| t.trim_start_matches("IP 属地").trim().to_owned());
  let mut c = Comment {
    id: v.str("id").unwrap_or_default(),
    author: author(v),
    text: html_to_text(&v.str("content").unwrap_or_default()),
    created_at: time(v, &["created_time"]),
    likes: v.first_count(&["like_count", "vote_count"]),
    reply_count: v.count("child_comment_count"),
    reply_to: v.first_str(&["reply_to_author.name", "reply_to_author.member.name"]),
    location,
    replies: v.list("child_comments").iter().map(comment).collect(),
    raw: Some(v.clone()),
    ..Comment::default()
  };
  for flag in ["hot", "is_author", "is_author_top"] {
    if v.bool(flag) == Some(true) {
      c.extra.insert(flag.into(), true.into());
    }
  }
  c
}

/// A favorites folder (收藏夹).
pub fn folder(v: &Value) -> Collection {
  let id = v.str("id").unwrap_or_default();
  let mut c = Collection {
    url: Some(format!("{WWW}/collection/{id}")),
    id,
    kind: "folder".into(),
    name: v.str("title").unwrap_or_default(),
    description: v
      .str("description")
      .map(|d| html_to_text(&d))
      .filter(|d| !d.is_empty()),
    items: v.first_count(&["item_count", "answer_count"]),
    followers: v.count("follower_count"),
    owner: v.at("creator").is_object().then(|| user(v.at("creator"))),
    raw: Some(v.clone()),
    ..Collection::default()
  };
  for flag in ["is_public", "is_default"] {
    if let Some(b) = v.bool(flag) {
      c.extra
        .insert(flag.trim_start_matches("is_").into(), b.into());
    }
  }
  c
}

pub fn topic(v: &Value) -> Collection {
  let id = v.str("id").unwrap_or_default();
  Collection {
    url: Some(format!("{WWW}/topic/{id}")),
    id,
    kind: "topic".into(),
    name: v.str("name").map(|n| html_to_text(&n)).unwrap_or_default(),
    description: v
      .first_str(&["introduction", "excerpt", "description"])
      .map(|d| html_to_text(&d))
      .filter(|d| !d.is_empty()),
    items: v.count("questions_count"),
    followers: v.first_count(&["followers_count", "follower_count"]),
    raw: Some(v.clone()),
    ..Collection::default()
  }
}

/// An entry of `notifications/v2/recent`: actors, a verb phrase and a target.
pub fn notification(v: &Value) -> Notification {
  let content = v.at("content");
  let actors = content.list("actors");
  let names: Vec<String> = actors.iter().filter_map(|a| a.str("name")).collect();
  let verb = content.str("verb").unwrap_or_default();
  let actor = actors.first().map(|a| {
    let token = a.str("link").and_then(|l| crate::refs::user(&l).ok());
    User {
      id: a.str("id").or_else(|| token.clone()).unwrap_or_default(),
      name: a.str("name").unwrap_or_default(),
      url: token.as_deref().map(people_url),
      handle: token,
      ..User::default()
    }
  });
  Notification {
    id: v.str("id").unwrap_or_default(),
    kind: kind_of(&verb).into(),
    text: format!("{} {verb}", names.join(", ")).trim().to_owned(),
    actor,
    target: content
      .str("target.text")
      .map(|t| html_to_text(&t))
      .filter(|t| !t.is_empty()),
    url: content.str("target.link"),
    created_at: time(v, &["create_time", "created_time"]),
    unread: v.bool("is_read").map(|r| !r),
    raw: Some(v.clone()),
  }
}

/// Coarse notification kind from Zhihu's verb phrase (`赞同了你的回答` ...);
/// the first matching word wins, so `回复了你的评论` is a reply.
fn kind_of(verb: &str) -> &'static str {
  const KINDS: &[(&str, &str)] = &[
    ("提到", "mention"),
    ("@", "mention"),
    ("回复", "reply"),
    ("赞同", "like"),
    ("赞了", "like"),
    ("喜欢", "like"),
    ("感谢", "like"),
    ("收藏", "favorite"),
    ("评论", "comment"),
    ("邀请", "invite"),
    ("关注", "follow"),
  ];
  KINDS
    .iter()
    .find(|(word, _)| verb.contains(word))
    .map(|(_, kind)| *kind)
    .unwrap_or("system")
}
