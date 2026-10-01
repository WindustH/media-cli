//! Bilibili for media-cli.
//!
//! Posts are videos (`BV...` / `av...`) or dynamics (`t.bilibili.com/<id>`);
//! users are mids, space links or names. The modules below own one domain
//! each; this file only describes the platform and delegates.

mod account;
mod api;
mod archive;
mod comment;
mod creator;
mod device;
mod dynamic;
mod extra;
mod library;
mod message;
mod page;
mod parse;
mod play;
mod proto;
mod refs;
mod sign;
mod user;
mod video;

use std::collections::BTreeMap;
use std::time::Duration;

use media_core::{
  Action, Cap, Choices, Collection, Comment, Ctx, Data, Draft, Error, Insights, Media,
  Notification, Page, PageReq, Platform, PlatformInfo, Post, QrStatus, QrTicket, Query, Reply,
  Result, User,
};

use crate::refs::PostRef;

pub use extra::Extra;

pub struct Bilibili {
  ctx: Ctx,
}

impl Bilibili {
  async fn post(&self, arg: &str) -> Result<PostRef> {
    refs::post(&self.ctx, arg).await
  }

  async fn section(&self, post: &str) -> Result<comment::Section> {
    comment::section(&self.ctx, &self.post(post).await?).await
  }

  /// Likers and reposts are those of a dynamic; a video's are those of the
  /// dynamic that announced it.
  async fn dynamic_id(&self, post: &str) -> Result<String> {
    match self.post(post).await? {
      PostRef::Dynamic(id) => Ok(id),
      PostRef::Video(v) => dynamic::of_video(&self.ctx, &v).await,
    }
  }
}

impl Platform for Bilibili {
  const INFO: PlatformInfo = PlatformInfo {
    id: "bili",
    name: "Bilibili",
    aliases: &["bilibili", "b23"],
    about: "Bilibili (哔哩哔哩)",
    home: "https://www.bilibili.com",
    cookie_domains: &["bilibili.com"],
    required_cookies: api::LOGIN_COOKIES,
    caps: &[
      Cap::QrLogin,
      Cap::Search,
      Cap::SearchUsers,
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
    ],
    choices: Choices {
      search_sort: &["totalrank", "click", "pubdate", "dm", "stow", "scores"],
      search_filter: &[],
      hot_category: video::HOT_CATEGORIES,
      feed_kind: &["all", "video"],
      comment_sort: &["hot", "time"],
      notification_kind: &["reply", "at", "like"],
    },
    min_interval: Duration::from_millis(300),
    guide: include_str!("../GUIDE.md"),
  };

  type Extra = Extra;

  fn new(mut ctx: Ctx) -> Result<Self> {
    api::configure(&mut ctx.http);
    Ok(Self { ctx })
  }

  fn ctx(&self) -> &Ctx {
    &self.ctx
  }

  async fn whoami(&self) -> Result<User> {
    account::whoami(&self.ctx).await
  }

  async fn prepare_login(&self) -> Result<()> {
    device::ensure(&self.ctx).await;
    Ok(())
  }

  async fn qr_start(&self) -> Result<QrTicket> {
    account::qr_start(&self.ctx).await
  }

  async fn qr_poll(&self, ticket: &QrTicket) -> Result<QrStatus> {
    account::qr_poll(&self.ctx, ticket).await
  }

  async fn search(&self, query: &Query, page: &PageReq) -> Result<Page<Post>> {
    video::search(&self.ctx, query, page).await
  }

  async fn search_users(&self, query: &Query, page: &PageReq) -> Result<Page<User>> {
    video::search_users(&self.ctx, query, page).await
  }

  async fn hot(&self, category: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    video::hot(&self.ctx, category, page).await
  }

  async fn feed(&self, kind: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    dynamic::feed(&self.ctx, kind, page).await
  }

  async fn read(&self, post: &str) -> Result<Post> {
    match self.post(post).await? {
      PostRef::Video(v) => video::read(&self.ctx, &v).await,
      PostRef::Dynamic(id) => dynamic::read(&self.ctx, &id).await,
    }
  }

  async fn comments(
    &self,
    post: &str,
    sort: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Comment>> {
    let section = self.section(post).await?;
    comment::list(&self.ctx, &section, sort, page).await
  }

  async fn replies(&self, post: &str, comment: &str, page: &PageReq) -> Result<Page<Comment>> {
    let section = self.section(post).await?;
    let (root, _) = refs::reply_target(comment)?;
    comment::thread(&self.ctx, &section, &root, page).await
  }

  async fn likers(&self, post: &str, page: &PageReq) -> Result<Page<User>> {
    let id = self.dynamic_id(post).await?;
    dynamic::likers(&self.ctx, &id, page).await
  }

  async fn reposts(&self, post: &str, page: &PageReq) -> Result<Page<Post>> {
    let id = self.dynamic_id(post).await?;
    dynamic::reposts(&self.ctx, &id, page).await
  }

  async fn user(&self, user: &str) -> Result<User> {
    user::profile(&self.ctx, user).await
  }

  async fn user_posts(&self, user: &str, page: &PageReq) -> Result<Page<Post>> {
    let mid = user::mid(&self.ctx, user).await?;
    video::uploads(&self.ctx, &mid, page).await
  }

  async fn followers(&self, user: &str, page: &PageReq) -> Result<Page<User>> {
    user::followers(&self.ctx, user, page).await
  }

  async fn following(&self, user: &str, page: &PageReq) -> Result<Page<User>> {
    user::following(&self.ctx, user, page).await
  }

  async fn collections(&self, user: Option<&str>, _page: &PageReq) -> Result<Page<Collection>> {
    library::collections(&self.ctx, user).await
  }

  async fn favorites(
    &self,
    user: Option<&str>,
    folder: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Post>> {
    library::favorites(&self.ctx, user, folder, page).await
  }

  async fn history(&self, page: &PageReq) -> Result<Page<Post>> {
    library::history(&self.ctx, page).await
  }

  async fn notifications(&self, kind: Option<&str>, page: &PageReq) -> Result<Page<Notification>> {
    message::notifications(&self.ctx, kind, page).await
  }

  async fn unread(&self) -> Result<BTreeMap<String, u64>> {
    message::unread(&self.ctx).await
  }

  async fn like(&self, post: &str, undo: bool) -> Result<Action> {
    match self.post(post).await? {
      PostRef::Video(v) => video::like(&self.ctx, &v, undo).await,
      PostRef::Dynamic(id) => dynamic::like(&self.ctx, &id, undo).await,
    }
  }

  async fn favorite(&self, post: &str, folder: Option<&str>, undo: bool) -> Result<Action> {
    match self.post(post).await? {
      PostRef::Video(v) => library::favorite(&self.ctx, &v, folder, undo).await,
      PostRef::Dynamic(id) => dynamic::favorite(&self.ctx, &id, undo).await,
    }
  }

  async fn comment(&self, post: &str, reply: &Reply) -> Result<Action> {
    self.ctx.require_login(api::LOGIN_COOKIES)?;
    let section = self.section(post).await?;
    comment::add(&self.ctx, &section, &reply.text, reply.reply_to.as_deref()).await
  }

  async fn delete_comment(&self, post: &str, comment: &str) -> Result<Action> {
    self.ctx.require_login(api::LOGIN_COOKIES)?;
    let section = self.section(post).await?;
    comment::delete(&self.ctx, &section, comment).await
  }

  async fn follow(&self, user: &str, undo: bool) -> Result<Action> {
    user::follow(&self.ctx, user, undo).await
  }

  async fn publish(&self, draft: &Draft) -> Result<Action> {
    let quote = match draft.quote.as_deref() {
      None => None,
      Some(q) => match self.post(q).await? {
        PostRef::Dynamic(id) => Some(id),
        PostRef::Video(_) => return Err(Error::input("--quote takes a dynamic, not a video")),
      },
    };
    dynamic::publish(&self.ctx, draft, quote.as_deref()).await
  }

  async fn delete(&self, post: &str) -> Result<Action> {
    match self.post(post).await? {
      PostRef::Dynamic(id) => dynamic::delete(&self.ctx, &id).await,
      PostRef::Video(_) => Err(Error::unsupported("delete (videos)")),
    }
  }

  async fn insights(&self, post: Option<&str>, days: u32) -> Result<Insights> {
    match post {
      None => creator::account(&self.ctx, days).await,
      Some(p) => archive::insights(&self.ctx, &self.post(p).await?, days).await,
    }
  }

  async fn media(&self, post: &str, audio_only: bool) -> Result<(Post, Vec<Media>)> {
    match self.post(post).await? {
      PostRef::Video(v) => play::media(&self.ctx, &v, audio_only).await,
      PostRef::Dynamic(id) => play::dynamic_media(&self.ctx, &id, audio_only).await,
    }
  }

  async fn run_extra(&self, command: Self::Extra) -> Result<Data> {
    extra::run(&self.ctx, command).await
  }
}
