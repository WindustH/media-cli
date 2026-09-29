//! Comment trees: `t1` things with nested `replies` listings; `more` stubs
//! count as replies that were not loaded.

use media_core::{Comment, Value, ValueExt};

use super::{author, body, data, flag, put, time, vote};
use crate::api::WWW;

/// The comments of a listing (`{data: {children}}`) as a tree; `parent` is
/// the author they answer.
pub fn comments(listing: &Value, parent: Option<&str>) -> Vec<Comment> {
  listing
    .list("data.children")
    .iter()
    .filter_map(|t| comment(t, parent))
    .collect()
}

/// A `t1` thing with the replies loaded under it.
pub fn comment(thing: &Value, parent: Option<&str>) -> Option<Comment> {
  let d = data(thing, "t1")?;
  let author = author(d);
  let replies = comments(d.at("replies"), author.as_ref().map(|a| a.name.as_str()));
  let unloaded: u64 = d
    .list("replies.data.children")
    .iter()
    .filter_map(|t| data(t, "more"))
    .map(|m| m.u64("count").unwrap_or(0))
    .sum();
  // The replies are in `replies` already; keep them out of every `raw`.
  let mut raw = d.clone();
  if let Some(o) = raw.as_object_mut() {
    o.remove("replies");
  }
  let mut c = Comment {
    id: d.str("id")?,
    text: body(d, "body").unwrap_or_default(),
    created_at: time(d, "created_utc"),
    likes: d.u64("score"),
    reply_count: Some(replies.len() as u64 + unloaded).filter(|n| *n > 0),
    reply_to: parent.map(str::to_owned),
    replies,
    raw: Some(raw),
    author,
    ..Comment::default()
  };
  let x = &mut c.extra;
  put(x, "score", d.i64("score").map(Value::from));
  put(
    x,
    "url",
    d.str("permalink").map(|p| format!("{WWW}{p}").into()),
  );
  put(x, "op", flag(d, "is_submitter"));
  put(x, "distinguished", d.str("distinguished").map(Value::from));
  put(x, "vote", vote(d));
  for key in ["stickied", "saved", "locked"] {
    put(x, key, flag(d, key));
  }
  if unloaded > 0 {
    put(x, "more", Some(unloaded.into()));
  }
  // Comments listed outside their thread (a user's comments) name their post.
  if d.str("link_title").is_some() {
    put(x, "subreddit", d.str("subreddit").map(Value::from));
    put(x, "post_title", d.str("link_title").map(Value::from));
    put(
      x,
      "post",
      d.str("link_id")
        .map(|l| l.trim_start_matches("t3_").to_owned().into()),
    );
  }
  Some(c)
}
