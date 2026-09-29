//! Media of a post at source resolution: Reddit-hosted videos (`v.redd.it`,
//! the separate audio track is added by [`crate::video`]), galleries, images
//! embedded in text posts, single images and GIF previews.

use media_core::{Media, MediaKind, Value, ValueExt};

const IMAGE_EXT: &[&str] = &[".jpg", ".jpeg", ".png", ".gif", ".webp"];

pub fn of(d: &Value) -> Vec<Media> {
  if let Some(v) = reddit_video(d) {
    return vec![video(v)];
  }
  if d.bool("is_gallery") == Some(true) {
    return gallery(d);
  }
  if d.bool("is_self") == Some(true) {
    return inline(d);
  }
  image(d).or_else(|| preview_video(d)).into_iter().collect()
}

/// `secure_media.reddit_video` (or `media.reddit_video`) of a post.
pub fn reddit_video(d: &Value) -> Option<&Value> {
  ["secure_media.reddit_video", "media.reddit_video"]
    .iter()
    .map(|p| d.at(p))
    .find(|v| v.str("fallback_url").is_some())
}

/// A `reddit_video` object: the best video-only rendition (`fallback_url`).
fn video(v: &Value) -> Media {
  let kind = if v.bool("is_gif") == Some(true) {
    MediaKind::Gif
  } else {
    MediaKind::Video
  };
  let mut m = Media::new(kind, v.str("fallback_url").unwrap_or_default());
  m.width = v.u64("width").map(|w| w as u32);
  m.height = v.u64("height").map(|h| h as u32);
  m.duration = v.f64("duration");
  m
}

/// Gallery items in their order, from `gallery_data` and `media_metadata`.
fn gallery(d: &Value) -> Vec<Media> {
  let meta = d.at("media_metadata");
  d.list("gallery_data.items")
    .iter()
    .filter_map(|item| {
      let mut m = metadata(&item.str("media_id")?, meta)?;
      m.alt = item.str("caption");
      Some(m)
    })
    .collect()
}

/// Images uploaded into a text post.
fn inline(d: &Value) -> Vec<Media> {
  let Value::Object(meta) = d.at("media_metadata") else {
    return Vec::new();
  };
  meta
    .keys()
    .filter_map(|id| metadata(id, d.at("media_metadata")))
    .collect()
}

/// One `media_metadata` entry: `{status, e, m: "image/png", s: {u | gif, x, y}}`.
fn metadata(id: &str, meta: &Value) -> Option<Media> {
  let m = meta.at(id);
  if m.str("status").as_deref() != Some("valid") {
    return None;
  }
  let s = m.at("s");
  let mut media = match m.str("e").as_deref() {
    Some("AnimatedImage") => Media::new(MediaKind::Gif, s.first_str(&["gif", "mp4"])?),
    Some("Image") => {
      // `s.u` is a resized preview; the original sits on i.redd.it.
      let ext = m.str("m").and_then(|mime| {
        let ext = mime.strip_prefix("image/")?.to_owned();
        Some(if ext == "jpeg" { "jpg".to_owned() } else { ext })
      });
      match ext {
        Some(ext) => Media::image(format!("https://i.redd.it/{id}.{ext}")),
        None => Media::image(s.str("u")?),
      }
    }
    _ => return None,
  };
  media.width = s.u64("x").map(|w| w as u32);
  media.height = s.u64("y").map(|h| h as u32);
  Some(media)
}

/// A linked image (i.redd.it, imgur ...), or an imgur `.gifv` as its mp4.
fn image(d: &Value) -> Option<Media> {
  let url = d.first_str(&["url_overridden_by_dest", "url"])?;
  let path = url.split(['?', '#']).next().unwrap_or_default();
  let lower = path.to_ascii_lowercase();
  if lower.ends_with(".gifv") {
    let stem = &path[..path.len() - ".gifv".len()];
    return Some(Media::new(MediaKind::Gif, format!("{stem}.mp4")));
  }
  let is_image =
    d.str("post_hint").as_deref() == Some("image") || IMAGE_EXT.iter().any(|e| lower.ends_with(e));
  if !is_image {
    return None;
  }
  let kind = if lower.ends_with(".gif") {
    MediaKind::Gif
  } else {
    MediaKind::Image
  };
  let mut m = Media::new(kind, url);
  let source = d.at("preview.images.0.source");
  m.width = source.u64("width").map(|w| w as u32);
  m.height = source.u64("height").map(|h| h as u32);
  Some(m)
}

/// The mp4 Reddit made of an external GIF or clip.
fn preview_video(d: &Value) -> Option<Media> {
  let v = d.at("preview.reddit_video_preview");
  v.str("fallback_url").map(|_| video(v))
}
