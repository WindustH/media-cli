//! Search: videos (with sort and one filter), channels and playlists.
//!
//! Sort and filters travel in the `params` protobuf (the `sp=` URL
//! parameter): field 1 is the sort, field 2 the filters (upload date 1,
//! type 2, duration 3, features 4…26), as YouTube.js `SearchFilter` and the
//! site's own filter links (`searchFilterGroupRenderer`) encode them. Since
//! 2025 the site offers two orders only, relevance and popularity; the old
//! upload-date and rating orders are ignored by the server.

use media_core::{Collection, Error, Page, PageReq, Post, Query, Result, User, Value, json};

use crate::api::Api;
use crate::parse::{self, Listing};
use crate::proto::Msg;

pub const SORTS: &[&str] = &["relevance", "popularity"];
pub const FILTERS: &[&str] = &[
  "video",
  "shorts",
  "live",
  "movie",
  "today",
  "week",
  "month",
  "year",
  "under-3min",
  "3-20min",
  "over-20min",
  "hd",
  "4k",
  "hdr",
  "subtitles",
  "creative-commons",
];

/// Shelves of Shorts and suggestions mixed into the results.
const SHELVES: &[&str] = &["gridShelfViewModel", "reelShelfRenderer", "shelfRenderer"];

/// Result types (filter field 2).
const VIDEO: u64 = 1;
const CHANNEL: u64 = 2;
const PLAYLIST: u64 = 3;

fn params(sort: Option<&str>, filter: Option<&str>, kind: Option<u64>) -> Result<String> {
  let sort = match sort.unwrap_or("relevance") {
    "relevance" => 0,
    "popularity" => 3,
    other => return Err(Error::input(format!("unknown sort `{other}`"))),
  };
  let mut fields: Vec<(u32, u64)> = kind.map(|k| (2, k)).into_iter().collect();
  if let Some(name) = filter {
    let (field, value) = match name {
      "today" => (1, 2),
      "week" => (1, 3),
      "month" => (1, 4),
      "year" => (1, 5),
      "video" => (2, VIDEO),
      "movie" => (2, 4),
      "shorts" => (2, 9),
      "over-20min" => (3, 2),
      "under-3min" => (3, 4),
      "3-20min" => (3, 5),
      "hd" => (4, 1),
      "subtitles" => (5, 1),
      "creative-commons" => (6, 1),
      "live" => (8, 1),
      "4k" => (14, 1),
      "hdr" => (25, 1),
      other => return Err(Error::input(format!("unknown filter `{other}`"))),
    };
    fields.push((field, value));
  }
  fields.sort();
  let f = fields
    .into_iter()
    .fold(Msg::new(), |m, (field, value)| m.int(field, value));
  let mut outer = Msg::new();
  if sort > 0 {
    outer = outer.int(1, sort);
  }
  if !f.is_empty() {
    outer = outer.msg(2, f);
  }
  Ok(outer.encode())
}

async fn run(api: &Api, q: &Query, kind: Option<u64>, req: &PageReq) -> Result<Listing> {
  let body = match &req.cursor {
    Some(token) => json!({ "continuation": token }),
    None => {
      let mut b = json!({ "query": q.keyword });
      let p = params(q.sort.as_deref(), q.filter.as_deref(), kind)?;
      if !p.is_empty() {
        b["params"] = Value::from(p);
      }
      b
    }
  };
  let v = api.call("search", body).await?;
  // The Shorts shelf in the middle of the results ignores filters.
  Ok(match q.filter.as_deref() {
    Some(f) if f != "shorts" => parse::listing_without(&v, SHELVES),
    _ => parse::listing(&v),
  })
}

pub async fn videos(api: &Api, q: &Query, req: &PageReq) -> Result<Page<Post>> {
  let l = run(api, q, None, req).await?;
  Ok(Page::new(l.posts, l.next))
}

pub async fn channels(api: &Api, q: &Query, req: &PageReq) -> Result<Page<User>> {
  let q = Query {
    filter: None,
    ..q.clone()
  };
  let l = run(api, &q, Some(CHANNEL), req).await?;
  Ok(Page::new(l.users, l.next))
}

/// Playlists (and courses / podcasts): YouTube has no hashtag or topic search.
pub async fn playlists(api: &Api, q: &Query, req: &PageReq) -> Result<Page<Collection>> {
  let q = Query {
    filter: None,
    ..q.clone()
  };
  let l = run(api, &q, Some(PLAYLIST), req).await?;
  Ok(Page::new(l.collections, l.next))
}
