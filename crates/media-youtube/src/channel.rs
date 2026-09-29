//! Channels: resolving handles and links to ids, the profile (page header
//! plus "about" panel) and the tabs (videos, Shorts, live streams,
//! playlists, community posts).

use std::time::Duration;

use media_core::{Error, ErrorCode, PageReq, Result, User, Value, ValueExt, json};

use crate::api::Api;
use crate::browse;
use crate::parse::{self, Listing};
use crate::refs::{self, ChannelRef};

/// Handles rarely move to another channel.
const ID_TTL: Duration = Duration::from_secs(30 * 86400);

/// Channel id (`UC…`) of an id, @handle or channel link.
pub async fn id(api: &Api, arg: &str) -> Result<String> {
  let url = match refs::channel(arg)? {
    ChannelRef::Id(id) => return Ok(id),
    ChannelRef::Url(url) => url,
  };
  let key: String = url
    .trim_start_matches(crate::api::WWW)
    .chars()
    .map(|c| {
      if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
        c
      } else {
        '_'
      }
    })
    .collect();
  let key = format!("channel{}", key.to_ascii_lowercase());
  if let Some(id) = api.ctx.store.cache_get::<String>(&key, ID_TTL) {
    return Ok(id);
  }
  let missing = || Error::not_found(format!("no YouTube channel at {url}"));
  let v = match api
    .call("navigation/resolve_url", json!({ "url": url }))
    .await
  {
    Err(e) if matches!(e.code, ErrorCode::NotFound | ErrorCode::UpstreamError) => {
      return Err(missing());
    }
    v => v?,
  };
  let id = v
    .str("endpoint.browseEndpoint.browseId")
    .filter(|id| refs::is_channel_id(id))
    .ok_or_else(missing)?;
  api.ctx.store.cache_put(&key, &id);
  Ok(id)
}

/// The channel page of an id; a missing channel answers 400 / 404.
async fn page(api: &Api, id: &str, params: Option<&str>) -> Result<Value> {
  match browse::page(api, id, params, &PageReq::default()).await {
    Err(e) if matches!(e.code, ErrorCode::NotFound | ErrorCode::UpstreamError) => Err(
      Error::not_found(format!("channel {id} not found ({})", e.message)),
    ),
    other => other,
  }
}

/// Profile: header, then the "about" panel (views, joined, country, links).
pub async fn profile(api: &Api, arg: &str, about: bool) -> Result<User> {
  let id = id(api, arg).await?;
  let v = page(api, &id, None).await?;
  let mut user =
    parse::channel_page(&v).ok_or_else(|| Error::not_found(format!("channel {id} not found")))?;
  // The panel opens from the header: a continuation token behind "…more".
  let token = parse::first(v.at("header"), "continuationCommand")
    .and_then(|c| c.str("token"))
    .filter(|_| about);
  if let Some(token) = token {
    match api.call("browse", json!({ "continuation": token })).await {
      Ok(about) => parse::about(&about, &mut user),
      Err(e) => tracing::debug!("about panel of {id}: {e}"),
    }
  }
  Ok(user)
}

/// Channel tabs, by the slug of their URL (`/@x/videos`).
#[derive(Debug, Clone, Copy)]
pub enum Tab {
  Videos,
  Shorts,
  Streams,
  Playlists,
  Posts,
}

impl Tab {
  fn slug(self) -> &'static str {
    match self {
      Tab::Videos => "videos",
      Tab::Shorts => "shorts",
      Tab::Streams => "streams",
      Tab::Playlists => "playlists",
      Tab::Posts => "posts",
    }
  }

  /// The params the site uses today; checked against the answer, see [`tab`].
  fn params(self) -> &'static str {
    match self {
      Tab::Videos => "EgZ2aWRlb3PyBgQKAjoA",
      Tab::Shorts => "EgZzaG9ydHPyBgUKA5oBAA==",
      Tab::Streams => "EgdzdHJlYW1z8gYECgJ6AA==",
      Tab::Playlists => "EglwbGF5bGlzdHPyBgQKAkIA",
      Tab::Posts => "EgVwb3N0c_IGBAoCSgA=",
    }
  }
}

fn tabs(v: &Value) -> impl Iterator<Item = &Value> {
  v.list("contents.twoColumnBrowseResultsRenderer.tabs")
    .iter()
    .map(|t| t.at("tabRenderer"))
}

fn is(tab: &Value, slug: &str) -> bool {
  tab
    .str("endpoint.commandMetadata.webCommandMetadata.url")
    .is_some_and(|u| u.ends_with(&format!("/{slug}")))
}

/// The first page of a tab. YouTube serves the home tab instead of a tab it
/// does not know, so the selected tab is checked; stale params are replaced
/// by the ones in the page's own tab strip, and a channel without the tab
/// lists nothing. Also returns the channel, which tab items do not name.
async fn first_page(api: &Api, id: &str, tab: Tab) -> Result<(Listing, Option<User>)> {
  let slug = tab.slug();
  let mut v = page(api, id, Some(tab.params())).await?;
  let selected = tabs(&v).any(|t| t.bool("selected") == Some(true) && is(t, slug));
  if !selected {
    let params = tabs(&v)
      .find(|t| is(t, slug))
      .and_then(|t| t.str("endpoint.browseEndpoint.params"));
    let Some(params) = params.filter(|p| p != tab.params()) else {
      return Ok((Listing::default(), None));
    };
    v = page(api, id, Some(&params)).await?;
  }
  let content = tabs(&v)
    .find(|t| t.bool("selected") == Some(true))
    .map(|t| t.at("content"))
    .unwrap_or(&Value::Null);
  let owner = parse::channel_page(&v).map(|u| User {
    id: u.id,
    name: u.name,
    handle: u.handle,
    url: u.url,
    verified: u.verified,
    ..User::default()
  });
  if let Some(o) = &owner {
    let cached = json!({ "name": o.name, "handle": o.handle, "verified": o.verified });
    api.ctx.store.cache_put(&owner_key(id), &cached);
  }
  Ok((parse::listing(&json!({ "contents": content })), owner))
}

fn owner_key(id: &str) -> String {
  format!("owner-of-{id}")
}

/// The channel remembered from the first page of one of its tabs.
fn cached_owner(api: &Api, id: &str) -> Option<User> {
  let o = api.ctx.store.cache_get::<Value>(&owner_key(id), ID_TTL)?;
  Some(User {
    name: o.str("name").unwrap_or_else(|| id.to_owned()),
    handle: o.str("handle"),
    verified: o.bool("verified").unwrap_or(false),
    url: Some(refs::channel_url(id)),
    id: id.to_owned(),
    ..User::default()
  })
}

/// One page of a channel tab. Cursors are `<channel id>:<token>`, so later
/// pages still know whose items they list.
pub async fn tab(api: &Api, arg: &str, tab: Tab, req: &PageReq) -> Result<Listing> {
  let (id, mut l, owner) = match req.cursor.as_deref().and_then(|c| c.split_once(':')) {
    Some((id, token)) => {
      let next = PageReq {
        cursor: Some(token.to_owned()),
        size: req.size,
      };
      let l = browse::listing(api, "", None, &next).await?;
      (id.to_owned(), l, cached_owner(api, id))
    }
    None => {
      let id = id(api, arg).await?;
      let (l, owner) = first_page(api, &id, tab).await?;
      (id, l, owner)
    }
  };
  if let Some(o) = owner {
    for p in l.posts.iter_mut().filter(|p| p.author.is_none()) {
      p.author = Some(o.clone());
    }
    for c in l.collections.iter_mut().filter(|c| c.owner.is_none()) {
      c.owner = Some(o.clone());
    }
  }
  l.next = l.next.map(|t| format!("{id}:{t}"));
  Ok(l)
}
