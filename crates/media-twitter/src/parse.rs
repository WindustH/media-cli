//! Upstream JSON → core models: users and tweets (retweets unwrapped, quotes,
//! long notes, articles, best media variants).

use media_core::text::parse_time;
use media_core::{Media, MediaKind, Metrics, Post, User, UserStats, Value, ValueExt};

use crate::refs::{tweet_url, user_url};

/// A user result (`user_results.result`); `None` for unavailable users.
/// Reads both the current shape (`core`, `relationship_counts`, `profile_bio` ...)
/// and the older one where everything sits in `legacy`.
pub fn user(v: &Value) -> Option<User> {
  if v.str("__typename").as_deref() == Some("UserUnavailable") {
    return None;
  }
  let id = v.str("rest_id")?;
  let handle = v.first_str(&["core.screen_name", "legacy.screen_name"]);
  let mut stats = UserStats {
    followers: v.first_count(&["relationship_counts.followers", "legacy.followers_count"]),
    following: v.first_count(&["relationship_counts.following", "legacy.friends_count"]),
    posts: v.first_count(&["tweet_counts.tweets", "legacy.statuses_count"]),
    likes: v.first_count(&["action_counts.favorites_count", "legacy.favourites_count"]),
    ..UserStats::default()
  };
  let other = [
    (
      "media",
      v.first_count(&["tweet_counts.media_tweets", "legacy.media_count"]),
    ),
    ("listed", v.count("legacy.listed_count")),
  ];
  for (key, n) in other {
    if let Some(n) = n {
      stats.other.insert(key.into(), n);
    }
  }
  let mut user = User {
    name: v
      .first_str(&["core.name", "legacy.name"])
      .or_else(|| handle.clone())
      .unwrap_or_default(),
    url: handle.as_deref().map(user_url),
    avatar: v.first_str(&["avatar.image_url", "legacy.profile_image_url_https"]),
    bio: v.first_str(&["profile_bio.description", "legacy.description"]),
    verified: v.bool("is_blue_verified").unwrap_or(false)
      || v.bool("verification.verified").unwrap_or(false)
      || v.bool("legacy.verified").unwrap_or(false),
    location: v.first_str(&["location.location", "legacy.location"]),
    stats,
    followed: v
      .bool("relationship_perspectives.following")
      .or_else(|| v.bool("legacy.following")),
    created_at: v
      .first_str(&["core.created_at", "legacy.created_at"])
      .and_then(|t| parse_time(&t)),
    raw: Some(v.clone()),
    id,
    handle,
    ..User::default()
  };
  let extra = &mut user.extra;
  let mut put = |key: &str, value: Option<String>| {
    if let Some(value) = value {
      extra.insert(key.into(), value.into());
    }
  };
  put(
    "website",
    v.first_str(&[
      "profile_bio.entities.url.urls.0.expanded_url",
      "legacy.entities.url.urls.0.expanded_url",
    ]),
  );
  put(
    "banner",
    v.first_str(&["banner.image_url", "legacy.profile_banner_url"]),
  );
  put(
    "pinned_tweet",
    v.first_str(&[
      "pinned_items.tweet_ids_str.0",
      "legacy.pinned_tweet_ids_str.0",
    ]),
  );
  let protected = v
    .bool("privacy.protected")
    .or_else(|| v.bool("legacy.protected"));
  if protected == Some(true) {
    user.extra.insert("protected".into(), true.into());
  }
  Some(user)
}

/// `TweetWithVisibilityResults` wraps the tweet of limited-visibility posts.
fn unwrap(v: &Value) -> &Value {
  match v.at("tweet") {
    t @ Value::Object(_)
      if v.str("__typename").as_deref() == Some("TweetWithVisibilityResults") =>
    {
      t
    }
    _ => v,
  }
}

fn is_tweet(v: &Value) -> bool {
  v.at("legacy").is_object() && v.at("core").is_object()
}

/// A tweet result (`tweet_results.result`); retweets become the original post
/// with `extra.retweeted_by`. `None` for tombstones and unavailable tweets.
pub fn post(v: &Value) -> Option<Post> {
  post_at(v, 0)
}

fn post_at(result: &Value, depth: usize) -> Option<Post> {
  let outer = unwrap(result);
  if depth > 2 || !is_tweet(outer) {
    return None;
  }
  let retweeted = unwrap(outer.at("legacy.retweeted_status_result.result"));
  let (t, retweeter) = if is_tweet(retweeted) {
    (retweeted, user(outer.at("core.user_results.result")))
  } else {
    (outer, None)
  };
  let l = t.at("legacy");
  let author = user(t.at("core.user_results.result"));
  let id = t.str("rest_id").or_else(|| l.str("id_str"))?;
  let handle = author.as_ref().and_then(|a| a.handle.clone());

  let mut post = Post {
    kind: "tweet".into(),
    text: Some(text(t)),
    url: Some(tweet_url(handle.as_deref().unwrap_or("i"), &id)),
    created_at: l.str("created_at").and_then(|s| parse_time(&s)),
    metrics: metrics(t),
    media: media(l),
    tags: l
      .list("entities.hashtags")
      .iter()
      .filter_map(|h| h.str("text"))
      .collect(),
    quoted: post_at(t.at("quoted_status_result.result"), depth + 1).map(Box::new),
    raw: Some(result.clone()),
    id,
    author,
    ..Post::default()
  };
  article(t, &mut post);

  let extra = &mut post.extra;
  let mut put = |key: &str, value: Option<Value>| {
    if let Some(v) = value {
      extra.insert(key.into(), v);
    }
  };
  put("lang", l.str("lang").map(Value::from));
  put(
    "conversation_id",
    l.str("conversation_id_str").map(Value::from),
  );
  put(
    "in_reply_to",
    l.str("in_reply_to_status_id_str").map(Value::from),
  );
  put(
    "in_reply_to_user",
    l.str("in_reply_to_screen_name").map(Value::from),
  );
  if let Some(r) = retweeter {
    put("retweeted_by", r.handle.map(Value::from));
    put("retweet_id", outer.str("rest_id").map(Value::from));
  }
  let urls: Vec<Value> = links(l.at("entities"))
    .into_iter()
    .map(|(_, expanded)| expanded.into())
    .collect();
  if !urls.is_empty() {
    put("urls", Some(urls.into()));
  }
  for (key, flag) in [
    ("liked", "favorited"),
    ("retweeted", "retweeted"),
    ("bookmarked", "bookmarked"),
  ] {
    if l.bool(flag) == Some(true) {
      put(key, Some(true.into()));
    }
  }
  Some(post)
}

fn metrics(t: &Value) -> Metrics {
  let l = t.at("legacy");
  let mut m = Metrics {
    views: t.count("views.count"),
    likes: l.count("favorite_count"),
    comments: l.count("reply_count"),
    shares: l.count("retweet_count"),
    favorites: l.count("bookmark_count"),
    ..Metrics::default()
  };
  if let Some(q) = l.count("quote_count") {
    m.other.insert("quotes".into(), q);
  }
  m
}

/// `(t.co, expanded)` pairs of an entities object.
fn links(entities: &Value) -> Vec<(String, String)> {
  entities
    .list("urls")
    .iter()
    .filter_map(|u| Some((u.str("url")?, u.str("expanded_url")?)))
    .collect()
}

/// Full text: the long note when present, t.co links expanded, media links dropped.
fn text(t: &Value) -> String {
  let (raw, entities) = match t.str("note_tweet.note_tweet_results.result.text") {
    Some(note) => (
      note,
      t.at("note_tweet.note_tweet_results.result.entity_set"),
    ),
    None => (
      t.str("legacy.full_text").unwrap_or_default(),
      t.at("legacy.entities"),
    ),
  };
  let mut text = raw;
  for (short, long) in links(entities) {
    text = text.replace(&short, &long);
  }
  for m in t.list("legacy.entities.media") {
    if let Some(short) = m.str("url") {
      text = text.replace(&short, "");
    }
  }
  text
    .replace("&lt;", "<")
    .replace("&gt;", ">")
    .replace("&amp;", "&")
    .trim()
    .to_owned()
}

/// Long-form article attached to the tweet: title, plain text and cover.
fn article(t: &Value, post: &mut Post) {
  let a = t.at("article.article_results.result");
  if !a.is_object() {
    return;
  }
  post.kind = "article".into();
  post.title = a.str("title");
  if let Some(body) = a.first_str(&["plain_text", "preview_text"]) {
    post.text = Some(body);
  }
  if let Some(cover) = a.str("cover_media.media_info.original_img_url") {
    post.media.insert(0, Media::image(cover));
  }
  if let Some(id) = a.str("rest_id") {
    post.extra.insert("article_id".into(), id.into());
  }
}

/// Photos at original size, videos and GIFs as their best mp4 variant.
fn media(l: &Value) -> Vec<Media> {
  let items = match l.list("extended_entities.media") {
    [] => l.list("entities.media"),
    items => items,
  };
  items.iter().filter_map(media_item).collect()
}

fn media_item(m: &Value) -> Option<Media> {
  let thumb = m.str("media_url_https")?;
  let mut out = match m.str("type").as_deref() {
    Some("video") | Some("animated_gif") => {
      let best = m
        .list("video_info.variants")
        .iter()
        .filter(|v| v.str("content_type").as_deref() == Some("video/mp4"))
        .max_by_key(|v| v.u64("bitrate").unwrap_or(0))
        .and_then(|v| v.str("url"));
      // The variant's own size (`/vid/avc1/1080x1920/`); `original_info` is often scaled.
      let size = best.as_deref().and_then(variant_size);
      match best {
        Some(url) => {
          let kind = if m.str("type").as_deref() == Some("video") {
            MediaKind::Video
          } else {
            MediaKind::Gif
          };
          let mut media = Media::new(kind, url);
          media.duration = m.f64("video_info.duration_millis").map(|ms| ms / 1000.0);
          media.width = size.map(|(w, _)| w);
          media.height = size.map(|(_, h)| h);
          media
        }
        None => Media::image(thumb),
      }
    }
    _ => Media::image(original(&thumb)),
  };
  if out.width.is_none() {
    out.width = m.u64("original_info.width").map(|w| w as u32);
    out.height = m.u64("original_info.height").map(|h| h as u32);
  }
  out.alt = m.str("ext_alt_text");
  Some(out)
}

fn variant_size(url: &str) -> Option<(u32, u32)> {
  url.split('/').find_map(|seg| {
    let (w, h) = seg.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
  })
}

/// `…/media/abc.jpg` → `…/media/abc.jpg?name=orig`.
fn original(url: &str) -> String {
  if url.contains("pbs.twimg.com/media/") && !url.contains('?') {
    format!("{url}?name=orig")
  } else {
    url.to_owned()
  }
}
