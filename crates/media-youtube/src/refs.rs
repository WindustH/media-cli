//! What users pass in: videos (id, watch / youtu.be / shorts / live / embed
//! links), community posts, channels (UC id, @handle, /channel/, /c/, /user/
//! links), playlists and comments; plus the canonical URLs we hand out.

use media_core::{Error, Result};
use url::Url;

use crate::api::WWW;

pub fn video_url(id: &str) -> String {
  format!("{WWW}/watch?v={id}")
}

pub fn short_url(id: &str) -> String {
  format!("{WWW}/shorts/{id}")
}

pub fn post_url(id: &str) -> String {
  format!("{WWW}/post/{id}")
}

pub fn channel_url(id: &str) -> String {
  format!("{WWW}/channel/{id}")
}

pub fn playlist_url(id: &str) -> String {
  format!("{WWW}/playlist?list={id}")
}

pub fn comment_url(video: &str, comment: &str) -> String {
  format!("{WWW}/watch?v={video}&lc={comment}")
}

fn id_chars(s: &str) -> bool {
  s.bytes()
    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn is_video_id(s: &str) -> bool {
  s.len() == 11 && id_chars(s)
}

pub fn is_channel_id(s: &str) -> bool {
  s.len() == 24 && s.starts_with("UC") && id_chars(s)
}

/// Community post ids: `Ugkx…` (36 characters) or the older `Ugz…` forms.
fn is_post_id(s: &str) -> bool {
  s.starts_with("Ug") && (20..=40).contains(&s.len()) && id_chars(s)
}

fn is_playlist_id(s: &str) -> bool {
  let known = [
    "PL", "UU", "LL", "WL", "FL", "OL", "RD", "UL", "LP", "PU", "EL",
  ];
  (s == "WL" || s == "LL" || (s.len() >= 12 && known.iter().any(|p| s.starts_with(p))))
    && id_chars(s)
}

/// A youtube.com / youtu.be URL, also without its scheme.
fn parse_url(arg: &str) -> Option<Url> {
  let url = if arg.contains("://") {
    Url::parse(arg).ok()?
  } else {
    Url::parse(&format!("https://{arg}")).ok()?
  };
  let host = url.host_str()?.to_ascii_lowercase();
  let ours = ["youtube.com", "youtu.be", "youtube-nocookie.com"]
    .iter()
    .any(|d| host == *d || host.ends_with(&format!(".{d}")));
  ours.then_some(url)
}

fn segments(url: &Url) -> Vec<&str> {
  url
    .path_segments()
    .map(|s| s.filter(|p| !p.is_empty()).collect())
    .unwrap_or_default()
}

fn query(url: &Url, key: &str) -> Option<String> {
  url
    .query_pairs()
    .find(|(k, _)| k == key)
    .map(|(_, v)| v.into_owned())
}

/// A video or a community post.
pub enum PostRef {
  Video(String),
  Post(String),
}

pub fn post(arg: &str) -> Result<PostRef> {
  let arg = arg.trim();
  if is_video_id(arg) {
    return Ok(PostRef::Video(arg.to_owned()));
  }
  if is_post_id(arg) {
    return Ok(PostRef::Post(arg.to_owned()));
  }
  let found = parse_url(arg).and_then(|url| {
    if url.host_str() == Some("youtu.be") {
      return segments(&url)
        .first()
        .filter(|s| is_video_id(s))
        .map(|s| PostRef::Video(s.to_string()));
    }
    if let Some(v) = query(&url, "v").filter(|v| is_video_id(v)) {
      return Some(PostRef::Video(v));
    }
    match segments(&url).as_slice() {
      ["shorts" | "live" | "embed" | "v" | "e", id, ..] if is_video_id(id) => {
        Some(PostRef::Video(id.to_string()))
      }
      ["post", id, ..] if is_post_id(id) => Some(PostRef::Post(id.to_string())),
      [.., "community"] => query(&url, "lb")
        .filter(|p| is_post_id(p))
        .map(PostRef::Post),
      _ => None,
    }
  });
  found.ok_or_else(|| Error::input(format!("not a YouTube video or post id / link: {arg}")))
}

/// Video id of an argument (community posts are refused).
pub fn video(arg: &str) -> Result<String> {
  match post(arg)? {
    PostRef::Video(id) => Ok(id),
    PostRef::Post(_) => Err(Error::input("this works on videos, not community posts")),
  }
}

/// A channel: its id, or a URL YouTube resolves to one.
pub enum ChannelRef {
  Id(String),
  Url(String),
}

pub fn channel(arg: &str) -> Result<ChannelRef> {
  let arg = arg.trim().trim_end_matches('/');
  if is_channel_id(arg) {
    return Ok(ChannelRef::Id(arg.to_owned()));
  }
  let handle = |h: &str| ChannelRef::Url(format!("{WWW}/@{h}"));
  if let Some(h) = arg.strip_prefix('@').filter(|h| is_handle(h)) {
    return Ok(handle(h));
  }
  if let Some(url) = parse_url(arg) {
    let found = match segments(&url).as_slice() {
      ["channel", id, ..] if is_channel_id(id) => Some(ChannelRef::Id(id.to_string())),
      [first, ..] if first.starts_with('@') => Some(handle(first.trim_start_matches('@'))),
      ["c" | "user", name, ..] => Some(ChannelRef::Url(format!(
        "{WWW}/{}/{name}",
        segments(&url)[0]
      ))),
      [name] if !matches!(*name, "watch" | "playlist" | "results" | "feed") => {
        Some(ChannelRef::Url(format!("{WWW}/{name}")))
      }
      _ => None,
    };
    if let Some(r) = found {
      return Ok(r);
    }
  } else if is_handle(arg) {
    return Ok(handle(arg));
  }
  Err(Error::input(format!(
    "not a YouTube channel id, @handle or channel link: {arg}"
  )))
}

/// Handles: 3–30 letters, digits, `_`, `-` and `.`.
fn is_handle(s: &str) -> bool {
  (3..=30).contains(&s.chars().count())
    && s
      .chars()
      .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Playlist id of an id or a playlist / watch link.
pub fn playlist(arg: &str) -> Result<String> {
  let arg = arg.trim();
  let bare = arg
    .strip_prefix("VL")
    .filter(|s| is_playlist_id(s))
    .unwrap_or(arg);
  if is_playlist_id(bare) {
    return Ok(bare.to_owned());
  }
  parse_url(arg)
    .and_then(|u| query(&u, "list"))
    .filter(|l| id_chars(l))
    .ok_or_else(|| Error::input(format!("not a YouTube playlist id or link: {arg}")))
}

/// Comment id of an id or a link with `lc=`.
pub fn comment(arg: &str) -> Result<String> {
  let arg = arg.trim();
  let ok = |s: &str| s.starts_with("Ug") && s.len() >= 20 && s.split('.').all(id_chars);
  if ok(arg) {
    return Ok(arg.to_owned());
  }
  parse_url(arg)
    .and_then(|u| query(&u, "lc"))
    .filter(|c| ok(c))
    .ok_or_else(|| Error::input(format!("not a YouTube comment id or link: {arg}")))
}

/// The thread (top-level comment) a comment id belongs to: replies are `<thread>.<reply>`.
pub fn thread_of(comment: &str) -> &str {
  comment.split('.').next().unwrap_or(comment)
}
