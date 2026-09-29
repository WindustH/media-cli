//! Reddit JSON → core models. Things arrive as `{kind, data}`: `t1`
//! comments, `t2` accounts, `t3` posts, `t4` messages, `t5` subreddits.

mod comment;
mod media;

use media_core::text::{from_secs, html_to_text};
use media_core::{Collection, Metrics, Notification, Post, User, UserStats, Value, ValueExt};

use crate::api::WWW;
use crate::refs::{post_url, sub_url, user_url};

pub use comment::{comment, comment_node};
pub use media::reddit_video;

/// The `data` of a thing of this kind.
pub fn data<'a>(thing: &'a Value, kind: &str) -> Option<&'a Value> {
  (thing.str("kind").as_deref() == Some(kind)).then(|| thing.at("data"))
}

fn time(d: &Value, key: &str) -> Option<jiff::Timestamp> {
  d.i64(key).and_then(from_secs)
}

/// Rendered text of a post (`selftext`) or comment (`body`).
fn body(d: &Value, key: &str) -> Option<String> {
  d.str(&format!("{key}_html"))
    .map(|html| html_to_text(&html))
    .or_else(|| d.str(key))
    .filter(|t| !t.is_empty())
}

fn put(extra: &mut media_core::Extra, key: &str, value: Option<Value>) {
  if let Some(v) = value.filter(|v| !v.is_null()) {
    extra.insert(key.into(), v);
  }
}

/// Only `true` flags are worth showing.
fn flag(d: &Value, key: &str) -> Option<Value> {
  (d.bool(key) == Some(true)).then_some(Value::Bool(true))
}

/// The author of a post or comment (`None` for deleted accounts).
fn author(d: &Value) -> Option<User> {
  let name = d.str("author").filter(|n| n != "[deleted]")?;
  Some(User {
    id: d.str("author_fullname").unwrap_or_else(|| name.clone()),
    url: Some(user_url(&name)),
    handle: Some(name.clone()),
    name,
    ..User::default()
  })
}

/// `likes` is the logged-in account's vote: true, false or null.
fn vote(d: &Value) -> Option<Value> {
  d.bool("likes")
    .map(|up| Value::from(if up { 1 } else { -1 }))
}

/// A `t3` post.
pub fn post(d: &Value) -> Option<Post> {
  post_at(d, 0)
}

fn post_at(d: &Value, depth: usize) -> Option<Post> {
  let id = d.str("id")?;
  let sub = d.str("subreddit").unwrap_or_default();
  let quoted = match depth {
    0 => d.list("crosspost_parent_list").first(),
    _ => None,
  };
  let mut post = Post {
    kind: "post".into(),
    title: d.str("title"),
    text: body(d, "selftext"),
    url: Some(post_url(&sub, &id)),
    author: author(d),
    created_at: time(d, "created_utc"),
    // `edited` is `false` or the time of the last edit.
    updated_at: time(d, "edited"),
    metrics: metrics(d),
    media: media::of(d),
    quoted: quoted.and_then(|q| post_at(q, depth + 1)).map(Box::new),
    raw: Some(d.clone()),
    id,
    ..Post::default()
  };
  let x = &mut post.extra;
  put(x, "subreddit", d.str("subreddit").map(Value::from));
  put(x, "flair", d.str("link_flair_text").map(Value::from));
  put(x, "score", d.i64("score").map(Value::from));
  put(x, "upvote_ratio", d.f64("upvote_ratio").map(Value::from));
  put(
    x,
    "subreddit_subscribers",
    d.u64("subreddit_subscribers").map(Value::from),
  );
  put(x, "nsfw", Some(d.bool("over_18").unwrap_or(false).into()));
  // `deleted` (by the author), `moderator`, `reddit`, `automod_filtered` ...
  put(x, "removed", d.str("removed_by_category").map(Value::from));
  put(x, "link", link(d).map(Value::from));
  put(x, "vote", vote(d));
  for key in ["spoiler", "stickied", "locked", "archived", "saved"] {
    put(x, key, flag(d, key));
  }
  Some(post)
}

/// Every counter of a post. `likes` is the score (upvotes minus downvotes;
/// `ups` carries the same number and `downs` is always 0); the author's own
/// views and shares are in `insights`.
fn metrics(d: &Value) -> Metrics {
  let mut m = Metrics {
    views: d.count("view_count"),
    likes: d.u64("score"),
    comments: d.count("num_comments"),
    ..Metrics::default()
  };
  // `gilded` (legacy gold) and `num_reports` (moderators only) are mostly empty.
  let counters = [
    ("crossposts", "num_crossposts", false),
    ("awards", "total_awards_received", false),
    ("gilded", "gilded", true),
    ("reports", "num_reports", true),
  ];
  for (name, key, skip_zero) in counters {
    if let Some(n) = d.u64(key).filter(|n| *n > 0 || !skip_zero) {
      m.other.insert(name.into(), n);
    }
  }
  m
}

/// The linked page of a link post (not Reddit's own media or the post itself).
fn link(d: &Value) -> Option<String> {
  if d.bool("is_self") == Some(true) {
    return None;
  }
  let url = d.first_str(&["url_overridden_by_dest", "url"])?;
  let host = url::Url::parse(&url).ok()?.host_str()?.to_owned();
  let own = ["i.redd.it", "v.redd.it", "www.reddit.com", "reddit.com"];
  (!own.contains(&host.as_str())).then_some(url)
}

/// A `t2` account (`/user/<name>/about`, `/api/me`, user search).
pub fn user(d: &Value) -> Option<User> {
  let name = d.str("name")?;
  let profile = d.at("subreddit");
  let mut stats = UserStats {
    followers: profile.count("subscribers"),
    likes: d.count("total_karma"),
    ..UserStats::default()
  };
  for key in [
    "link_karma",
    "comment_karma",
    "awarder_karma",
    "awardee_karma",
  ] {
    if let Some(n) = d.count(key) {
      stats.other.insert(key.into(), n);
    }
  }
  let mut user = User {
    id: d
      .str("id")
      .map(|id| format!("t2_{id}"))
      .unwrap_or_else(|| name.clone()),
    name: profile.str("title").unwrap_or_else(|| name.clone()),
    url: Some(user_url(&name)),
    avatar: d
      .first_str(&["icon_img", "snoovatar_img"])
      .or_else(|| profile.str("icon_img")),
    bio: profile.str("public_description"),
    stats,
    followed: profile.bool("user_is_subscriber"),
    created_at: time(d, "created_utc"),
    raw: Some(d.clone()),
    handle: Some(name),
    ..User::default()
  };
  let x = &mut user.extra;
  put(x, "banner", profile.str("banner_img").map(Value::from));
  for key in ["is_mod", "is_gold", "is_employee", "is_suspended"] {
    put(x, key, flag(d, key));
  }
  put(x, "nsfw", flag(profile, "over_18"));
  put(x, "inbox_count", d.u64("inbox_count").map(Value::from));
  Some(user)
}

/// A `t5` subreddit as a collection (`kind` is `user` for profile subreddits).
pub fn subreddit(d: &Value) -> Option<Collection> {
  let name = d.str("display_name")?;
  let is_user = d.str("subreddit_type").as_deref() == Some("user");
  let mut c = Collection {
    kind: if is_user { "user" } else { "subreddit" }.into(),
    name: d
      .str("display_name_prefixed")
      .unwrap_or_else(|| name.clone()),
    description: d
      .first_str(&["public_description", "title"])
      .map(|t| t.trim().to_owned()),
    url: Some(
      d.str("url")
        .map(|u| format!("{WWW}{u}"))
        .unwrap_or_else(|| sub_url(&name)),
    ),
    followers: d.count("subscribers"),
    raw: Some(d.clone()),
    id: name,
    ..Collection::default()
  };
  let x = &mut c.extra;
  put(x, "title", d.str("title").map(Value::from));
  put(x, "fullname", d.str("name").map(Value::from));
  put(
    x,
    "active",
    d.first_count(&["active_user_count", "accounts_active"])
      .map(Value::from),
  );
  put(
    x,
    "created_at",
    time(d, "created_utc").map(|t| t.to_string().into()),
  );
  put(
    x,
    "icon",
    d.first_str(&["community_icon", "icon_img"])
      .map(|u| u.split('?').next().unwrap_or_default().to_owned())
      .filter(|u| !u.is_empty())
      .map(Value::from),
  );
  put(x, "nsfw", flag(d, "over18"));
  put(x, "subscribed", flag(d, "user_is_subscriber"));
  Some(c)
}

/// An inbox item: `t1` (comment reply, post reply, mention) or `t4` message.
pub fn notification(thing: &Value) -> Option<Notification> {
  let d = thing.at("data");
  let kind = match (thing.str("kind")?.as_str(), d.str("type").as_deref()) {
    ("t4", _) => "message",
    (_, Some("comment_reply")) => "reply",
    (_, Some("post_reply")) => "comment",
    (_, Some("username_mention")) => "mention",
    _ => "other",
  };
  Some(Notification {
    id: d.str("name")?,
    kind: kind.into(),
    text: body(d, "body").unwrap_or_default(),
    actor: author(d),
    target: d.first_str(&["link_title", "subject"]),
    url: d
      .str("context")
      .filter(|c| !c.is_empty())
      .map(|c| format!("{WWW}{c}")),
    created_at: time(d, "created_utc"),
    unread: d.bool("new"),
    raw: Some(d.clone()),
  })
}
