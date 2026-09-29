//! Parse the ids and links users pass in: videos (BV / av / URL / b23.tv),
//! dynamics (`t.bilibili.com`, `/opus/`), users (mid / space URL / name),
//! comment threads and favorites folders.

use std::sync::LazyLock;

use media_core::{Ctx, Error, Result};
use regex::Regex;

pub const VIDEO_URL: &str = "https://www.bilibili.com/video/";
pub const DYNAMIC_URL: &str = "https://t.bilibili.com/";
pub const SPACE_URL: &str = "https://space.bilibili.com/";

static BVID: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)\bBV([0-9A-Za-z]{10})\b").expect("valid regex"));
static AID: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)(?:^|/)av(\d+)\b").expect("valid regex"));
static DYNAMIC: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?:t\.bilibili\.com/|/opus/|/dynamic/)(\d+)").expect("valid regex")
});
static SPACE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?:space\.bilibili\.com/|m\.bilibili\.com/space/)(\d+)").expect("valid regex")
});

/// A video, with both ids (they convert into each other) and the part (`?p=`).
#[derive(Debug, Clone)]
pub struct Video {
  pub bvid: String,
  pub aid: u64,
  /// 1-based part number of a multi-part video.
  pub page: usize,
}

impl Video {
  pub fn from_bvid(bvid: &str) -> Option<Self> {
    let bvid = format!("BV{}", bvid.get(2..)?);
    Some(Self {
      aid: bvid_to_aid(&bvid)?,
      bvid,
      page: 1,
    })
  }

  pub fn from_aid(aid: u64) -> Self {
    Self {
      bvid: aid_to_bvid(aid),
      aid,
      page: 1,
    }
  }

  pub fn url(&self) -> String {
    format!("{VIDEO_URL}{}", self.bvid)
  }
}

/// What a post argument points at.
#[derive(Debug, Clone)]
pub enum PostRef {
  Video(Video),
  /// Dynamic / opus id.
  Dynamic(String),
}

/// Parse a post argument; b23.tv short links are resolved over HTTP.
pub async fn post(ctx: &Ctx, input: &str) -> Result<PostRef> {
  let input = input.trim();
  if is_short_link(input) {
    let url = if input.starts_with("http") {
      input.to_owned()
    } else {
      format!("https://{input}")
    };
    let resp = ctx.http.get(url).no_cookies().send().await?;
    return parse_post(&resp.url)
      .ok_or_else(|| Error::input(format!("short link did not lead to a post: {}", resp.url)));
  }
  parse_post(input).ok_or_else(|| {
    Error::input(format!(
      "not a Bilibili video or dynamic: {input} (expected BV id, av id, dynamic id or URL)"
    ))
  })
}

/// A post argument that must be a video.
pub async fn video(ctx: &Ctx, input: &str) -> Result<Video> {
  match post(ctx, input).await? {
    PostRef::Video(v) => Ok(v),
    PostRef::Dynamic(_) => Err(Error::input(format!("not a video: {input}"))),
  }
}

fn is_short_link(s: &str) -> bool {
  let host = s
    .trim_start_matches("https://")
    .trim_start_matches("http://");
  host.starts_with("b23.tv/") || host.starts_with("bili2233.cn/")
}

/// Offline part of [`post`].
pub fn parse_post(input: &str) -> Option<PostRef> {
  let page = query_param(input, "p")
    .and_then(|p| p.parse().ok())
    .unwrap_or(1);
  let video = if let Some(c) = BVID.captures(input) {
    Video::from_bvid(c.get(0)?.as_str())
  } else if let Some(c) = AID.captures(input) {
    c[1].parse().ok().map(Video::from_aid)
  } else {
    None
  };
  if let Some(v) = video {
    return Some(PostRef::Video(Video { page, ..v }));
  }
  if let Some(c) = DYNAMIC.captures(input) {
    return Some(PostRef::Dynamic(c[1].to_owned()));
  }
  // Bare numbers: dynamic ids are 17+ digits, anything shorter is an aid.
  if !input.is_empty() && input.bytes().all(|b| b.is_ascii_digit()) {
    return Some(if input.len() >= 17 {
      PostRef::Dynamic(input.to_owned())
    } else {
      PostRef::Video(Video::from_aid(input.parse().ok()?))
    });
  }
  None
}

/// A user argument: a numeric mid, a space URL, or a name to look up.
pub enum UserRef {
  Mid(u64),
  Name(String),
}

pub fn user(input: &str) -> UserRef {
  let input = input.trim();
  let digits = input
    .strip_prefix("UID:")
    .or_else(|| input.strip_prefix("uid:"))
    .unwrap_or(input);
  if let Ok(mid) = digits.parse() {
    return UserRef::Mid(mid);
  }
  if let Some(mid) = SPACE.captures(input).and_then(|c| c[1].parse().ok()) {
    return UserRef::Mid(mid);
  }
  UserRef::Name(input.trim_start_matches('@').to_owned())
}

pub fn space_url(mid: impl std::fmt::Display) -> String {
  format!("{SPACE_URL}{mid}")
}

/// `ROOT` or `ROOT:PARENT` (reply to a reply inside a thread).
pub fn reply_target(input: &str) -> Result<(String, String)> {
  let (root, parent) = input.split_once([':', '/']).unwrap_or((input, input));
  let ok = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
  if ok(root) && ok(parent) {
    Ok((root.to_owned(), parent.to_owned()))
  } else {
    Err(Error::input(format!(
      "bad comment id `{input}`: expected ROOT or ROOT:PARENT"
    )))
  }
}

/// Favorites folder: a media id or a `favlist?fid=` link.
pub fn folder(input: &str) -> Result<u64> {
  query_param(input, "fid")
    .unwrap_or(input.trim())
    .parse()
    .map_err(|_| Error::input(format!("bad favorites folder id: {input}")))
}

fn query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
  let query = url.split_once('?')?.1;
  query
    .split(['&', '#'])
    .filter_map(|kv| kv.split_once('='))
    .find(|(k, _)| *k == key)
    .map(|(_, v)| v)
}

// ── BV <-> av ─────────────────────────────────────────────────────────────

const XOR_CODE: u64 = 23442827791579;
const MASK_CODE: u64 = 2251799813685247;
const MAX_AID: u64 = 1 << 51;
const ALPHABET: &[u8; 58] = b"FcwAPNKTMug3GV5Lj7EJnHpWsx4tb8haYeviqBz6rkCy12mUSDQX9RdoZf";

pub fn bvid_to_aid(bvid: &str) -> Option<u64> {
  let mut b: Vec<u8> = bvid.bytes().collect();
  if b.len() != 12 {
    return None;
  }
  b.swap(3, 9);
  b.swap(4, 7);
  let mut n: u64 = 0;
  for c in &b[3..] {
    let i = ALPHABET.iter().position(|a| a == c)? as u64;
    n = n.checked_mul(58)?.checked_add(i)?;
  }
  Some((n & MASK_CODE) ^ XOR_CODE)
}

pub fn aid_to_bvid(aid: u64) -> String {
  let mut b = *b"BV1000000000";
  let mut n = (MAX_AID | aid) ^ XOR_CODE;
  let mut i = b.len() - 1;
  while n > 0 && i > 2 {
    b[i] = ALPHABET[(n % 58) as usize];
    n /= 58;
    i -= 1;
  }
  b.swap(3, 9);
  b.swap(4, 7);
  String::from_utf8_lossy(&b).into_owned()
}
