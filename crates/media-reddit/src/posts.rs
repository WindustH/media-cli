//! Posts: search, trending, the front page, one subreddit, a user's
//! submissions, saved and upvoted posts, and single posts with their media.

use media_core::{Error, ErrorCode, Media, Page, PageReq, Post, Query, Result, ValueExt};

use crate::api::Api;
use crate::{listing, parse, refs, video};

pub const SEARCH_SORTS: &[&str] = &["relevance", "hot", "top", "new", "comments"];
pub const TIME_RANGES: &[&str] = &["hour", "day", "week", "month", "year", "all"];
pub const FEED_KINDS: &[&str] = &["best", "hot", "new", "top", "rising"];

pub async fn search(api: &Api, q: &Query, req: &PageReq) -> Result<Page<Post>> {
  let mut query = vec![
    ("q", q.keyword.clone()),
    ("type", "link".into()),
    (
      "sort",
      q.sort.clone().unwrap_or_else(|| SEARCH_SORTS[0].into()),
    ),
  ];
  if let Some(t) = &q.filter {
    query.push(("t", t.clone()));
  }
  listing::posts(api, "/search", query, req).await
}

/// `r/popular` (default) or `r/all`, hot first.
pub async fn hot(api: &Api, category: Option<&str>, req: &PageReq) -> Result<Page<Post>> {
  let sub = category.unwrap_or("popular");
  listing::posts(api, &format!("/r/{sub}/hot"), Vec::new(), req).await
}

/// The front page of a logged-in account, `r/popular` otherwise.
pub async fn feed(api: &Api, kind: Option<&str>, req: &PageReq) -> Result<Page<Post>> {
  let path = if api.logged_in() {
    format!("/{}", kind.unwrap_or("best"))
  } else {
    // `best` exists for the personal front page only.
    let kind = kind.filter(|k| *k != "best").unwrap_or("hot");
    format!("/r/popular/{kind}")
  };
  listing::posts(api, &path, Vec::new(), req).await
}

/// One subreddit in `sort` order; `time` ranges `top` and `controversial`.
pub async fn subreddit(
  api: &Api,
  sub: &str,
  sort: &str,
  time: Option<&str>,
  req: &PageReq,
) -> Result<Page<Post>> {
  let name = refs::subreddit(sub)?;
  let query = time.map(|t| ("t", t.to_owned())).into_iter().collect();
  listing::posts(api, &format!("/r/{name}/{sort}"), query, req).await
}

pub async fn user_posts(api: &Api, user: &str, req: &PageReq) -> Result<Page<Post>> {
  let name = refs::user(user)?;
  let query = vec![("sort", "new".into())];
  listing::posts(api, &format!("/user/{name}/submitted"), query, req).await
}

/// `saved` or `upvoted` posts; Reddit shows them to their owner only.
pub async fn own(api: &Api, user: Option<&str>, which: &str, req: &PageReq) -> Result<Page<Post>> {
  api.require_login()?;
  let name = match user {
    Some(u) => refs::user(u)?,
    None => api.username().await?,
  };
  let query = vec![("type", "links".into())];
  listing::posts(api, &format!("/user/{name}/{which}"), query, req).await
}

/// A post without its comments.
async fn fetch(api: &Api, arg: &str) -> Result<Post> {
  let id = refs::post(&api.ctx, arg).await?;
  let missing = || Error::not_found(format!("post {id} not found"));
  let query = [("limit", "1".into()), ("depth", "1".into())];
  let v = match api.get(&format!("/comments/{id}"), &query).await {
    Err(e) if e.code == ErrorCode::NotFound => return Err(missing()),
    v => v?,
  };
  parse::data(v.at("0.data.children.0"), "t3")
    .and_then(parse::post)
    .ok_or_else(missing)
}

pub async fn read(api: &Api, arg: &str) -> Result<Post> {
  let mut post = fetch(api, arg).await?;
  if let Err(e) = video::add_audio(api, &mut post).await {
    tracing::debug!("no audio track for {}: {e}", post.id);
  }
  Ok(post)
}

/// Media to download: the post's own, or the crossposted original's.
pub async fn media(api: &Api, arg: &str) -> Result<(Post, Vec<Media>)> {
  let mut post = fetch(api, arg).await?;
  video::add_audio(api, &mut post).await?;
  let media = match (&post.quoted, post.media.is_empty()) {
    (Some(parent), true) => parent.media.clone(),
    _ => post.media.clone(),
  };
  Ok((post, media))
}
