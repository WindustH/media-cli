//! Normalized data shared by every platform.
//!
//! Platforms map their upstream payloads into these types, so every command
//! prints the same shape everywhere. Anything that does not fit a common field
//! goes into `extra`; the untouched upstream payload is kept in `raw` and only
//! printed with `--raw`.

use std::collections::BTreeMap;

use jiff::Timestamp;
use serde::Serialize;
use serde_json::Value;

pub type Extra = BTreeMap<String, Value>;

fn is_false(b: &bool) -> bool {
  !*b
}

/// Counters on a post. Platform-only counters (coins, danmaku, quotes, ...) go in `other`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Metrics {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub views: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub likes: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub comments: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub shares: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub favorites: Option<u64>,
  #[serde(flatten)]
  pub other: BTreeMap<String, u64>,
}

impl Metrics {
  pub fn is_empty(&self) -> bool {
    self.views.is_none()
      && self.likes.is_none()
      && self.comments.is_none()
      && self.shares.is_none()
      && self.favorites.is_none()
      && self.other.is_empty()
  }
}

/// Counters on a user.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UserStats {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub followers: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub following: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub posts: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub likes: Option<u64>,
  #[serde(flatten)]
  pub other: BTreeMap<String, u64>,
}

impl UserStats {
  pub fn is_empty(&self) -> bool {
    self.followers.is_none()
      && self.following.is_none()
      && self.posts.is_none()
      && self.likes.is_none()
      && self.other.is_empty()
  }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct User {
  pub id: String,
  pub name: String,
  /// @screen_name, url_token, red id ... whatever the platform shows as a handle.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub handle: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub avatar: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub bio: Option<String>,
  #[serde(skip_serializing_if = "is_false")]
  pub verified: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub location: Option<String>,
  #[serde(skip_serializing_if = "UserStats::is_empty")]
  pub stats: UserStats,
  /// Whether the logged-in account follows this user, when known.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub followed: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub created_at: Option<Timestamp>,
  #[serde(skip_serializing_if = "Extra::is_empty")]
  pub extra: Extra,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub raw: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
  Image,
  Video,
  Audio,
  Gif,
}

#[derive(Debug, Clone, Serialize)]
pub struct Media {
  pub kind: MediaKind,
  pub url: String,
  /// Separate audio track of a split (DASH) video; downloads merge it with ffmpeg.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub audio_url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub width: Option<u32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub height: Option<u32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub duration: Option<f64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub alt: Option<String>,
  /// Bytes of `url` / `audio_url`, when the platform says.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub size: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub audio_size: Option<u64>,
  /// URL query parameter the host takes byte ranges in (`range=a-b`) instead
  /// of the `Range` header; downloads then need `size` / `audio_size`.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub range_param: Option<String>,
}

impl Media {
  pub fn new(kind: MediaKind, url: impl Into<String>) -> Self {
    Self {
      kind,
      url: url.into(),
      audio_url: None,
      width: None,
      height: None,
      duration: None,
      alt: None,
      size: None,
      audio_size: None,
      range_param: None,
    }
  }

  pub fn image(url: impl Into<String>) -> Self {
    Self::new(MediaKind::Image, url)
  }

  pub fn video(url: impl Into<String>) -> Self {
    Self::new(MediaKind::Video, url)
  }
}

/// A piece of content: note, tweet, video, answer, article, question, pin, dynamic ...
#[derive(Debug, Clone, Default, Serialize)]
pub struct Post {
  pub id: String,
  /// Platform content type, e.g. `note`, `tweet`, `video`, `answer`, `question`.
  pub kind: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub text: Option<String>,
  /// Canonical link. Other commands accept it back as a post reference.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub author: Option<User>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub created_at: Option<Timestamp>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub updated_at: Option<Timestamp>,
  #[serde(skip_serializing_if = "Metrics::is_empty")]
  pub metrics: Metrics,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub media: Vec<Media>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub tags: Vec<String>,
  /// Quoted / reposted / parent content.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub quoted: Option<Box<Post>>,
  #[serde(skip_serializing_if = "Extra::is_empty")]
  pub extra: Extra,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub raw: Option<Value>,
}

impl Post {
  /// What other commands should receive to refer back to this post.
  pub fn reference(&self) -> &str {
    self.url.as_deref().unwrap_or(&self.id)
  }
}

impl User {
  /// What other commands should receive to refer back to this user.
  pub fn reference(&self) -> &str {
    self.url.as_deref().unwrap_or(&self.id)
  }
}

impl Collection {
  /// What other commands should receive to refer back to this collection:
  /// folder options take ids, not links.
  pub fn reference(&self) -> &str {
    &self.id
  }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Comment {
  pub id: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub author: Option<User>,
  pub text: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub created_at: Option<Timestamp>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub likes: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub reply_count: Option<u64>,
  /// Name of the user this comment answers, inside a thread.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub reply_to: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub location: Option<String>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub replies: Vec<Comment>,
  #[serde(skip_serializing_if = "Extra::is_empty")]
  pub extra: Extra,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub raw: Option<Value>,
}

/// A named group of content: topic / hashtag, favorites folder, list.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Collection {
  pub id: String,
  /// `topic`, `folder`, `list` ...
  pub kind: String,
  pub name: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub description: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub items: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub followers: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub views: Option<u64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub owner: Option<User>,
  #[serde(skip_serializing_if = "Extra::is_empty")]
  pub extra: Extra,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub raw: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Notification {
  pub id: String,
  /// `like`, `comment`, `mention`, `follow`, `reply`, `system` ...
  pub kind: String,
  pub text: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub actor: Option<User>,
  /// Title or excerpt of the content the notification is about.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub target: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub created_at: Option<Timestamp>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub unread: Option<bool>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub raw: Option<Value>,
}

/// One page of a listing. Pass `next_cursor` back with `--cursor` to continue.
#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
  pub items: Vec<T>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub next_cursor: Option<String>,
  pub has_more: bool,
}

impl<T> Page<T> {
  pub fn new(items: Vec<T>, next_cursor: Option<String>) -> Self {
    let has_more = next_cursor.is_some();
    Self {
      items,
      next_cursor,
      has_more,
    }
  }

  /// A page with nothing after it.
  pub fn last(items: Vec<T>) -> Self {
    Self {
      items,
      next_cursor: None,
      has_more: false,
    }
  }
}

/// Result of a write operation.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Action {
  /// `like`, `unlike`, `follow`, `comment`, `publish`, `delete` ...
  pub action: String,
  pub target: String,
  pub ok: bool,
  /// Id of anything the action created (comment, post ...).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
}

impl Action {
  pub fn done(action: impl Into<String>, target: impl Into<String>) -> Self {
    Self {
      action: action.into(),
      target: target.into(),
      ok: true,
      ..Self::default()
    }
  }

  pub fn with_id(mut self, id: impl Into<String>) -> Self {
    self.id = Some(id.into());
    self
  }

  pub fn with_url(mut self, url: impl Into<String>) -> Self {
    self.url = Some(url.into());
    self
  }

  pub fn with_message(mut self, message: impl Into<String>) -> Self {
    self.message = Some(message.into());
    self
  }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AuthStatus {
  pub authenticated: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub user: Option<Box<User>>,
  /// Where the credential came from: `qrcode`, `cookie`, `browser:chrome`, `env` ...
  #[serde(skip_serializing_if = "Option::is_none")]
  pub source: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub saved_at: Option<Timestamp>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
}

/// Items with a stable id, so pages an upstream repeats can be de-duplicated.
pub trait Keyed {
  fn key(&self) -> &str;
}

macro_rules! keyed {
  ($($t:ty),*) => {$(
    impl Keyed for $t {
      fn key(&self) -> &str {
        &self.id
      }
    }
  )*};
}

keyed!(Post, User, Comment, Collection, Notification);

/// Items with a publication time, which `--since` / `--until` filter on.
pub trait Dated {
  fn date(&self) -> Option<Timestamp>;
}

impl Dated for Post {
  fn date(&self) -> Option<Timestamp> {
    self.created_at
  }
}

impl Dated for Comment {
  fn date(&self) -> Option<Timestamp> {
    self.created_at
  }
}

impl Dated for Notification {
  fn date(&self) -> Option<Timestamp> {
    self.created_at
  }
}

/// A timed line of a transcript / subtitle track.
#[derive(Debug, Clone, Serialize)]
pub struct Cue {
  pub from: f64,
  pub to: f64,
  pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Transcript {
  pub lang: String,
  pub cues: Vec<Cue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Downloaded {
  pub kind: MediaKind,
  pub path: String,
  pub bytes: u64,
}

/// One value of a trend.
#[derive(Debug, Clone, Serialize)]
pub struct Point {
  /// `YYYY-MM-DD` for daily data, RFC 3339 for finer steps.
  pub date: String,
  pub value: Value,
}

/// A metric over time, e.g. daily views.
#[derive(Debug, Clone, Serialize)]
pub struct Series {
  pub metric: String,
  pub points: Vec<Point>,
}

/// One slice of a distribution, e.g. `search` in traffic sources.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Share {
  pub label: String,
  /// Id of what the slice stands for (a post, a region code ...), when it has one.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub id: Option<String>,
  pub value: Value,
  /// Fraction of the whole (0..=1) when the platform reports or implies it.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub ratio: Option<f64>,
}

/// A distribution over one dimension: traffic source, age, gender, region, device ...
#[derive(Debug, Clone, Default, Serialize)]
pub struct Breakdown {
  pub dimension: String,
  /// What the numbers cover when it differs from the insights' `from`..`to`:
  /// `lifetime`, `30d`, `yesterday` ...
  #[serde(skip_serializing_if = "Option::is_none")]
  pub period: Option<String>,
  pub items: Vec<Share>,
}

/// Analytics of the logged-in account or of one of its posts, as the
/// platform's creator center reports them.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Insights {
  /// `account` or `post`.
  pub kind: String,
  /// The account or post the numbers are about.
  pub subject: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
  /// First and last day covered, `YYYY-MM-DD`.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub from: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub to: Option<String>,
  /// What `totals` cover when it differs from `from`..`to` (for a post
  /// usually `lifetime`).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub totals_period: Option<String>,
  /// Headline numbers: `views`, `likes`, `new_followers`, `avg_watch_seconds`,
  /// `completion_rate` ... (snake_case, rates as 0..=1 fractions).
  #[serde(skip_serializing_if = "BTreeMap::is_empty")]
  pub totals: BTreeMap<String, Value>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub series: Vec<Series>,
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub breakdowns: Vec<Breakdown>,
  /// Why data is missing or partial: a paid tier, a creator level, a
  /// follower threshold, a panel that failed ... (sentences for people).
  #[serde(skip_serializing_if = "Vec::is_empty")]
  pub warnings: Vec<String>,
  #[serde(skip_serializing_if = "Extra::is_empty")]
  pub extra: Extra,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub raw: Option<Value>,
}

impl Insights {
  /// Set a headline number; skipped when the platform did not report it.
  pub fn total(&mut self, metric: &str, value: impl Into<Option<Value>>) {
    if let Some(v) = value.into().filter(|v| !v.is_null()) {
      self.totals.insert(metric.to_owned(), v);
    }
  }
}

/// Everything a command can print. Serialized as the envelope's `data`.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Data {
  Posts(Page<Post>),
  Users(Page<User>),
  Comments(Page<Comment>),
  Collections(Page<Collection>),
  Notifications(Page<Notification>),
  Post(Box<Post>),
  User(Box<User>),
  Action(Action),
  Auth(AuthStatus),
  Counts(BTreeMap<String, u64>),
  Transcript(Transcript),
  Downloads(Vec<Downloaded>),
  Insights(Box<Insights>),
  /// Anything else; printed as YAML in the terminal.
  Value(Value),
}

impl Data {
  /// Drop upstream payloads (kept only with `--raw`).
  pub fn strip_raw(&mut self) {
    fn user(u: &mut User) {
      u.raw = None;
    }
    fn post(p: &mut Post) {
      p.raw = None;
      if let Some(a) = &mut p.author {
        user(a);
      }
      if let Some(q) = &mut p.quoted {
        post(q);
      }
    }
    fn comment(c: &mut Comment) {
      c.raw = None;
      if let Some(a) = &mut c.author {
        user(a);
      }
      c.replies.iter_mut().for_each(comment);
    }
    match self {
      Data::Posts(p) => p.items.iter_mut().for_each(post),
      Data::Users(p) => p.items.iter_mut().for_each(user),
      Data::Comments(p) => p.items.iter_mut().for_each(comment),
      Data::Collections(p) => p.items.iter_mut().for_each(|c| {
        c.raw = None;
        if let Some(o) = &mut c.owner {
          user(o);
        }
      }),
      Data::Notifications(p) => p.items.iter_mut().for_each(|n| {
        n.raw = None;
        if let Some(a) = &mut n.actor {
          user(a);
        }
      }),
      Data::Post(p) => post(p),
      Data::User(u) => user(u),
      Data::Auth(a) => {
        if let Some(u) = &mut a.user {
          user(u);
        }
      }
      Data::Insights(i) => i.raw = None,
      _ => {}
    }
  }
}
