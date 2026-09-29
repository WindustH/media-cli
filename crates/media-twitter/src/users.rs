//! Accounts: profiles, the logged-in user, followers / following, likers and
//! retweeters of a tweet, people search, and the lists and bookmark folders
//! shown as collections.

use media_core::{
  Collection, Error, ErrorCode, Page, PageReq, Query, Result, User, Value, ValueExt, json,
};

use crate::api::{Api, REST};
use crate::graphql::{
  BOOKMARK_FOLDERS, FAVORITERS, FOLLOWERS, FOLLOWING, LIST_OWNERSHIPS, Op, RETWEETERS, SEARCH,
  USER_BY_REST_ID, USER_BY_SCREEN_NAME,
};
use crate::parse;
use crate::refs::{self, UserRef, list_url};
use crate::timeline::{self, Timeline, vars, with};
use crate::tweets;

const USER_TIMELINE: &[&str] = &[
  "data.user.result.timeline.timeline.instructions",
  "data.user.result.timeline_v2.timeline.instructions",
];
pub const SEARCH_TIMELINE: &[&str] =
  &["data.search_by_raw_query.search_timeline.timeline.instructions"];

pub async fn lookup(api: &Api, r: &UserRef) -> Result<User> {
  let (op, variables) = match r {
    UserRef::Handle(h) => (&USER_BY_SCREEN_NAME, json!({ "screen_name": h })),
    UserRef::Id(id) => (&USER_BY_REST_ID, json!({ "userId": id })),
  };
  let variables = with(variables, json!({ "withSafetyModeUserFields": true }));
  let data = api.graphql(op, variables).await?;
  let mut user = parse::user(data.at("data.user.result")).ok_or_else(|| {
    let who = match r {
      UserRef::Handle(h) => format!("@{h}"),
      UserRef::Id(id) => format!("user id {id}"),
    };
    let reason = data
      .str("data.user.result.reason")
      .map(|r| format!(" ({r})"))
      .unwrap_or_default();
    Error::not_found(format!("{who} not found or unavailable{reason}"))
  })?;
  if !api.logged_in() {
    user.followed = None;
  }
  Ok(user)
}

pub async fn user(api: &Api, arg: &str) -> Result<User> {
  lookup(api, &refs::user(arg)?).await
}

/// Numeric id of a user argument (a lookup unless it already is one).
pub async fn user_id(api: &Api, arg: &str) -> Result<String> {
  match refs::user(arg)? {
    UserRef::Id(id) => Ok(id),
    handle => Ok(lookup(api, &handle).await?.id),
  }
}

/// Numeric id of `arg`, or of the logged-in account when absent.
pub async fn user_id_or_self(api: &Api, arg: Option<&str>) -> Result<String> {
  match arg {
    Some(arg) => user_id(api, arg).await,
    None => match api.own_id() {
      Some(id) => Ok(id),
      None => Ok(whoami(api).await?.id),
    },
  }
}

pub async fn whoami(api: &Api) -> Result<User> {
  api.require_login()?;
  let settings = api
    .get(&format!("{REST}/account/settings.json"), &[])
    .await?;
  let handle = settings
    .str("screen_name")
    .ok_or_else(|| Error::auth("X did not return the account of this session"))?;
  lookup(api, &UserRef::Handle(handle)).await
}

async fn people(api: &Api, op: &Op, arg: &str, req: &PageReq) -> Result<Page<User>> {
  let id = user_id(api, arg).await?;
  let variables = with(
    vars(req),
    json!({ "userId": id, "includePromotedContent": false }),
  );
  timeline::page(api, op, variables, USER_TIMELINE, req, timeline::users).await
}

pub async fn followers(api: &Api, arg: &str, req: &PageReq) -> Result<Page<User>> {
  people(api, &FOLLOWERS, arg, req).await
}

pub async fn following(api: &Api, arg: &str, req: &PageReq) -> Result<Page<User>> {
  people(api, &FOLLOWING, arg, req).await
}

/// One page of the users of a tweet's engagement timeline (`Favoriters`, `Retweeters`).
async fn engaged(api: &Api, op: &Op, id: &str, path: &str, req: &PageReq) -> Result<Page<User>> {
  api.require_login()?;
  let variables = with(
    vars(req),
    json!({ "tweetId": id, "includePromotedContent": false }),
  );
  let data = api.graphql(op, variables).await?;
  let tl = Timeline::at(&data, &[path]);
  // Past the end X answers the last page again, with the same bottom cursor.
  if req.cursor.is_some() && tl.bottom == req.cursor {
    return Ok(Page::last(Vec::new()));
  }
  let items = timeline::users(&tl);
  Ok(match tl.bottom {
    Some(next) if !items.is_empty() => Page::new(items, Some(next)),
    _ => Page::last(items),
  })
}

/// Accounts that liked a tweet. Likes are private on X since June 2024: only
/// the author sees them; for other posts X answers an empty list.
pub async fn likers(api: &Api, arg: &str, req: &PageReq) -> Result<Page<User>> {
  let id = refs::tweet_id(arg)?;
  let path = "data.favoriters_timeline.timeline.instructions";
  let page = engaged(api, &FAVORITERS, &id, path, req).await?;
  if page.items.is_empty() && req.cursor.is_none() {
    let tweet = tweets::read(api, &id).await?;
    let own = user_id_or_self(api, None).await?;
    if tweet.author.as_ref().is_some_and(|a| a.id != own) {
      let hint = "likes are private on X since June 2024; `media x read POST` shows the like count";
      return Err(
        Error::new(
          ErrorCode::PermissionDenied,
          "X shows who liked a post only to its author",
        )
        .with_hint(hint),
      );
    }
  }
  Ok(page)
}

/// Accounts that retweeted a tweet (its quotes are `reposts`).
pub async fn retweeters(api: &Api, arg: &str, req: &PageReq) -> Result<Page<User>> {
  let id = refs::tweet_id(arg)?;
  let path = "data.retweeters_timeline.timeline.instructions";
  engaged(api, &RETWEETERS, &id, path, req).await
}

pub async fn search(api: &Api, q: &Query, req: &PageReq) -> Result<Page<User>> {
  let variables = with(
    vars(req),
    json!({ "rawQuery": q.keyword, "querySource": "typed_query", "product": "People" }),
  );
  timeline::page(
    api,
    &SEARCH,
    variables,
    SEARCH_TIMELINE,
    req,
    timeline::users,
  )
  .await
}

/// Lists owned by a user (the logged-in one when absent).
pub async fn lists(api: &Api, arg: Option<&str>, req: &PageReq) -> Result<Page<Collection>> {
  let id = user_id_or_self(api, arg).await?;
  let variables = with(
    vars(req),
    json!({ "userId": id, "isListMembershipShown": true, "isListMemberTargetUserId": id }),
  );
  timeline::page(
    api,
    &LIST_OWNERSHIPS,
    variables,
    USER_TIMELINE,
    req,
    |tl: &Timeline| {
      tl.items()
        .filter_map(|(_, item)| list(item.at("list")))
        .collect()
    },
  )
  .await
}

fn list(v: &Value) -> Option<Collection> {
  let id = v.str("id_str")?;
  let mut c = Collection {
    kind: "list".into(),
    name: v.str("name").unwrap_or_default(),
    description: v.str("description"),
    url: Some(list_url(&id)),
    items: v.count("member_count"),
    followers: v.count("subscriber_count"),
    owner: parse::user(v.at("user_results.result")),
    raw: Some(v.clone()),
    id,
    ..Collection::default()
  };
  if let Some(mode) = v.str("mode") {
    c.extra.insert("mode".into(), mode.to_lowercase().into());
  }
  Some(c)
}

/// Bookmark folders of the logged-in account (a Premium feature).
pub async fn folders(api: &Api, req: &PageReq) -> Result<Page<Collection>> {
  let mut variables = json!({});
  if let Some(c) = &req.cursor {
    variables["cursor"] = c.as_str().into();
  }
  let data = api.graphql(&BOOKMARK_FOLDERS, variables).await?;
  let slice = data.at("data.viewer.user_results.result.bookmark_collections_slice");
  let items = slice
    .list("items")
    .iter()
    .filter_map(|f| {
      Some(Collection {
        id: f.str("id")?,
        kind: "folder".into(),
        name: f.str("name").unwrap_or_default(),
        raw: Some(f.clone()),
        ..Collection::default()
      })
    })
    .collect();
  let next = slice
    .str("slice_info.next_cursor")
    .filter(|c| Some(c) != req.cursor.as_ref());
  Ok(Page::new(items, next))
}
