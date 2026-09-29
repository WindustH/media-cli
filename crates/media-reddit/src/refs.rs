//! What users pass in: posts (base36 id, `t3_` fullname, www / old / new /
//! np / m / sh reddit.com links, `redd.it`, share and `v.redd.it` links),
//! comments, users and subreddits; plus the canonical URLs we hand out.

use media_core::{Ctx, Error, Result};
use url::Url;

use crate::api::WWW;

pub fn post_url(sub: &str, id: &str) -> String {
  if sub.is_empty() {
    format!("{WWW}/comments/{id}/")
  } else {
    format!("{WWW}/r/{sub}/comments/{id}/")
  }
}

pub fn user_url(name: &str) -> String {
  format!("{WWW}/user/{name}/")
}

pub fn sub_url(name: &str) -> String {
  format!("{WWW}/r/{name}/")
}

/// Base36 id of a post or comment.
fn is_id(s: &str) -> bool {
  (1..=13).contains(&s.len())
    && s
      .bytes()
      .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// User or subreddit name.
fn is_name(s: &str) -> bool {
  (2..=24).contains(&s.len())
    && s
      .bytes()
      .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A reddit.com / redd.it URL, also without its scheme.
fn parse_url(arg: &str) -> Option<Url> {
  let url = if arg.contains("://") {
    Url::parse(arg).ok()?
  } else {
    Url::parse(&format!("https://{arg}")).ok()?
  };
  let host = url.host_str()?;
  let ours = ["reddit.com", "redd.it"]
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

/// Links that reveal their post only through a redirect.
fn is_short(url: &Url) -> bool {
  url.host_str() == Some("v.redd.it") || matches!(segments(url).as_slice(), ["r", _, "s", _, ..])
}

/// Follow a share (`/r/x/s/…`) or `v.redd.it` link; anything else passes through.
pub async fn resolve(ctx: &Ctx, arg: &str) -> Result<String> {
  let arg = arg.trim();
  match parse_url(arg) {
    Some(url) if is_short(&url) => ctx.http.final_url(url.as_str()).await,
    _ => Ok(arg.to_owned()),
  }
}

/// Post id of an argument (short links are resolved first).
pub async fn post(ctx: &Ctx, arg: &str) -> Result<String> {
  let arg = resolve(ctx, arg).await?;
  post_id(&arg).ok_or_else(|| Error::input(format!("not a Reddit post id or link: {arg}")))
}

fn post_id(arg: &str) -> Option<String> {
  let id = arg.strip_prefix("t3_").unwrap_or(arg);
  if is_id(id) {
    return Some(id.to_owned());
  }
  let url = parse_url(arg)?;
  if url.host_str() == Some("redd.it") {
    return segments(&url)
      .first()
      .filter(|s| is_id(s))
      .map(|s| s.to_string());
  }
  thread(&url).map(|(post, _)| post)
}

/// `(post, comment)` of `…/comments/<post>[/<slug>/<comment>]` or `/gallery/<post>`.
fn thread(url: &Url) -> Option<(String, Option<String>)> {
  let segs = segments(url);
  if let Some(i) = segs.iter().position(|s| *s == "comments") {
    let post = segs.get(i + 1).filter(|s| is_id(s))?;
    let comment = segs.get(i + 3).filter(|s| is_id(s)).map(|s| s.to_string());
    return Some((post.to_string(), comment));
  }
  match segs.as_slice() {
    ["gallery", id, ..] if is_id(id) => Some((id.to_string(), None)),
    _ => None,
  }
}

/// Comment id of `abc`, `t1_abc` or a comment permalink.
pub fn comment_id(arg: &str) -> Result<String> {
  let arg = arg.trim();
  let id = arg.strip_prefix("t1_").unwrap_or(arg);
  if is_id(id) {
    return Ok(id.to_owned());
  }
  parse_url(arg)
    .and_then(|u| thread(&u))
    .and_then(|(_, comment)| comment)
    .ok_or_else(|| Error::input(format!("not a Reddit comment id or link: {arg}")))
}

/// Fullname for votes, saves, edits and deletes: `t1_…` for comments
/// (fullname or comment permalink), `t3_…` for posts.
pub async fn thing(ctx: &Ctx, arg: &str) -> Result<String> {
  let arg = resolve(ctx, arg).await?;
  if let Some(id) = arg.strip_prefix("t1_").filter(|id| is_id(id)) {
    return Ok(format!("t1_{id}"));
  }
  if let Some((_, Some(comment))) = parse_url(&arg).and_then(|u| thread(&u)) {
    return Ok(format!("t1_{comment}"));
  }
  post_id(&arg)
    .map(|id| format!("t3_{id}"))
    .ok_or_else(|| Error::input(format!("not a Reddit post or comment: {arg}")))
}

/// User name of `name`, `u/name`, `/user/name` or a profile link.
pub fn user(arg: &str) -> Result<String> {
  let arg = arg.trim();
  let bare = arg.trim_start_matches(['/', '@']).trim_end_matches('/');
  let bare = bare
    .strip_prefix("u/")
    .or_else(|| bare.strip_prefix("user/"))
    .unwrap_or(bare);
  if is_name(bare) {
    return Ok(bare.to_owned());
  }
  let found = parse_url(arg).and_then(|u| match segments(&u).as_slice() {
    ["user" | "u", name, ..] if is_name(name) => Some(name.to_string()),
    _ => None,
  });
  found.ok_or_else(|| Error::input(format!("not a Reddit user name or profile link: {arg}")))
}

/// Subreddit name of `name`, `r/name` or a community link; `u/name` gives
/// the profile's `u_name`.
pub fn subreddit(arg: &str) -> Result<String> {
  let arg = arg.trim();
  let bare = arg.trim_start_matches('/').trim_end_matches('/');
  if bare.starts_with("u/") || bare.starts_with("user/") {
    return user(bare).map(|u| format!("u_{u}"));
  }
  let bare = bare.strip_prefix("r/").unwrap_or(bare);
  if is_name(bare) {
    return Ok(bare.to_owned());
  }
  let found = parse_url(arg).and_then(|u| match segments(&u).as_slice() {
    ["r", name, ..] if is_name(name) => Some(name.to_string()),
    ["user" | "u", name, ..] if is_name(name) => Some(format!("u_{name}")),
    _ => None,
  });
  found.ok_or_else(|| Error::input(format!("not a subreddit name or link: {arg}")))
}

/// What `follow` subscribes to.
pub enum Target {
  Subreddit(String),
  User(String),
}

/// `r/name` and community links are subreddits; anything else is a user.
pub fn target(arg: &str) -> Result<Target> {
  let bare = arg.trim().trim_start_matches('/');
  let community =
    bare.starts_with("r/") || parse_url(arg).is_some_and(|u| segments(&u).first() == Some(&"r"));
  if community {
    subreddit(arg).map(Target::Subreddit)
  } else {
    user(arg).map(Target::User)
  }
}
