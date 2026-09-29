//! Watch pages: one video in full (`player` for the details, `next` for
//! owner, likes and chapters), community posts, related videos and what
//! YouTube offers in place of Trending.
//!
//! YouTube retired its Trending page in 2025 (`FEtrending` answers 400);
//! [`hot`] reads the Hype leaderboard and the Explore destinations instead.

use media_core::{Error, Page, PageReq, Post, Result, Value, ValueExt, json};

use crate::api::{self, Api};
use crate::parse;
use crate::refs::{self, PostRef};
use crate::{browse, comment};

/// `hot --category`: the leaderboard of hyped videos, then Explore pages.
pub const HOT: &[(&str, &str)] = &[
  ("hype", "FEhype_leaderboard"),
  ("music", "UC-9-kyTW8ZkZNDHQJ6FgpwQ"),
  ("gaming", "UCOpNcN46UbXVtpKMrmU4Abg"),
  ("news", "UCYfdidRxbB8Qhf0Nx7ioOYw"),
  ("sports", "UCEgdi0XIXXZ-qJOFPf4JSKw"),
  ("live", "UC4R8DWoMoI7CAwX8_LjQHig"),
  ("learning", "UCtFRv9O2AHqOZjjynzrv-xg"),
  ("podcasts", "FEpodcasts_destination"),
];
pub const HOT_CATEGORIES: &[&str] = &[
  "hype", "music", "gaming", "news", "sports", "live", "learning", "podcasts",
];

/// The web player: full details (the web client gets no streams without a
/// proof-of-origin token, but the metadata is complete).
pub async fn player(api: &Api, id: &str) -> Result<Value> {
  let body = json!({ "videoId": id, "contentCheckOk": true, "racyCheckOk": true });
  api.call("player", body).await
}

/// The Android VR player: plain stream URLs and caption tracks.
pub async fn mobile_player(api: &Api, id: &str) -> Result<Value> {
  let body = json!({
    "videoId": id,
    "contentCheckOk": true,
    "racyCheckOk": true,
    "playbackContext": { "contentPlaybackContext": { "html5Preference": "HTML5_PREF_WANTS" } },
  });
  api.call_as(api::Client::AndroidVr, "player", body).await
}

pub async fn read(api: &Api, arg: &str) -> Result<Post> {
  match refs::post(arg)? {
    PostRef::Video(id) => video(api, &id).await,
    PostRef::Post(id) => community(api, &id).await,
  }
}

/// Number of comments shown on the watch page (`1.5K`) and whether it is exact.
fn comment_count(next: &Value) -> Option<(u64, bool)> {
  let panel = next.list("engagementPanels").iter().find(|p| {
    p.str("engagementPanelSectionListRenderer.panelIdentifier")
      .as_deref()
      == Some("engagement-panel-comments-section")
  })?;
  let t = parse::text(panel.at(
    "engagementPanelSectionListRenderer.header.engagementPanelTitleHeaderRenderer.contextualInfo",
  ))?;
  let exact = t.chars().all(|c| c.is_ascii_digit() || c == ',');
  Some((parse::count(&t)?, exact))
}

pub async fn video(api: &Api, id: &str) -> Result<Post> {
  let player = player(api, id).await?;
  if player.at("videoDetails").is_null() {
    // Private, removed or never existed: the refusal says which.
    api::playable(&player)?;
    return Err(Error::not_found(format!("video {id} not found")));
  }
  let next = api.call("next", json!({ "videoId": id })).await?;
  let mut post = parse::full_video(&player, &next)
    .ok_or_else(|| Error::not_found(format!("video {id} not found")))?;
  post.metrics.comments = match comment_count(&next) {
    Some((n, true)) => Some(n),
    // Abbreviated (`1.5K`): the comments header has the exact number.
    Some((n, false)) => Some(comment::count(api, id).await.ok().flatten().unwrap_or(n)),
    None => None,
  };
  Ok(post)
}

/// A community post: `/post/<id>` resolves to its detail page.
async fn community(api: &Api, id: &str) -> Result<Post> {
  let url = refs::post_url(id);
  let missing = || Error::not_found(format!("post {id} not found"));
  let v = api
    .call("navigation/resolve_url", json!({ "url": url }))
    .await?;
  let endpoint = v.at("endpoint.browseEndpoint");
  let (Some(browse_id), params) = (endpoint.str("browseId"), endpoint.str("params")) else {
    return Err(missing());
  };
  let page = browse::listing(api, &browse_id, params.as_deref(), &PageReq::default()).await?;
  page
    .posts
    .into_iter()
    .find(|p| p.id == id)
    .ok_or_else(missing)
}

pub async fn hot(api: &Api, category: Option<&str>, req: &PageReq) -> Result<Page<Post>> {
  let name = category.unwrap_or("hype");
  let id = HOT
    .iter()
    .find(|(n, _)| *n == name)
    .map(|(_, id)| *id)
    .ok_or_else(|| Error::input(format!("unknown category `{name}`")))?;
  let l = browse::listing(api, id, None, req).await?;
  Ok(Page::new(l.posts, l.next))
}

/// Videos YouTube suggests next to a video.
pub async fn related(api: &Api, arg: &str, req: &PageReq) -> Result<Page<Post>> {
  let body = match &req.cursor {
    Some(token) => json!({ "continuation": token }),
    None => json!({ "videoId": refs::video(arg)? }),
  };
  let v = api.call("next", body).await?;
  // Only the side column: the main column holds the video itself.
  let side = match &req.cursor {
    Some(_) => v,
    None => json!({ "contents": v.at("contents.twoColumnWatchNextResults.secondaryResults") }),
  };
  let l = parse::listing(&side);
  Ok(Page::new(l.posts, l.next))
}
