//! Xiaohongshu / RedNote for media-cli.
//!
//! - [`api`]: signed transport and error mapping; [`sign`]: x-s / x-s-common / XYW.
//! - [`refs`]: note and user references; [`parse`]: JSON → models; [`page`]: note pages.
//! - Endpoints by domain: [`discover`], [`notes`], [`people`], [`inbox`], [`write`],
//!   [`creator`], [`insights`] (with [`stats`]), [`login`]; platform-only commands in [`extra`].

mod api;
mod creator;
mod discover;
mod events;
mod extra;
mod inbox;
mod insights;
mod login;
mod notes;
mod page;
mod parse;
mod people;
mod refs;
mod sign;
mod stats;
mod write;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::time::Duration;

use media_core::{
  Action, Cap, Choices, Collection, Comment, Ctx, Data, Draft, Insights, Notification, Page,
  PageReq, Platform, PlatformInfo, Post, QrStatus, QrTicket, Query, Result, User,
};

use api::Client;

pub struct Xhs {
  client: Client,
  qr_errors: Cell<u32>,
}

impl Platform for Xhs {
  const INFO: PlatformInfo = PlatformInfo {
    id: "xhs",
    name: "Xiaohongshu",
    aliases: &["xiaohongshu", "rednote"],
    about: "Xiaohongshu / RedNote (小红书)",
    home: api::HOME,
    cookie_domains: &["xiaohongshu.com"],
    required_cookies: &["a1", "web_session"],
    caps: &[
      Cap::QrLogin,
      Cap::Search,
      Cap::SearchUsers,
      Cap::SearchTopics,
      Cap::Hot,
      Cap::Feed,
      Cap::Read,
      Cap::Comments,
      Cap::Replies,
      Cap::User,
      Cap::UserPosts,
      Cap::Favorites,
      Cap::Likes,
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
    ],
    choices: Choices {
      search_sort: &["general", "popular", "latest"],
      search_filter: &["video", "image"],
      hot_category: discover::HOT_CATEGORIES,
      feed_kind: &[],
      comment_sort: &[],
      notification_kind: inbox::KINDS,
    },
    min_interval: Duration::from_secs(1),
    guide: include_str!("../GUIDE.md"),
  };

  type Extra = extra::XhsCommand;

  fn new(ctx: Ctx) -> Result<Self> {
    Ok(Self {
      client: Client::new(ctx),
      qr_errors: Cell::new(0),
    })
  }

  fn ctx(&self) -> &Ctx {
    &self.client.ctx
  }

  async fn whoami(&self) -> Result<User> {
    people::whoami(&self.client).await
  }

  async fn prepare_login(&self) -> Result<()> {
    login::prepare(&self.client);
    Ok(())
  }

  async fn qr_start(&self) -> Result<QrTicket> {
    self.qr_errors.set(0);
    login::qr_start(&self.client).await
  }

  async fn qr_poll(&self, ticket: &QrTicket) -> Result<QrStatus> {
    login::qr_poll(&self.client, ticket, &self.qr_errors).await
  }

  async fn search(&self, query: &Query, page: &PageReq) -> Result<Page<Post>> {
    discover::search(&self.client, query, page).await
  }

  async fn search_users(&self, query: &Query, page: &PageReq) -> Result<Page<User>> {
    creator::search_users(&self.client, query, page).await
  }

  async fn search_topics(&self, query: &Query, page: &PageReq) -> Result<Page<Collection>> {
    creator::search_topics(&self.client, query, page).await
  }

  async fn hot(&self, category: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    discover::hot(&self.client, category, page).await
  }

  async fn feed(&self, kind: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    discover::feed(&self.client, kind, page).await
  }

  async fn read(&self, post: &str) -> Result<Post> {
    notes::read(&self.client, post).await
  }

  async fn comments(
    &self,
    post: &str,
    _sort: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Comment>> {
    notes::comments(&self.client, post, page).await
  }

  async fn replies(&self, post: &str, comment: &str, page: &PageReq) -> Result<Page<Comment>> {
    notes::replies(&self.client, post, comment, page).await
  }

  async fn user(&self, user: &str) -> Result<User> {
    people::user(&self.client, user).await
  }

  async fn user_posts(&self, user: &str, page: &PageReq) -> Result<Page<Post>> {
    people::user_posts(&self.client, user, page).await
  }

  async fn favorites(
    &self,
    user: Option<&str>,
    folder: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Post>> {
    if folder.is_some() {
      return Err(media_core::Error::unsupported("favorites --folder"));
    }
    people::favorites(&self.client, user, page).await
  }

  async fn likes(&self, user: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    people::likes(&self.client, user, page).await
  }

  async fn notifications(&self, kind: Option<&str>, page: &PageReq) -> Result<Page<Notification>> {
    inbox::notifications(&self.client, kind, page).await
  }

  async fn unread(&self) -> Result<BTreeMap<String, u64>> {
    inbox::unread(&self.client).await
  }

  async fn like(&self, post: &str, undo: bool) -> Result<Action> {
    write::like(&self.client, post, undo).await
  }

  async fn favorite(&self, post: &str, folder: Option<&str>, undo: bool) -> Result<Action> {
    write::favorite(&self.client, post, folder, undo).await
  }

  async fn comment(&self, post: &str, text: &str, reply_to: Option<&str>) -> Result<Action> {
    write::comment(&self.client, post, text, reply_to).await
  }

  async fn delete_comment(&self, post: &str, comment: &str) -> Result<Action> {
    write::delete_comment(&self.client, post, comment).await
  }

  async fn follow(&self, user: &str, undo: bool) -> Result<Action> {
    write::follow(&self.client, user, undo).await
  }

  async fn publish(&self, draft: &Draft) -> Result<Action> {
    creator::publish(&self.client, draft).await
  }

  async fn delete(&self, post: &str) -> Result<Action> {
    creator::delete(&self.client, post).await
  }

  async fn insights(&self, post: Option<&str>, days: u32) -> Result<Insights> {
    match post {
      None => insights::account(&self.client, days).await,
      Some(post) => insights::note(&self.client, post, days).await,
    }
  }

  async fn run_extra(&self, command: Self::Extra) -> Result<Data> {
    extra::run(&self.client, command).await
  }
}
