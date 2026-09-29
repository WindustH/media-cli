//! Channels: search / grid entries, the channel page header and its "about"
//! panel (`aboutChannelViewModel`).

use media_core::{User, Value, ValueExt, json};

use super::{count, date, find, first, image, put, text, verified};
use crate::refs::channel_url;

/// `channelRenderer` / `gridChannelRenderer`. Search results show the handle
/// where the subscriber count used to be, and the subscribers in place of
/// the video count.
pub fn channel(r: &Value) -> Option<User> {
  let id = r.str("channelId")?;
  let texts: Vec<String> = ["subscriberCountText", "videoCountText"]
    .iter()
    .filter_map(|k| text(r.at(k)))
    .collect();
  let handle = texts
    .iter()
    .find(|t| t.starts_with('@'))
    .cloned()
    .or_else(|| {
      r.str("navigationEndpoint.browseEndpoint.canonicalBaseUrl")
        .and_then(|u| u.strip_prefix('/').map(str::to_owned))
        .filter(|h| h.starts_with('@'))
    });
  let mut u = User {
    name: text(r.at("title")).unwrap_or_else(|| id.clone()),
    url: Some(channel_url(&id)),
    avatar: image(r.at("thumbnail")),
    bio: text(r.at("descriptionSnippet")),
    verified: verified(r.at("ownerBadges")),
    handle,
    id,
    raw: Some(r.clone()),
    ..User::default()
  };
  for t in &texts {
    if t.contains("subscriber") {
      u.stats.followers = count(t);
    } else if t.contains("video") {
      u.stats.posts = count(t);
    }
  }
  if let Some(b) = r.bool("subscribeButton.subscribeButtonRenderer.subscribed") {
    u.followed = Some(b);
  }
  Some(u)
}

/// The channel page (`browse` of a UC id): header plus `channelMetadataRenderer`.
pub fn channel_page(v: &Value) -> Option<User> {
  let meta = v.at("metadata.channelMetadataRenderer");
  let header = v.at("header.pageHeaderRenderer.content.pageHeaderViewModel");
  let old = v.at("header.c4TabbedHeaderRenderer");
  let id = meta.str("externalId").or_else(|| old.str("channelId"))?;
  let rows: Vec<String> = header
    .list("metadata.contentMetadataViewModel.metadataRows")
    .iter()
    .flat_map(|r| r.list("metadataParts"))
    .filter_map(|p| p.str("text.content"))
    .collect();
  let handle = rows
    .iter()
    .find(|t| t.starts_with('@'))
    .cloned()
    .or_else(|| text(old.at("channelHandleText")))
    .or_else(|| {
      meta
        .str("vanityChannelUrl")
        .and_then(|u| u.rsplit('/').next().map(str::to_owned))
        .filter(|h| h.starts_with('@'))
    });
  let title = header.at("title");
  let mut u = User {
    name: meta
      .str("title")
      .or_else(|| text(title.at("dynamicTextViewModel.text")))
      .unwrap_or_else(|| id.clone()),
    url: Some(channel_url(&id)),
    avatar: image(meta.at("avatar")).or_else(|| first(header.at("image"), "image").and_then(image)),
    bio: meta.str("description").map(|d| d.trim().to_owned()),
    verified: find(title, "imageName")
      .iter()
      .any(|n| n.as_str().is_some_and(|s| s.starts_with("CHECK_CIRCLE")))
      || verified(old.at("badges")),
    followed: followed(v),
    handle,
    id,
    raw: Some(json!({ "header": v.at("header"), "metadata": meta })),
    ..User::default()
  };
  for t in rows
    .iter()
    .chain(text(old.at("subscriberCountText")).iter())
    .chain(text(old.at("videosCountText")).iter())
  {
    if t.contains("subscriber") {
      u.stats.followers = count(t);
    } else if t.contains("video") {
      u.stats.posts = count(t);
    }
  }
  let x = &mut u.extra;
  put(x, "keywords", meta.str("keywords"));
  put(x, "rss", meta.str("rssUrl"));
  put(x, "family_safe", meta.bool("isFamilySafe"));
  Some(u)
}

/// Add the "about" panel: total views, joined date, country, links, exact
/// counts where the header abbreviates them.
pub fn about(v: &Value, u: &mut User) {
  let Some(a) = first(v, "aboutChannelViewModel") else {
    return;
  };
  if let Some(d) = a.str("description").map(|d| d.trim().to_owned()) {
    u.bio = Some(d);
  }
  u.location = a.str("country");
  let joined = text(a.at("joinedDateText"));
  u.created_at = joined.as_deref().and_then(date);
  if let Some(n) = text(a.at("viewCountText")).and_then(|t| count(&t)) {
    u.stats.other.insert("views".into(), n);
  }
  if let Some(n) = text(a.at("videoCountText")).and_then(|t| count(&t)) {
    u.stats.posts = Some(n);
  }
  if let Some(n) = text(a.at("subscriberCountText")).and_then(|t| count(&t)) {
    u.stats.followers = Some(n);
  }
  let links: Vec<Value> = a
    .list("links")
    .iter()
    .filter_map(|l| {
      let l = l.at("channelExternalLinkViewModel");
      let url = text(l.at("link"))?;
      Some(json!({ "title": text(l.at("title")), "url": url }))
    })
    .collect();
  let x = &mut u.extra;
  put(x, "joined", joined);
  if !links.is_empty() {
    put(x, "links", links);
  }
  if let Some(r) = u.raw.as_mut().and_then(Value::as_object_mut) {
    r.insert("about".into(), a.clone());
  }
}

/// Whether the logged-in account subscribes to the channel of a page.
pub fn followed(v: &Value) -> Option<bool> {
  find(v.at("frameworkUpdates"), "subscriptionStateEntity")
    .first()
    .and_then(|s| s.bool("subscribed"))
    .or_else(|| {
      find(v.at("header"), "subscribeButtonRenderer")
        .first()
        .and_then(|b| b.bool("subscribed"))
    })
}
