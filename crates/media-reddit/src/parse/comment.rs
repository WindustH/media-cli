//! `t1` comments. Threads are assembled by [`crate::thread`]; this maps one
//! comment without its replies.

use media_core::{Comment, Value, ValueExt};

use super::{author, body, data, flag, put, time, vote};
use crate::api::WWW;

/// A `t1` thing listed on its own (a user's comments); `parent` is the
/// author it answers.
pub fn comment(thing: &Value, parent: Option<&str>) -> Option<Comment> {
  data(thing, "t1").and_then(|d| comment_node(d, parent))
}

/// The `data` of a `t1` thing, without replies.
pub fn comment_node(d: &Value, parent: Option<&str>) -> Option<Comment> {
  // Replies are nested by the thread; keep them out of every `raw`.
  let mut raw = d.clone();
  if let Some(o) = raw.as_object_mut() {
    o.remove("replies");
  }
  let mut c = Comment {
    id: d.str("id")?,
    text: body(d, "body").unwrap_or_default(),
    created_at: time(d, "created_utc"),
    likes: d.u64("score"),
    reply_to: parent.map(str::to_owned),
    raw: Some(raw),
    author: author(d),
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
  put(
    x,
    "awards",
    d.u64("total_awards_received")
      .filter(|n| *n > 0)
      .map(Value::from),
  );
  put(
    x,
    "controversial",
    (d.u64("controversiality") == Some(1)).then_some(Value::Bool(true)),
  );
  put(
    x,
    "edited_at",
    time(d, "edited").map(|t| t.to_string().into()),
  );
  for key in ["stickied", "saved", "locked"] {
    put(x, key, flag(d, key));
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
