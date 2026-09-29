//! Replies shown as comments (`TweetDetail`): one comment per conversation
//! thread, the rest of the thread nested under the reply it answers. Pages
//! follow the bottom cursor, then the "show more replies" cursor that holds
//! the low-ranked threads.

use media_core::{Comment, Page, PageReq, Post, Result, ValueExt, json};

use crate::api::Api;
use crate::graphql::TWEET_DETAIL;
use crate::parse;
use crate::refs;
use crate::timeline::{Timeline, is_promoted};

const CONVERSATION: &[&str] = &[
  "data.threaded_conversation_with_injections_v2.instructions",
  "data.tweetResult.result.timeline.instructions",
];

/// Replies of a tweet. `sort`: relevance, recency or likes.
pub async fn comments(
  api: &Api,
  arg: &str,
  sort: Option<&str>,
  req: &PageReq,
) -> Result<Page<Comment>> {
  conversation(api, &refs::tweet_id(arg)?, sort, req).await
}

/// Replies under one reply: the threads of its own conversation page, which
/// start with its direct replies.
pub async fn replies(api: &Api, comment: &str, req: &PageReq) -> Result<Page<Comment>> {
  conversation(api, &refs::tweet_id(comment)?, None, req).await
}

async fn conversation(
  api: &Api,
  id: &str,
  sort: Option<&str>,
  req: &PageReq,
) -> Result<Page<Comment>> {
  let ranking = match sort {
    Some("recency") => "Recency",
    Some("likes") => "Likes",
    _ => "Relevance",
  };
  let mut variables = json!({
    "focalTweetId": id,
    "referrer": "tweet",
    "with_rux_injections": false,
    "includePromotedContent": false,
    "rankingMode": ranking,
    "withCommunity": true,
    "withQuickPromoteEligibilityTweetFields": true,
    "withBirdwatchNotes": true,
    "withVoice": true,
  });
  if let Some(c) = &req.cursor {
    variables["cursor"] = c.as_str().into();
  }
  let data = api.graphql(&TWEET_DETAIL, variables).await?;
  let tl = Timeline::at(&data, CONVERSATION);
  let items: Vec<Comment> = tl
    .entries
    .iter()
    .filter(|e| e.id.starts_with("conversationthread-"))
    .filter_map(|e| {
      let thread = e
        .items
        .iter()
        .filter(|(id, item)| !is_promoted(id, item))
        .filter_map(|(_, item)| parse::post(item.at("tweet_results.result")))
        .map(comment);
      nest(thread)
    })
    // Ads can come as threads too; every real thread starts with a reply.
    .filter(|c| c.extra.contains_key("in_reply_to"))
    .collect();
  // The last page still carries a cursor; the empty page behind it ends the listing.
  let next = tl
    .bottom
    .or(tl.more)
    .filter(|c| Some(c) != req.cursor.as_ref());
  Ok(match next {
    Some(next) if !items.is_empty() => Page::new(items, Some(next)),
    _ => Page::last(items),
  })
}

/// A thread as one comment: every later reply goes under the reply it answers
/// (or under the first one when that is not in the thread).
fn nest(thread: impl Iterator<Item = Comment>) -> Option<Comment> {
  let mut thread = thread;
  let mut root = thread.next()?;
  root.reply_to = None;
  for c in thread {
    let parent = c
      .extra
      .get("in_reply_to")
      .and_then(|v| v.as_str())
      .map(str::to_owned);
    match parent.as_deref().and_then(|p| find(&mut root, p)) {
      Some(slot) => slot.replies.push(c),
      None => root.replies.push(c),
    }
  }
  Some(root)
}

fn find<'a>(c: &'a mut Comment, id: &str) -> Option<&'a mut Comment> {
  if c.id == id {
    return Some(c);
  }
  c.replies.iter_mut().find_map(|r| find(r, id))
}

/// A reply shown as a comment: the leading `@mentions` of the reply are
/// dropped; counters without a comment field go to `extra`.
fn comment(p: Post) -> Comment {
  let mut text = p.text.unwrap_or_default();
  while let Some(rest) = text.strip_prefix('@') {
    match rest.split_once(char::is_whitespace) {
      Some((_, tail)) => text = tail.trim_start().to_owned(),
      None => break,
    }
  }
  let reply_to = p
    .extra
    .get("in_reply_to_user")
    .and_then(|v| v.as_str())
    .map(str::to_owned);
  let mut extra = p.extra;
  extra.insert("url".into(), p.url.unwrap_or_default().into());
  let m = &p.metrics;
  let counters = [
    ("views", m.views),
    ("retweets", m.shares),
    ("quotes", m.other.get("quotes").copied()),
    ("bookmarks", m.favorites),
  ];
  for (key, n) in counters {
    if let Some(n) = n {
      extra.insert(key.into(), n.into());
    }
  }
  if !p.media.is_empty() {
    extra.insert("media".into(), json!(p.media));
  }
  Comment {
    id: p.id,
    author: p.author,
    text,
    created_at: p.created_at,
    likes: p.metrics.likes,
    reply_count: p.metrics.comments,
    reply_to,
    extra,
    raw: p.raw,
    ..Comment::default()
  }
}
