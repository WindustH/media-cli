//! Arguments of the shared commands.

use std::path::PathBuf;

use clap::Subcommand;
use jiff::Timestamp;

use crate::error::{Error, Result};
use crate::model::{Dated, Page};
use crate::output::Format;
use crate::paging::{Window, collect, collect_window};
use crate::platform::PageReq;

/// Options available on every command.
#[derive(Debug, Clone, clap::Args)]
pub struct GlobalArgs {
  /// Output format (default: table on a terminal, yaml when piped)
  #[arg(short = 'f', long, global = true, value_enum, env = "MEDIA_OUTPUT")]
  pub format: Option<Format>,
  /// Shortcut for `--format json` (wins over --format and MEDIA_OUTPUT)
  #[arg(long, global = true)]
  pub json: bool,
  /// Shortcut for `--format yaml` (wins over --format and MEDIA_OUTPUT)
  #[arg(long, global = true)]
  pub yaml: bool,
  /// Include the untouched upstream payloads (`raw` fields)
  #[arg(long, global = true)]
  pub raw: bool,
  /// Proxy URL (http, https or socks5); HTTPS_PROXY / ALL_PROXY are honored too
  #[arg(long, global = true, env = "MEDIA_PROXY")]
  pub proxy: Option<String>,
  /// Request timeout in seconds
  #[arg(long, global = true, default_value_t = 30)]
  pub timeout: u64,
  /// Minimum seconds between requests (random jitter is added); slows bulk
  /// jobs down, never below the platform's own pacing
  #[arg(long, global = true, env = "MEDIA_INTERVAL", value_name = "SECS")]
  pub interval: Option<f64>,
  /// Log requests to stderr
  #[arg(short, long, global = true)]
  pub verbose: bool,
}

impl GlobalArgs {
  pub fn output_format(&self) -> Format {
    Format::resolve(if self.json {
      Some(Format::Json)
    } else if self.yaml {
      Some(Format::Yaml)
    } else {
      self.format
    })
  }
}

#[derive(Debug, clap::Args)]
pub struct PageArgs {
  /// Number of items to fetch; pages are followed automatically
  #[arg(short = 'n', long, default_value_t = 20)]
  pub limit: usize,
  /// Continue from the `next_cursor` of an earlier listing
  #[arg(long)]
  pub cursor: Option<String>,
  /// Only items published since then: 7d, 12h, 2026-09-01 or an RFC 3339 time
  #[arg(long, value_parser = crate::text::parse_when, value_name = "WHEN")]
  pub since: Option<Timestamp>,
  /// Only items published until then (same forms as --since)
  #[arg(long, value_parser = crate::text::parse_when, value_name = "WHEN")]
  pub until: Option<Timestamp>,
}

impl PageArgs {
  pub fn window(&self) -> Window {
    Window {
      since: self.since,
      until: self.until,
    }
  }

  /// Follow a listing of dated items (posts, comments, notifications) within `--since` / `--until`.
  pub async fn collect_dated<T: Dated>(
    &self,
    fetch: impl AsyncFnMut(PageReq) -> Result<Page<T>>,
  ) -> Result<Page<T>> {
    collect_window(self.limit, self.cursor.clone(), self.window(), fetch).await
  }

  /// Follow a listing without publication times (users, collections).
  pub async fn collect<T>(
    &self,
    fetch: impl AsyncFnMut(PageReq) -> Result<Page<T>>,
  ) -> Result<Page<T>> {
    if !self.window().is_open() {
      return Err(Error::input(
        "--since / --until only apply to posts, comments and notifications",
      ));
    }
    collect(self.limit, self.cursor.clone(), fetch).await
  }
}

#[derive(Debug, clap::Args)]
#[group(multiple = false)]
pub struct LoginArgs {
  /// Scan a QR code with the mobile app (default when the platform supports it)
  #[arg(long)]
  pub qrcode: bool,
  /// Import cookies from a local browser; all known browsers when no name is given
  #[arg(long, num_args = 0..=1, default_missing_value = "auto", value_name = "BROWSER")]
  pub browser: Option<String>,
  /// Use a cookie header copied from the browser's developer tools (`name=value; ...`)
  #[arg(long, value_name = "COOKIES")]
  pub cookie: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum SearchKind {
  Post,
  User,
  Topic,
}

#[derive(Debug, clap::Args)]
pub struct SearchArgs {
  pub query: String,
  /// What to search for
  #[arg(short = 't', long = "type", value_enum, default_value_t = SearchKind::Post)]
  pub kind: SearchKind,
  /// Result order
  #[arg(short, long)]
  pub sort: Option<String>,
  /// Content filter
  #[arg(long)]
  pub filter: Option<String>,
  #[command(flatten)]
  pub page: PageArgs,
}

#[derive(Debug, clap::Args)]
pub struct PublishArgs {
  /// Text to publish; `-` reads it from stdin
  pub text: Option<String>,
  #[arg(long)]
  pub title: Option<String>,
  /// Attach an image (repeatable)
  #[arg(short = 'i', long = "image", value_name = "PATH")]
  pub images: Vec<PathBuf>,
  /// Publish as a reply to this post
  #[arg(long, value_name = "POST")]
  pub reply_to: Option<String>,
  /// Quote / repost this post
  #[arg(long, value_name = "POST")]
  pub quote: Option<String>,
  /// Attach a topic / hashtag (repeatable)
  #[arg(long = "topic", value_name = "TOPIC")]
  pub topics: Vec<String>,
}

#[derive(Debug, clap::Args)]
pub struct DownloadArgs {
  pub post: String,
  /// Directory to save into
  #[arg(short = 'o', long, default_value = ".")]
  pub dir: PathBuf,
  /// Keep only the audio track
  #[arg(long)]
  pub audio_only: bool,
  /// Also split the audio into WAV segments of this many seconds (16 kHz mono, for speech recognition)
  #[arg(long, value_name = "SECS")]
  pub split: Option<u32>,
}

/// Commands every platform shares. `POST` accepts an id, a URL or `#N` from the
/// last printed list; `USER` likewise.
#[derive(Debug, Subcommand)]
pub enum CommonCommand {
  /// Log in and save the session
  Login(LoginArgs),
  /// Forget the saved session
  Logout,
  /// Check whether the saved session still works (exit code 1 when not)
  Status,
  /// Show the logged-in account
  Whoami,
  /// Search posts, users or topics
  Search(SearchArgs),
  /// Trending content
  Hot {
    #[arg(short, long)]
    category: Option<String>,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Your timeline / recommendations
  Feed {
    #[arg(short = 't', long = "type")]
    kind: Option<String>,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Show one post
  #[command(visible_alias = "show")]
  Read { post: String },
  /// Comments of a post
  Comments {
    post: String,
    #[arg(short, long)]
    sort: Option<String>,
    /// Fetch every comment (ignores --limit)
    #[arg(long)]
    all: bool,
    /// Also fetch every reply under each comment
    #[arg(long)]
    replies: bool,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Accounts that liked a post
  Likers {
    post: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Reposts, retweets, quotes or crossposts of a post
  Reposts {
    post: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Creator analytics of your account, or of one of your posts
  Insights {
    post: Option<String>,
    /// Days of history for trends
    #[arg(short, long, default_value_t = 30)]
    days: u32,
  },
  /// Replies under one comment
  Replies {
    post: String,
    comment: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Show a user profile
  User { user: String },
  /// Posts of a user
  #[command(visible_alias = "posts")]
  UserPosts {
    user: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Followers of a user
  Followers {
    user: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Accounts a user follows
  Following {
    user: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Favorites folders / lists (yours when USER is omitted)
  Collections {
    user: Option<String>,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Saved / bookmarked posts (yours when USER is omitted)
  #[command(visible_alias = "bookmarks")]
  Favorites {
    user: Option<String>,
    /// Only this folder / collection id
    #[arg(long)]
    folder: Option<String>,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Liked posts (yours when USER is omitted)
  Likes {
    user: Option<String>,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Your viewing history
  History {
    #[command(flatten)]
    page: PageArgs,
  },
  /// Your notifications
  Notifications {
    #[arg(short = 't', long = "type")]
    kind: Option<String>,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Unread counters
  Unread,
  /// Like a post
  Like {
    post: String,
    /// Remove the like instead
    #[arg(long)]
    undo: bool,
  },
  /// Remove a like
  Unlike { post: String },
  /// Save a post to favorites / bookmarks
  Favorite {
    post: String,
    /// Folder / collection id
    #[arg(long)]
    folder: Option<String>,
    /// Remove it instead
    #[arg(long)]
    undo: bool,
  },
  /// Remove a post from favorites / bookmarks
  Unfavorite {
    post: String,
    #[arg(long)]
    folder: Option<String>,
  },
  /// Comment on a post, or reply to one of its comments
  Comment {
    post: String,
    text: String,
    /// Reply to this comment id
    #[arg(long, value_name = "COMMENT")]
    reply_to: Option<String>,
  },
  /// Delete one of your comments
  DeleteComment {
    post: String,
    comment: String,
    #[arg(short, long)]
    yes: bool,
  },
  /// Follow a user
  Follow {
    user: String,
    #[arg(long)]
    undo: bool,
  },
  /// Unfollow a user
  Unfollow { user: String },
  /// Publish a post
  #[command(name = "post", visible_alias = "publish")]
  Publish(PublishArgs),
  /// Delete one of your posts
  Delete {
    post: String,
    #[arg(short, long)]
    yes: bool,
  },
  /// Download the images / video / audio of a post
  Download(DownloadArgs),
}
