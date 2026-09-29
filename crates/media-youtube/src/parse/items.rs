//! One walk over a listing response (search, browse, continuation) that maps
//! every item renderer it knows and remembers the continuation token.
//!
//! Only the page's own content is walked (`contents` and the continuation
//! actions), not the header, engagement panels or entity updates, so tokens
//! of the "about" panel or of reply threads never pass for the next page.

use media_core::{Collection, Post, User, Value, ValueExt};

use super::video::{self, Lockup};
use super::{byline, channel, count, image, post, put, text};
use crate::refs::playlist_url;

#[derive(Default)]
pub struct Listing {
  pub posts: Vec<Post>,
  pub users: Vec<User>,
  pub collections: Vec<Collection>,
  /// Token of the next page.
  pub next: Option<String>,
}

/// Parts of a response that hold its items.
const ROOTS: &[&str] = &[
  "contents",
  "continuationContents",
  "onResponseReceivedActions",
  "onResponseReceivedCommands",
  "onResponseReceivedEndpoints",
];

pub fn listing(v: &Value) -> Listing {
  listing_without(v, &[])
}

/// [`listing`] leaving out the containers named in `skip` (e.g. shelves).
pub fn listing_without(v: &Value, skip: &[&str]) -> Listing {
  let mut out = Listing::default();
  for root in ROOTS {
    walk(v.at(root), skip, &mut out);
  }
  out
}

/// The token of a `continuationItemRenderer` (a list's "load more" item or a
/// "show more replies" button).
pub fn token(cir: &Value) -> Option<String> {
  cir.first_str(&[
    "continuationEndpoint.continuationCommand.token",
    "button.buttonRenderer.command.continuationCommand.token",
    "continuationEndpoint.getNotificationMenuEndpoint.ctoken",
  ])
}

pub fn video_renderer(v: &Value) -> Option<Post> {
  video::renderer(v, false)
}

fn walk(v: &Value, skip: &[&str], out: &mut Listing) {
  match v {
    Value::Object(m) => {
      for (k, c) in m {
        match k.as_str() {
          k if skip.contains(&k) => {}
          "videoRenderer"
          | "gridVideoRenderer"
          | "compactVideoRenderer"
          | "playlistVideoRenderer"
          | "videoWithContextRenderer"
          | "playlistPanelVideoRenderer" => out.posts.extend(video::renderer(c, false)),
          "reelItemRenderer" => out.posts.extend(video::renderer(c, true)),
          "shortsLockupViewModel" => out.posts.extend(video::short(c)),
          "lockupViewModel" => match video::lockup(c) {
            Some(Lockup::Video(p)) => out.posts.push(p),
            Some(Lockup::Playlist(p)) => out.collections.push(p),
            None => {}
          },
          "channelRenderer" | "gridChannelRenderer" => out.users.extend(channel(c)),
          "playlistRenderer" | "gridPlaylistRenderer" | "compactPlaylistRenderer" => {
            out.collections.extend(playlist(c))
          }
          "backstagePostRenderer" => out.posts.extend(post::backstage(c)),
          "continuationItemRenderer" => {
            if let Some(t) = token(c) {
              out.next = Some(t);
            }
          }
          _ => walk(c, skip, out),
        }
      }
    }
    Value::Array(a) => a.iter().for_each(|c| walk(c, skip, out)),
    _ => {}
  }
}

/// `playlistRenderer` / `gridPlaylistRenderer`.
fn playlist(r: &Value) -> Option<Collection> {
  let id = r.str("playlistId")?;
  let owner = ["longBylineText", "shortBylineText", "ownerText"]
    .iter()
    .find_map(|k| byline(r.at(k)));
  let mut c = Collection {
    kind: "playlist".into(),
    name: text(r.at("title")).unwrap_or_else(|| id.clone()),
    url: Some(playlist_url(&id)),
    items: r
      .u64("videoCount")
      .or_else(|| text(r.at("videoCountText")).and_then(|t| count(&t))),
    owner,
    raw: Some(r.clone()),
    id,
    ..Collection::default()
  };
  put(
    &mut c.extra,
    "thumbnail",
    image(r.at("thumbnail")).or_else(|| r.list("thumbnails").first().and_then(image)),
  );
  put(&mut c.extra, "updated", text(r.at("publishedTimeText")));
  Some(c)
}
