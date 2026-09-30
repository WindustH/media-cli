//! The contract between the shared command set and one platform.
//!
//! A platform describes itself with a static [`PlatformInfo`] and implements
//! the [`Platform`] operations it supports; every other operation keeps its
//! default body, which reports `unsupported_operation`. Commands that only make
//! sense on one platform live in the platform's own `Extra` subcommand enum.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::{
  Action, Collection, Comment, Data, Insights, Media, Notification, Page, Post, User,
};

/// Shared operations a platform can provide. Drives `--help` (unsupported
/// commands are hidden) and the `media platforms` matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
  QrLogin,
  Search,
  SearchUsers,
  SearchTopics,
  Hot,
  Feed,
  Read,
  Comments,
  Replies,
  Likers,
  Reposts,
  User,
  UserPosts,
  Followers,
  Following,
  Collections,
  Favorites,
  Likes,
  History,
  Notifications,
  Unread,
  Like,
  Favorite,
  Comment,
  DeleteComment,
  Follow,
  Publish,
  Delete,
  Download,
  AccountInsights,
  PostInsights,
}

impl Cap {
  pub const ALL: &[Cap] = &[
    Cap::QrLogin,
    Cap::Search,
    Cap::SearchUsers,
    Cap::SearchTopics,
    Cap::Hot,
    Cap::Feed,
    Cap::Read,
    Cap::Comments,
    Cap::Replies,
    Cap::Likers,
    Cap::Reposts,
    Cap::User,
    Cap::UserPosts,
    Cap::Followers,
    Cap::Following,
    Cap::Collections,
    Cap::Favorites,
    Cap::Likes,
    Cap::History,
    Cap::Notifications,
    Cap::Unread,
    Cap::Like,
    Cap::Favorite,
    Cap::Comment,
    Cap::DeleteComment,
    Cap::Follow,
    Cap::Publish,
    Cap::Delete,
    Cap::Download,
    Cap::AccountInsights,
    Cap::PostInsights,
  ];

  pub fn name(self) -> &'static str {
    match self {
      Cap::QrLogin => "qr-login",
      Cap::Search => "search",
      Cap::SearchUsers => "search-users",
      Cap::SearchTopics => "search-topics",
      Cap::Hot => "hot",
      Cap::Feed => "feed",
      Cap::Read => "read",
      Cap::Comments => "comments",
      Cap::Replies => "replies",
      Cap::Likers => "likers",
      Cap::Reposts => "reposts",
      Cap::User => "user",
      Cap::UserPosts => "user-posts",
      Cap::Followers => "followers",
      Cap::Following => "following",
      Cap::Collections => "collections",
      Cap::Favorites => "favorites",
      Cap::Likes => "likes",
      Cap::History => "history",
      Cap::Notifications => "notifications",
      Cap::Unread => "unread",
      Cap::Like => "like",
      Cap::Favorite => "favorite",
      Cap::Comment => "comment",
      Cap::DeleteComment => "delete-comment",
      Cap::Follow => "follow",
      Cap::Publish => "post",
      Cap::Delete => "delete",
      Cap::Download => "download",
      Cap::AccountInsights => "insights",
      Cap::PostInsights => "post-insights",
    }
  }
}

/// Platform-specific values accepted by shared options. An empty list hides the option.
#[derive(Debug, Clone, Copy)]
pub struct Choices {
  /// `search --sort`; the first entry is the default.
  pub search_sort: &'static [&'static str],
  /// `search --filter` (content type filters such as `video`, `image`).
  pub search_filter: &'static [&'static str],
  /// `hot --category`.
  pub hot_category: &'static [&'static str],
  /// `feed --type`.
  pub feed_kind: &'static [&'static str],
  /// `comments --sort`.
  pub comment_sort: &'static [&'static str],
  /// `notifications --type`.
  pub notification_kind: &'static [&'static str],
}

impl Choices {
  pub const NONE: Choices = Choices {
    search_sort: &[],
    search_filter: &[],
    hot_category: &[],
    feed_kind: &[],
    comment_sort: &[],
    notification_kind: &[],
  };
}

/// Static description of a platform.
#[derive(Debug, Clone, Copy)]
pub struct PlatformInfo {
  /// Subcommand name, e.g. `bili`.
  pub id: &'static str,
  /// Display name, e.g. `Bilibili`.
  pub name: &'static str,
  pub aliases: &'static [&'static str],
  /// One-line description for `--help`.
  pub about: &'static str,
  /// Home page; also the `Referer` for media downloads.
  pub home: &'static str,
  /// Cookie domains to read when importing from a browser.
  pub cookie_domains: &'static [&'static str],
  /// Cookies a logged-in session must contain.
  pub required_cookies: &'static [&'static str],
  pub caps: &'static [Cap],
  pub choices: Choices,
  /// Minimum gap between requests (anti-bot pacing); zero disables it.
  pub min_interval: Duration,
  /// Markdown notes appended to the generated `media guide <id>` topic:
  /// reference formats, login notes, analytics, limits.
  pub guide: &'static str,
}

impl PlatformInfo {
  pub fn supports(&self, cap: Cap) -> bool {
    self.caps.contains(&cap)
  }
}

/// Which page of a listing to fetch.
#[derive(Debug, Clone, Default)]
pub struct PageReq {
  /// Opaque cursor from the previous page's `next_cursor`; `None` for the first page.
  pub cursor: Option<String>,
  /// How many more items the caller wants; a hint for the upstream page size.
  pub size: usize,
}

impl PageReq {
  /// Upstream page size: the wanted count clamped to `[1, max]`.
  pub fn size_within(&self, max: usize) -> usize {
    self.size.clamp(1, max)
  }

  /// Numeric cursor (offset / page number) with a default for the first page.
  pub fn number_or(&self, first: u64) -> u64 {
    self
      .cursor
      .as_deref()
      .and_then(|c| c.parse().ok())
      .unwrap_or(first)
  }
}

#[derive(Debug, Clone, Default)]
pub struct Query {
  pub keyword: String,
  /// One of `Choices::search_sort`.
  pub sort: Option<String>,
  /// One of `Choices::search_filter`.
  pub filter: Option<String>,
}

/// Content to publish.
#[derive(Debug, Clone, Default)]
pub struct Draft {
  pub title: Option<String>,
  pub text: String,
  pub images: Vec<PathBuf>,
  /// Post to reply to (threads / replies).
  pub reply_to: Option<String>,
  /// Post to quote / repost with comment.
  pub quote: Option<String>,
  pub topics: Vec<String>,
}

/// A started QR login: `url` is encoded into the QR code, `token` identifies it when polling.
#[derive(Debug, Clone, Default)]
pub struct QrTicket {
  pub url: String,
  pub token: String,
  pub extra: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrStatus {
  Waiting,
  Scanned,
  /// Login finished; the platform has put the session cookies into `ctx.http`.
  Confirmed,
  Expired,
}

macro_rules! unsupported {
  ($op:literal) => {
    Err(Error::unsupported($op))
  };
}

/// Operations of one platform. Post and user arguments arrive already
/// resolved from `#N`; they may be ids or URLs in the platform's own formats.
#[allow(async_fn_in_trait, unused_variables)]
pub trait Platform: Sized {
  const INFO: PlatformInfo;

  /// Platform-only commands, merged next to the shared ones.
  type Extra: clap::Subcommand;

  fn new(ctx: Ctx) -> Result<Self>;

  fn ctx(&self) -> &Ctx;

  // ── account ──────────────────────────────────────────────────────────

  /// The logged-in account; fails with `not_authenticated` when the session is not valid.
  async fn whoami(&self) -> Result<User>;

  /// Whether credentials are present (not whether they still work). Defaults
  /// to the required cookies; platforms with other credentials override it.
  fn logged_in(&self) -> bool {
    let ctx = self.ctx();
    ctx
      .info
      .required_cookies
      .iter()
      .all(|c| ctx.http.has_cookie(c))
  }

  /// Called after cookies were imported (cookie string or browser), before they are
  /// verified and saved. Use it to fetch missing helper cookies.
  async fn prepare_login(&self) -> Result<()> {
    Ok(())
  }

  async fn qr_start(&self) -> Result<QrTicket> {
    unsupported!("login --qrcode")
  }

  async fn qr_poll(&self, ticket: &QrTicket) -> Result<QrStatus> {
    unsupported!("login --qrcode")
  }

  // ── reading ──────────────────────────────────────────────────────────

  async fn search(&self, query: &Query, page: &PageReq) -> Result<Page<Post>> {
    unsupported!("search")
  }

  async fn search_users(&self, query: &Query, page: &PageReq) -> Result<Page<User>> {
    unsupported!("search --type user")
  }

  async fn search_topics(&self, query: &Query, page: &PageReq) -> Result<Page<Collection>> {
    unsupported!("search --type topic")
  }

  async fn hot(&self, category: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    unsupported!("hot")
  }

  async fn feed(&self, kind: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    unsupported!("feed")
  }

  async fn read(&self, post: &str) -> Result<Post> {
    unsupported!("read")
  }

  async fn comments(
    &self,
    post: &str,
    sort: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Comment>> {
    unsupported!("comments")
  }

  async fn replies(&self, post: &str, comment: &str, page: &PageReq) -> Result<Page<Comment>> {
    unsupported!("replies")
  }

  /// Accounts that liked a post (where the platform shows them).
  async fn likers(&self, post: &str, page: &PageReq) -> Result<Page<User>> {
    unsupported!("likers")
  }

  /// Reposts, retweets, quotes or crossposts of a post.
  async fn reposts(&self, post: &str, page: &PageReq) -> Result<Page<Post>> {
    unsupported!("reposts")
  }

  async fn user(&self, user: &str) -> Result<User> {
    unsupported!("user")
  }

  async fn user_posts(&self, user: &str, page: &PageReq) -> Result<Page<Post>> {
    unsupported!("user-posts")
  }

  async fn followers(&self, user: &str, page: &PageReq) -> Result<Page<User>> {
    unsupported!("followers")
  }

  async fn following(&self, user: &str, page: &PageReq) -> Result<Page<User>> {
    unsupported!("following")
  }

  /// Favorites folders, lists and similar containers (`None` = the logged-in account).
  async fn collections(&self, user: Option<&str>, page: &PageReq) -> Result<Page<Collection>> {
    unsupported!("collections")
  }

  /// Saved / bookmarked posts, optionally inside one folder.
  async fn favorites(
    &self,
    user: Option<&str>,
    folder: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Post>> {
    unsupported!("favorites")
  }

  async fn likes(&self, user: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    unsupported!("likes")
  }

  async fn history(&self, page: &PageReq) -> Result<Page<Post>> {
    unsupported!("history")
  }

  async fn notifications(&self, kind: Option<&str>, page: &PageReq) -> Result<Page<Notification>> {
    unsupported!("notifications")
  }

  async fn unread(&self) -> Result<BTreeMap<String, u64>> {
    unsupported!("unread")
  }

  // ── writing ──────────────────────────────────────────────────────────

  async fn like(&self, post: &str, undo: bool) -> Result<Action> {
    unsupported!("like")
  }

  async fn favorite(&self, post: &str, folder: Option<&str>, undo: bool) -> Result<Action> {
    unsupported!("favorite")
  }

  /// Comment on a post, or reply to one of its comments.
  async fn comment(&self, post: &str, text: &str, reply_to: Option<&str>) -> Result<Action> {
    unsupported!("comment")
  }

  async fn delete_comment(&self, post: &str, comment: &str) -> Result<Action> {
    unsupported!("delete-comment")
  }

  async fn follow(&self, user: &str, undo: bool) -> Result<Action> {
    unsupported!("follow")
  }

  async fn publish(&self, draft: &Draft) -> Result<Action> {
    unsupported!("post")
  }

  async fn delete(&self, post: &str) -> Result<Action> {
    unsupported!("delete")
  }

  // ── analytics ────────────────────────────────────────────────────────

  /// Creator-center analytics of the logged-in account (`post` = `None`) or of
  /// one of its posts, over the last `days` days where the platform allows it.
  async fn insights(&self, post: Option<&str>, days: u32) -> Result<Insights> {
    unsupported!("insights")
  }

  // ── media ────────────────────────────────────────────────────────────

  /// The post and the media files to download. Defaults to the media of `read`.
  async fn media(&self, post: &str, audio_only: bool) -> Result<(Post, Vec<Media>)> {
    let post = self.read(post).await?;
    let media = post.media.clone();
    Ok((post, media))
  }

  // ── platform-only commands ───────────────────────────────────────────

  async fn run_extra(&self, command: Self::Extra) -> Result<Data>;
}

/// `Extra` for a platform without commands of its own.
#[derive(Debug, clap::Subcommand)]
pub enum NoExtra {}
