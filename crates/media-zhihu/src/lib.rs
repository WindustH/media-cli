//! Zhihu (知乎) for media-cli.
//!
//! Questions, answers, column articles and pins (想法) over the web API
//! (`www.zhihu.com/api/v4`), authenticated by the `z_c0` cookie. Posts are
//! referred to by URL or typed id (`q:`, `a:`, `p:` for pins, `article:`);
//! a bare number is an answer. Users are referred to by `url_token`.

mod account;
mod api;
mod comments;
mod extra;
mod insights;
mod parse;
mod people;
mod publish;
mod read;
mod refs;
mod sign;
mod write;

use std::collections::BTreeMap;
use std::time::Duration;

use media_core::{
  Action, Cap, Choices, Collection, Comment, Ctx, Data, Draft, Error, Insights, Notification, Page,
  PageReq, Platform, PlatformInfo, Post, QrStatus, QrTicket, Query, Result, User,
};

pub use extra::Extra;
use refs::Target;

pub struct Zhihu {
  ctx: Ctx,
}

impl Platform for Zhihu {
  const INFO: PlatformInfo = PlatformInfo {
    id: "zhihu",
    name: "Zhihu",
    aliases: &["zh"],
    about: "Zhihu (知乎)",
    home: "https://www.zhihu.com",
    cookie_domains: &["zhihu.com"],
    required_cookies: &["z_c0"],
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
      Cap::Likers,
      Cap::User,
      Cap::UserPosts,
      Cap::Followers,
      Cap::Following,
      Cap::Collections,
      Cap::Favorites,
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
      search_sort: &["default", "upvoted", "newest"],
      search_filter: &["answer", "article"],
      comment_sort: &["score", "ts"],
      ..Choices::NONE
    },
    min_interval: Duration::from_millis(600),
    guide: include_str!("../GUIDE.md"),
  };

  type Extra = Extra;

  fn new(mut ctx: Ctx) -> Result<Self> {
    ctx.http.set_header("referer", "https://www.zhihu.com/");
    Ok(Self { ctx })
  }

  fn ctx(&self) -> &Ctx {
    &self.ctx
  }

  async fn whoami(&self) -> Result<User> {
    account::whoami(&self.ctx).await
  }

  async fn prepare_login(&self) -> Result<()> {
    account::helper_cookies(&self.ctx).await
  }

  async fn qr_start(&self) -> Result<QrTicket> {
    account::qr_start(&self.ctx).await
  }

  async fn qr_poll(&self, ticket: &QrTicket) -> Result<QrStatus> {
    account::qr_poll(&self.ctx, ticket).await
  }

  async fn search(&self, query: &Query, page: &PageReq) -> Result<Page<Post>> {
    read::search(&self.ctx, query, page).await
  }

  async fn search_users(&self, query: &Query, page: &PageReq) -> Result<Page<User>> {
    read::search_users(&self.ctx, query, page).await
  }

  async fn search_topics(&self, query: &Query, page: &PageReq) -> Result<Page<Collection>> {
    read::search_topics(&self.ctx, query, page).await
  }

  async fn hot(&self, _category: Option<&str>, _page: &PageReq) -> Result<Page<Post>> {
    read::hot(&self.ctx).await
  }

  async fn feed(&self, _kind: Option<&str>, page: &PageReq) -> Result<Page<Post>> {
    read::feed(&self.ctx, page).await
  }

  async fn read(&self, post: &str) -> Result<Post> {
    read::read(&self.ctx, &Target::parse(post)?).await
  }

  async fn comments(
    &self,
    post: &str,
    sort: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Comment>> {
    comments::roots(&self.ctx, &Target::parse(post)?, sort, page).await
  }

  async fn replies(&self, post: &str, comment: &str, page: &PageReq) -> Result<Page<Comment>> {
    Target::parse(post)?;
    comments::children(&self.ctx, comment, page).await
  }

  async fn likers(&self, post: &str, page: &PageReq) -> Result<Page<User>> {
    people::likers(&self.ctx, &Target::parse(post)?, page).await
  }

  async fn user(&self, user: &str) -> Result<User> {
    people::user(&self.ctx, &refs::user(user)?).await
  }

  async fn user_posts(&self, user: &str, page: &PageReq) -> Result<Page<Post>> {
    people::answers(&self.ctx, &refs::user(user)?, page).await
  }

  async fn followers(&self, user: &str, page: &PageReq) -> Result<Page<User>> {
    people::follows(&self.ctx, &refs::user(user)?, "followers", page).await
  }

  async fn following(&self, user: &str, page: &PageReq) -> Result<Page<User>> {
    people::follows(&self.ctx, &refs::user(user)?, "followees", page).await
  }

  async fn collections(&self, user: Option<&str>, page: &PageReq) -> Result<Page<Collection>> {
    let user = user.map(refs::user).transpose()?;
    people::folders(&self.ctx, user.as_deref(), page).await
  }

  async fn favorites(
    &self,
    user: Option<&str>,
    folder: Option<&str>,
    page: &PageReq,
  ) -> Result<Page<Post>> {
    let folder = match folder {
      Some(f) => f.to_owned(),
      None => {
        let user = user.map(refs::user).transpose()?;
        people::first_folder(&self.ctx, user.as_deref()).await?
      }
    };
    people::folder_items(&self.ctx, &folder, page).await
  }

  async fn notifications(&self, _kind: Option<&str>, page: &PageReq) -> Result<Page<Notification>> {
    people::notifications(&self.ctx, page).await
  }

  async fn unread(&self) -> Result<BTreeMap<String, u64>> {
    account::unread(&self.ctx).await
  }

  async fn like(&self, post: &str, undo: bool) -> Result<Action> {
    write::like(&self.ctx, &Target::parse(post)?, undo).await
  }

  async fn favorite(&self, post: &str, folder: Option<&str>, undo: bool) -> Result<Action> {
    write::favorite(&self.ctx, &Target::parse(post)?, folder, undo).await
  }

  async fn comment(&self, post: &str, text: &str, reply_to: Option<&str>) -> Result<Action> {
    write::comment(&self.ctx, &Target::parse(post)?, text, reply_to).await
  }

  async fn delete_comment(&self, post: &str, comment: &str) -> Result<Action> {
    write::delete_comment(&self.ctx, &Target::parse(post)?, comment).await
  }

  async fn follow(&self, user: &str, undo: bool) -> Result<Action> {
    write::follow(&self.ctx, "members", &refs::user(user)?, undo).await
  }

  /// Publishes a pin (想法); questions and articles have their own commands.
  async fn publish(&self, draft: &Draft) -> Result<Action> {
    if draft.reply_to.is_some() || draft.quote.is_some() || !draft.topics.is_empty() {
      return Err(Error::input(
        "Zhihu pins take a text, --title and --image only; use `ask` / `article` for topics",
      ));
    }
    let title = draft.title.as_deref().unwrap_or_default().trim();
    publish::pin(&self.ctx, title, &draft.text, &draft.images).await
  }

  async fn delete(&self, post: &str) -> Result<Action> {
    write::delete(&self.ctx, &Target::parse(post)?).await
  }

  async fn insights(&self, post: Option<&str>, days: u32) -> Result<Insights> {
    match post {
      Some(post) => insights::post(&self.ctx, &Target::parse(post)?, days).await,
      None => insights::account(&self.ctx, days).await,
    }
  }

  async fn run_extra(&self, command: Self::Extra) -> Result<Data> {
    extra::run(&self.ctx, command).await
  }
}
