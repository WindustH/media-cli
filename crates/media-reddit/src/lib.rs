//! Reddit for media-cli.
//!
//! Reads Reddit's JSON listings (`www.reddit.com/<path>.json`) anonymously or
//! with the cookies of a browser session, and writes through the classic API
//! with the session's modhash; OAuth "script" app credentials in the
//! environment switch both to `oauth.reddit.com` (see [`api`]). Posts are
//! `t3` things, comments `t1`; collections are subreddits.

mod api;
mod comments;
mod extra;
mod inbox;
mod listing;
mod oauth;
mod parse;
mod posts;
mod refs;
mod subs;
mod upload;
mod users;
mod video;
mod write;

use std::collections::BTreeMap;
use std::time::Duration;

use media_core::{
  Action, Cap, Choices, Collection, Comment, Ctx, Data, Draft, Error, Media, Notification, Page,
  PageReq, Platform, PlatformInfo, Post, Query, Result, User,
};

use crate::api::Api;

pub struct Reddit {
  api: Api,
}

impl Platform for Reddit {
  const INFO: PlatformInfo = PlatformInfo {
    id: "reddit",
    name: "Reddit",
    aliases: &["rd"],
    about: "Reddit: posts, comments, subreddits, users and the inbox",
    home: "https://www.reddit.com",
    cookie_domains: &["reddit.com"],
    required_cookies: &[api::SESSION_COOKIE],
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
      Cap::Collections,
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
    ],
    choices: Choices {
      search_sort: posts::SEARCH_SORTS,
      search_filter: posts::TIME_RANGES,
      hot_category: &["popular", "all"],
      feed_kind: posts::FEED_KINDS,
      comment_sort: comments::SORTS,
      notification_kind: inbox::KINDS,
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
    users::whoami(&self.api).await
  }

  async fn prepare_login(&self) -> Result<()> {
    self.api.reset_account();
    Ok(())
  }

  async fn search(&self, query: &Query, page: &PageReq) -> Result<Page<Post>> {
    posts::search(&self.api, query, page).await
  }

  async fn search_users(&self, query: &Query, page: &PageReq) -> Result<Page<User>> {
    users::search(&self.api, query, page).await
  }

  async fn search_topics(&self, query: &Query, page: &PageReq) -> Result<Page<Collection>> {
    subs::search(&self.api, query, page).await
  }

  async fn hot(&self, category: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    posts::hot(&self.api, category, page).await
  }

  async fn feed(&self, kind: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    posts::feed(&self.api, kind, page).await
  }

  async fn read(&self, post: &str) -> Result<Post> {
    posts::read(&self.api, post).await
  }

  async fn comments(
    &self,
    post: &str,
    sort: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Comment>> {
    comments::list(&self.api, post, sort, page).await
  }

  async fn replies(&self, post: &str, comment: &str, page: &PageReq) -> Result<Page<Comment>> {
    comments::replies(&self.api, post, comment, page).await
  }

  async fn user(&self, user: &str) -> Result<User> {
    users::user(&self.api, user).await
  }

  async fn user_posts(&self, user: &str, page: &PageReq) -> Result<Page<Post>> {
    posts::user_posts(&self.api, user, page).await
  }

  async fn collections(&self, user: Option<&str>, page: &PageReq) -> Result<Page<Collection>> {
    subs::subscriptions(&self.api, user, page).await
  }

  async fn favorites(
    &self,
    user: Option<&str>,
    folder: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Post>> {
    if folder.is_some() {
      return Err(Error::input("Reddit has no folders for saved posts"));
    }
    posts::own(&self.api, user, "saved", page).await
  }

  async fn likes(&self, user: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    posts::own(&self.api, user, "upvoted", page).await
  }

  async fn notifications(&self, kind: Option<&str>, page: &PageReq) -> Result<Page<Notification>> {
    inbox::list(&self.api, kind, page).await
  }

  async fn unread(&self) -> Result<BTreeMap<String, u64>> {
    inbox::unread(&self.api).await
  }

  async fn like(&self, post: &str, undo: bool) -> Result<Action> {
    let (dir, name) = if undo { (0, "unlike") } else { (1, "like") };
    write::vote(&self.api, post, dir, name).await
  }

  async fn favorite(&self, post: &str, folder: Option<&str>, undo: bool) -> Result<Action> {
    write::save(&self.api, post, folder, undo).await
  }

  async fn comment(&self, post: &str, text: &str, reply_to: Option<&str>) -> Result<Action> {
    write::comment(&self.api, post, text, reply_to).await
  }

  async fn delete_comment(&self, _post: &str, comment: &str) -> Result<Action> {
    write::delete_comment(&self.api, comment).await
  }

  async fn follow(&self, user: &str, undo: bool) -> Result<Action> {
    subs::subscribe(&self.api, user, undo).await
  }

  async fn publish(&self, draft: &Draft) -> Result<Action> {
    write::publish(&self.api, draft).await
  }

  async fn delete(&self, post: &str) -> Result<Action> {
    write::delete_post(&self.api, post).await
  }

  async fn media(&self, post: &str, _audio_only: bool) -> Result<(Post, Vec<Media>)> {
    posts::media(&self.api, post).await
  }

  async fn run_extra(&self, command: Self::Extra) -> Result<Data> {
    extra::run(&self.api, command).await
  }
}
