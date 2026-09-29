//! Post and user references: typed ids and the URL forms Zhihu links use.

use media_core::{Error, Result};

use crate::api::WWW;

/// A piece of Zhihu content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
  Question(String),
  Answer(String),
  Article(String),
  Pin(String),
}

impl Target {
  /// `q:<id>`, `a:<id>`, `p:<id>` (pin), `article:<id>`, a bare number (answer) or a URL.
  pub fn parse(arg: &str) -> Result<Self> {
    let arg = arg.trim();
    if let Some((prefix, id)) = arg.split_once(':')
      && is_id(id)
    {
      let id = id.to_owned();
      return match prefix {
        "q" | "question" => Ok(Self::Question(id)),
        "a" | "answer" => Ok(Self::Answer(id)),
        "p" | "pin" => Ok(Self::Pin(id)),
        "article" => Ok(Self::Article(id)),
        _ => Err(bad(arg)),
      };
    }
    if is_id(arg) {
      return Ok(Self::Answer(arg.to_owned()));
    }
    let (host, segments) = split_url(arg).ok_or_else(|| bad(arg))?;
    let find = |key: &str| {
      segments
        .iter()
        .position(|s| *s == key)
        .and_then(|i| segments.get(i + 1))
        .filter(|id| is_id(id))
        .map(|id| (*id).to_owned())
    };
    let target = if host.starts_with("zhuanlan.") {
      find("p").map(Self::Article)
    } else {
      None
    };
    target
      .or_else(|| find("answer").or_else(|| find("answers")).map(Self::Answer))
      .or_else(|| {
        find("question")
          .or_else(|| find("questions"))
          .map(Self::Question)
      })
      .or_else(|| find("pin").or_else(|| find("pins")).map(Self::Pin))
      .or_else(|| find("articles").or_else(|| find("p")).map(Self::Article))
      .ok_or_else(|| bad(arg))
  }

  /// A question given as a bare id, `q:<id>` or a question URL.
  pub fn question(arg: &str) -> Result<String> {
    let arg = arg.trim();
    if is_id(arg) {
      return Ok(arg.to_owned());
    }
    match Self::parse(arg)? {
      Self::Question(id) => Ok(id),
      _ => Err(Error::input(format!("`{arg}` is not a question"))),
    }
  }

  pub fn id(&self) -> &str {
    match self {
      Self::Question(id) | Self::Answer(id) | Self::Article(id) | Self::Pin(id) => id,
    }
  }

  /// Singular content type (`answer`, `article` ...).
  pub fn kind(&self) -> &'static str {
    match self {
      Self::Question(_) => "question",
      Self::Answer(_) => "answer",
      Self::Article(_) => "article",
      Self::Pin(_) => "pin",
    }
  }

  /// Resource segment of the comment API (`answers`, `articles` ...).
  pub fn plural(&self) -> &'static str {
    match self {
      Self::Question(_) => "questions",
      Self::Answer(_) => "answers",
      Self::Article(_) => "articles",
      Self::Pin(_) => "pins",
    }
  }

  pub fn url(&self) -> String {
    match self {
      Self::Question(id) => question_url(id),
      Self::Answer(id) => format!("{WWW}/answer/{id}"),
      Self::Article(id) => article_url(id),
      Self::Pin(id) => format!("{WWW}/pin/{id}"),
    }
  }
}

pub fn question_url(id: &str) -> String {
  format!("{WWW}/question/{id}")
}

pub fn answer_url(id: &str, question: Option<&str>) -> String {
  match question {
    Some(q) => format!("{WWW}/question/{q}/answer/{id}"),
    None => format!("{WWW}/answer/{id}"),
  }
}

pub fn article_url(id: &str) -> String {
  format!("https://zhuanlan.zhihu.com/p/{id}")
}

pub fn people_url(token: &str) -> String {
  format!("{WWW}/people/{token}")
}

/// A user's `url_token` (or member hash id) from `token`, `@token` or a people URL.
pub fn user(arg: &str) -> Result<String> {
  let arg = arg.trim().trim_start_matches('@');
  if is_token(arg) {
    return Ok(arg.to_owned());
  }
  let (_, segments) = split_url(arg).ok_or_else(|| bad_user(arg))?;
  segments
    .iter()
    .position(|s| matches!(*s, "people" | "org" | "members"))
    .and_then(|i| segments.get(i + 1))
    .filter(|t| is_token(t))
    .map(|t| (*t).to_owned())
    .ok_or_else(|| bad_user(arg))
}

fn is_id(s: &str) -> bool {
  !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_token(s: &str) -> bool {
  !s.is_empty()
    && s
      .bytes()
      .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    && !s.contains("zhihu.")
}

/// Host and path segments of a Zhihu URL (scheme optional).
fn split_url(arg: &str) -> Option<(String, Vec<&str>)> {
  let rest = arg
    .strip_prefix("https://")
    .or_else(|| arg.strip_prefix("http://"))
    .unwrap_or(arg);
  let rest = rest.split(['?', '#']).next()?;
  let mut parts = rest.split('/');
  let host = parts.next()?.to_ascii_lowercase();
  if !(host == "zhihu.com" || host.ends_with(".zhihu.com")) {
    return None;
  }
  Some((host, parts.filter(|s| !s.is_empty()).collect()))
}

fn bad(arg: &str) -> Error {
  Error::input(format!(
    "not a Zhihu post: `{arg}` (use a URL, q:<id>, a:<id>, p:<id>, article:<id> or an answer id)"
  ))
}

fn bad_user(arg: &str) -> Error {
  Error::input(format!(
    "not a Zhihu user: `{arg}` (use a url_token or https://www.zhihu.com/people/<token>)"
  ))
}
