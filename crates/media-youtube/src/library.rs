//! The account's library: home and subscription feeds, subscriptions,
//! playlists (Watch later `WL`, Liked videos `LL`, own ones), history; and
//! any public playlist.

use media_core::{Collection, Error, Page, PageReq, Post, Result, User};

use crate::api::Api;
use crate::channel::{self, Tab};
use crate::{account, browse, refs};

pub const FEED_KINDS: &[&str] = &["home", "subscriptions"];

pub async fn feed(api: &Api, kind: Option<&str>, req: &PageReq) -> Result<Page<Post>> {
  let id = match kind.unwrap_or("home") {
    "subscriptions" => {
      api.require_login()?;
      "FEsubscriptions"
    }
    _ => "FEwhat_to_watch",
  };
  let l = browse::listing(api, id, None, req).await?;
  if l.posts.is_empty() && req.cursor.is_none() && !api.logged_in() {
    return Err(
      Error::auth("YouTube shows no home feed without a login").with_hint(api.ctx.login_hint()),
    );
  }
  Ok(Page::new(l.posts, l.next))
}

/// Refuse lists YouTube shows to their owner only.
async fn own_only(api: &Api, user: Option<&str>, what: &str) -> Result<()> {
  api.require_login()?;
  match user {
    Some(u) if !account::is_mine(api, u).await? => Err(Error::input(format!(
      "YouTube shows {what} of the logged-in account only (use `me`)"
    ))),
    _ => Ok(()),
  }
}

/// Channels the logged-in account subscribes to.
pub async fn following(api: &Api, user: &str, req: &PageReq) -> Result<Page<User>> {
  own_only(api, Some(user), "subscriptions").await?;
  let l = browse::listing(api, "FEchannels", None, req).await?;
  Ok(Page::new(l.users, l.next))
}

/// Playlists of a channel, or of the logged-in account (with Watch later and Liked videos).
pub async fn collections(api: &Api, user: Option<&str>, req: &PageReq) -> Result<Page<Collection>> {
  let l = match user {
    Some(u) if !account::is_me(u) => channel::tab(api, u, Tab::Playlists, req).await?,
    _ => {
      api.require_login()?;
      browse::listing(api, "FEplaylist_aggregation", None, req).await?
    }
  };
  Ok(Page::new(l.collections, l.next))
}

/// Videos of a playlist, in playlist order.
pub async fn playlist(api: &Api, id: &str, req: &PageReq) -> Result<Page<Post>> {
  let l = browse::listing(api, &format!("VL{id}"), None, req).await?;
  Ok(Page::new(l.posts, l.next))
}

/// Watch later, or the playlist given as the folder.
pub async fn favorites(
  api: &Api,
  user: Option<&str>,
  folder: Option<&str>,
  req: &PageReq,
) -> Result<Page<Post>> {
  match folder {
    Some(f) => playlist(api, &refs::playlist(f)?, req).await,
    None => {
      own_only(api, user, "Watch later").await?;
      playlist(api, "WL", req).await
    }
  }
}

/// Liked videos (the `LL` playlist).
pub async fn likes(api: &Api, user: Option<&str>, req: &PageReq) -> Result<Page<Post>> {
  own_only(api, user, "liked videos").await?;
  playlist(api, "LL", req).await
}

pub async fn history(api: &Api, req: &PageReq) -> Result<Page<Post>> {
  api.require_login()?;
  let l = browse::listing(api, "FEhistory", None, req).await?;
  Ok(Page::new(l.posts, l.next))
}
