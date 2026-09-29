//! Reading tweets: one tweet, its replies, search, home feeds, user
//! timelines, likes, bookmarks and lists.

use media_core::{Comment, Error, Page, PageReq, Post, Query, Result, ValueExt, json};

use crate::api::Api;
use crate::graphql::{
  BOOKMARK_FOLDER, BOOKMARKS, HOME, HOME_LATEST, LIKES, LIST_TWEETS, SEARCH, TWEET_DETAIL,
  TWEET_RESULT, USER_TWEETS,
};
use crate::parse;
use crate::refs;
use crate::timeline::{self, Timeline, vars, with};
use crate::users::{self, SEARCH_TIMELINE};

const USER_TIMELINE: &[&str] = &[
  "data.user.result.timeline.timeline.instructions",
  "data.user.result.timeline_v2.timeline.instructions",
];
const CONVERSATION: &[&str] = &[
  "data.threaded_conversation_with_injections_v2.instructions",
  "data.tweetResult.result.timeline.instructions",
];

pub async fn read(api: &Api, arg: &str) -> Result<Post> {
  let id = refs::tweet_id(arg)?;
  let variables = json!({
    "tweetId": id,
    "withCommunity": false,
    "includePromotedContent": false,
    "withVoice": false,
  });
  let data = api.graphql(&TWEET_RESULT, variables).await?;
  parse::post(data.at("data.tweetResult.result")).ok_or_else(|| {
    let reason = data
      .str("data.tweetResult.result.reason")
      .or_else(|| data.str("data.tweetResult.result.__typename"))
      .map(|r| format!(" ({r})"))
      .unwrap_or_default();
    Error::not_found(format!("tweet {id} not found or not visible{reason}"))
  })
}

/// Replies of a tweet, one conversation thread per comment (the rest of the
/// thread as its replies). `sort`: relevance, recency or likes.
pub async fn comments(
  api: &Api,
  arg: &str,
  sort: Option<&str>,
  req: &PageReq,
) -> Result<Page<Comment>> {
  let id = refs::tweet_id(arg)?;
  let ranking = match sort {
    Some("recency") => "Recency",
    Some("likes") => "Likes",
    _ => "Relevance",
  };
  let mut variables = json!({
    "focalTweetId": id,
    "referrer": "tweet",
    "with_rux_injections": false,
    "includePromotedContent": true,
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
      let mut thread = e
        .items
        .iter()
        .filter_map(|(_, item)| parse::post(item.at("tweet_results.result")))
        .map(comment);
      let mut first = thread.next()?;
      first.reply_to = None;
      first.replies = thread.collect();
      Some(first)
    })
    .collect();
  let next = tl
    .bottom
    .or(tl.more)
    .filter(|c| Some(c) != req.cursor.as_ref());
  Ok(match next {
    Some(next) if !items.is_empty() => Page::new(items, Some(next)),
    _ => Page::last(items),
  })
}

/// A reply shown as a comment: the leading `@mentions` of the reply are dropped.
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
  if let Some(views) = p.metrics.views {
    extra.insert("views".into(), views.into());
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

/// `sort`: top (default) or latest; `filter`: media, images or videos.
pub async fn search(api: &Api, q: &Query, req: &PageReq) -> Result<Page<Post>> {
  let product = match q.sort.as_deref() {
    Some("latest") => "Latest",
    _ => "Top",
  };
  let mut raw = q.keyword.trim().to_owned();
  if let Some(f) = &q.filter {
    raw.push_str(&format!(" filter:{f}"));
  }
  let variables = with(
    vars(req),
    json!({ "rawQuery": raw, "querySource": "typed_query", "product": product }),
  );
  timeline::page(
    api,
    &SEARCH,
    variables,
    SEARCH_TIMELINE,
    req,
    timeline::posts,
  )
  .await
}

/// `for-you` (default) or `following` (chronological).
pub async fn feed(api: &Api, kind: Option<&str>, req: &PageReq) -> Result<Page<Post>> {
  let op = match kind {
    Some("following") => &HOME_LATEST,
    _ => &HOME,
  };
  let variables = with(
    vars(req),
    json!({
      "includePromotedContent": false,
      "latestControlAvailable": true,
      "requestContext": "launch",
      "withCommunity": true,
    }),
  );
  let paths = &["data.home.home_timeline_urt.instructions"];
  timeline::page(api, op, variables, paths, req, timeline::posts).await
}

pub async fn user_posts(api: &Api, arg: &str, req: &PageReq) -> Result<Page<Post>> {
  let id = users::user_id(api, arg).await?;
  let variables = with(
    vars(req),
    json!({
      "userId": id,
      "includePromotedContent": true,
      "latestControlAvailable": true,
      "requestContext": "launch",
      "withQuickPromoteEligibilityTweetFields": true,
      "withVoice": true,
      "withV2Timeline": true,
    }),
  );
  timeline::page(
    api,
    &USER_TWEETS,
    variables,
    USER_TIMELINE,
    req,
    timeline::posts,
  )
  .await
}

pub async fn likes(api: &Api, arg: Option<&str>, req: &PageReq) -> Result<Page<Post>> {
  api.require_login()?;
  let id = users::user_id_or_self(api, arg).await?;
  let variables = with(
    vars(req),
    json!({
      "userId": id,
      "includePromotedContent": false,
      "withClientEventToken": false,
      "withBirdwatchNotes": false,
      "withVoice": true,
    }),
  );
  timeline::page(api, &LIKES, variables, USER_TIMELINE, req, timeline::posts).await
}

/// Bookmarks of the logged-in account, or of one bookmark folder.
pub async fn bookmarks(
  api: &Api,
  arg: Option<&str>,
  folder: Option<&str>,
  req: &PageReq,
) -> Result<Page<Post>> {
  if arg.is_some() {
    return Err(Error::input(
      "bookmarks are private on X: omit USER to list your own",
    ));
  }
  if let Some(folder) = folder {
    let variables = with(
      vars(req),
      json!({ "bookmark_collection_id": folder, "includePromotedContent": false }),
    );
    let paths = &["data.bookmark_collection_timeline.timeline.instructions"];
    return timeline::page(
      api,
      &BOOKMARK_FOLDER,
      variables,
      paths,
      req,
      timeline::posts,
    )
    .await;
  }
  let variables = with(
    vars(req),
    json!({
      "includePromotedContent": false,
      "latestControlAvailable": true,
      "requestContext": "launch",
    }),
  );
  let paths = &[
    "data.bookmark_timeline_v2.timeline.instructions",
    "data.bookmark_timeline.timeline.instructions",
  ];
  timeline::page(api, &BOOKMARKS, variables, paths, req, timeline::posts).await
}

/// Latest tweets of a list.
pub async fn list(api: &Api, arg: &str, req: &PageReq) -> Result<Page<Post>> {
  let id = refs::list_id(arg)?;
  let variables = with(vars(req), json!({ "listId": id }));
  let paths = &["data.list.tweets_timeline.timeline.instructions"];
  timeline::page(api, &LIST_TWEETS, variables, paths, req, timeline::posts).await
}
