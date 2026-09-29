//! YouTube for media-cli.
//!
//! Talks to InnerTube, the API behind youtube.com (`/youtubei/v1/...`), as
//! the web client, anonymously or with the cookies of a browser session
//! (signed with `SAPISIDHASH`); streams and captions come from the Android
//! VR client (see [`api`]). Posts are videos, Shorts and community posts;
//! users are channels (`UC…` ids, @handles or links); collections are
//! playlists.

mod account;
mod api;
mod browse;
mod caption;
mod channel;
mod comment;
mod extra;
mod library;
mod notify;
mod parse;
mod proto;
mod refs;
mod search;
mod sign;
mod stream;
mod video;
mod write;

use std::collections::BTreeMap;
use std::time::Duration;

use media_core::{
  Action, Cap, Choices, Collection, Comment, Ctx, Data, Media, Notification, Page, PageReq,
  Platform, PlatformInfo, Post, Query, Result, User,
};

use crate::api::Api;
use crate::channel::Tab;

pub struct YouTube {
  api: Api,
}

impl Platform for YouTube {
  const INFO: PlatformInfo = PlatformInfo {
    id: "youtube",
    name: "YouTube",
    aliases: &["yt"],
    about: "YouTube: videos, Shorts, channels, comments, playlists and transcripts",
    home: "https://www.youtube.com",
    cookie_domains: &["youtube.com"],
    required_cookies: &["SAPISID"],
    caps: &[
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
      Cap::Download,
    ],
    choices: Choices {
      search_sort: search::SORTS,
      search_filter: search::FILTERS,
      hot_category: video::HOT_CATEGORIES,
      feed_kind: library::FEED_KINDS,
      comment_sort: comment::SORTS,
      notification_kind: &[],
    },
    min_interval: Duration::from_millis(1000),
  };

  type Extra = extra::Command;

  fn new(ctx: Ctx) -> Result<Self> {
    Ok(Self { api: Api::new(ctx) })
  }

  fn ctx(&self) -> &Ctx {
    &self.api.ctx
  }

  async fn whoami(&self) -> Result<User> {
    account::whoami(&self.api).await
  }

  fn logged_in(&self) -> bool {
    self.api.logged_in()
  }

  /// Some cookie exports lack `SAPISID`; `__Secure-3PAPISID` holds the same secret.
  async fn prepare_login(&self) -> Result<()> {
    let http = &self.api.ctx.http;
    if !http.has_cookie("SAPISID")
      && let Some(v) = http.cookie("__Secure-3PAPISID")
    {
      http.set_cookie("SAPISID", &v);
    }
    account::reset(&self.api);
    Ok(())
  }

  async fn search(&self, query: &Query, page: &PageReq) -> Result<Page<Post>> {
    search::videos(&self.api, query, page).await
  }

  async fn search_users(&self, query: &Query, page: &PageReq) -> Result<Page<User>> {
    search::channels(&self.api, query, page).await
  }

  async fn search_topics(&self, query: &Query, page: &PageReq) -> Result<Page<Collection>> {
    search::playlists(&self.api, query, page).await
  }

  async fn hot(&self, category: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    video::hot(&self.api, category, page).await
  }

  async fn feed(&self, kind: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    library::feed(&self.api, kind, page).await
  }

  async fn read(&self, post: &str) -> Result<Post> {
    video::read(&self.api, post).await
  }

  async fn comments(
    &self,
    post: &str,
    sort: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Comment>> {
    comment::list(&self.api, post, sort, page).await
  }

  async fn replies(&self, post: &str, comment: &str, page: &PageReq) -> Result<Page<Comment>> {
    comment::replies(&self.api, post, comment, page).await
  }

  async fn user(&self, user: &str) -> Result<User> {
    if account::is_me(user) {
      return account::whoami(&self.api).await;
    }
    channel::profile(&self.api, user).await
  }

  async fn user_posts(&self, user: &str, page: &PageReq) -> Result<Page<Post>> {
    let l = channel::tab(&self.api, user, Tab::Videos, page).await?;
    Ok(Page::new(l.posts, l.next))
  }

  async fn following(&self, user: &str, page: &PageReq) -> Result<Page<User>> {
    library::following(&self.api, user, page).await
  }

  async fn collections(&self, user: Option<&str>, page: &PageReq) -> Result<Page<Collection>> {
    library::collections(&self.api, user, page).await
  }

  async fn favorites(
    &self,
    user: Option<&str>,
    folder: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Post>> {
    library::favorites(&self.api, user, folder, page).await
  }

  async fn likes(&self, user: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    library::likes(&self.api, user, page).await
  }

  async fn history(&self, page: &PageReq) -> Result<Page<Post>> {
    library::history(&self.api, page).await
  }

  async fn notifications(&self, _kind: Option<&str>, page: &PageReq) -> Result<Page<Notification>> {
    notify::list(&self.api, page).await
  }

  async fn unread(&self) -> Result<BTreeMap<String, u64>> {
    notify::unread(&self.api).await
  }

  async fn like(&self, post: &str, undo: bool) -> Result<Action> {
    write::like(&self.api, post, undo).await
  }

  async fn favorite(&self, post: &str, folder: Option<&str>, undo: bool) -> Result<Action> {
    write::favorite(&self.api, post, folder, undo).await
  }

  async fn comment(&self, post: &str, text: &str, reply_to: Option<&str>) -> Result<Action> {
    comment::add(&self.api, post, text, reply_to).await
  }

  async fn delete_comment(&self, post: &str, comment: &str) -> Result<Action> {
    comment::delete(&self.api, post, comment).await
  }

  async fn follow(&self, user: &str, undo: bool) -> Result<Action> {
    write::follow(&self.api, user, undo).await
  }

  async fn media(&self, post: &str, audio_only: bool) -> Result<(Post, Vec<Media>)> {
    stream::media(&self.api, post, audio_only).await
  }

  async fn run_extra(&self, command: Self::Extra) -> Result<Data> {
    extra::run(&self.api, command).await
  }
}
