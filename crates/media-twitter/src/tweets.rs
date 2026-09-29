//! Reading tweets: one tweet, search, quotes, home feeds, user timelines,
//! likes, bookmarks and lists.

use media_core::{Error, Page, PageReq, Post, Query, Result, ValueExt, json};

use crate::api::Api;
use crate::graphql::{
  BOOKMARK_FOLDER, BOOKMARKS, HOME, HOME_LATEST, LIKES, LIST_TWEETS, SEARCH, TWEET_RESULT,
  USER_TWEETS,
};
use crate::parse;
use crate::refs;
use crate::timeline::{self, vars, with};
use crate::users::{self, SEARCH_TIMELINE};

const USER_TIMELINE: &[&str] = &[
  "data.user.result.timeline.timeline.instructions",
  "data.user.result.timeline_v2.timeline.instructions",
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

/// Quotes of a tweet, newest first: the search the web client's quotes tab runs.
pub async fn quotes(api: &Api, arg: &str, req: &PageReq) -> Result<Page<Post>> {
  let id = refs::tweet_id(arg)?;
  let variables = with(
    vars(req),
    json!({
      "rawQuery": format!("quoted_tweet_id:{id}"),
      "querySource": "tdqt",
      "product": "Latest",
    }),
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
