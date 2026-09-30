//! Arguments of the shared commands.

use std::path::PathBuf;

use clap::Subcommand;
use jiff::Timestamp;

use crate::error::{Error, Result};
use crate::model::{Dated, Keyed, Page};
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
  pub async fn collect_dated<T: Dated + Keyed>(
    &self,
    fetch: impl AsyncFnMut(PageReq) -> Result<Page<T>>,
  ) -> Result<Page<T>> {
    let page = collect_window(self.limit, self.cursor.clone(), self.window(), fetch).await?;
    Ok(dedup(page))
  }

  /// Follow a listing without publication times (users, collections).
  pub async fn collect<T: Keyed>(
    &self,
    fetch: impl AsyncFnMut(PageReq) -> Result<Page<T>>,
  ) -> Result<Page<T>> {
    if !self.window().is_open() {
      return Err(Error::input(
        "--since / --until only apply to posts, comments and notifications",
      ));
    }
    Ok(dedup(
      collect(self.limit, self.cursor.clone(), fetch).await?,
    ))
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
  #[command(after_long_help = EX_LOGIN)]
  Login(LoginArgs),
  /// Forget the saved session
  Logout,
  /// Check whether the saved session still works (exit code 1 when not)
  Status,
  /// Show the logged-in account
  Whoami,
  /// Search posts, users or topics
  #[command(after_long_help = EX_SEARCH)]
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
  #[command(after_long_help = EX_COMMENTS)]
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
  #[command(after_long_help = EX_INSIGHTS)]
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
  /// Posts of a user (yours when USER is omitted)
  #[command(visible_alias = "posts", after_long_help = EX_USER_POSTS)]
  UserPosts {
    user: Option<String>,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Followers of a user (yours when USER is omitted)
  Followers {
    user: Option<String>,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Accounts a user follows (yours when USER is omitted)
  Following {
    user: Option<String>,
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
  #[command(name = "post", visible_alias = "publish", after_long_help = EX_POST)]
  Publish(PublishArgs),
  /// Delete one of your posts
  Delete {
    post: String,
    #[arg(short, long)]
    yes: bool,
  },
  /// Download the images / video / audio of a post
  #[command(after_long_help = EX_DOWNLOAD)]
  Download(DownloadArgs),
}

/// Drop items an upstream repeated across pages (same id), keeping the first.
fn dedup<T: Keyed>(mut page: Page<T>) -> Page<T> {
  let mut seen = std::collections::HashSet::new();
  page
    .items
    .retain(|i| i.key().is_empty() || seen.insert(i.key().to_owned()));
  page
}

// ── examples for `--help` ────────────────────────────────────────────────

const EX_LOGIN: &str = "\
Examples:
  media bili login                      # QR code for the Bilibili app
  media x login --browser               # reuse a logged-in browser
  media x login --browser firefox
  media zhihu login --cookie 'z_c0=...; _xsrf=...'

More: media guide login";

const EX_SEARCH: &str = "\
Examples:
  media bili search \"rust 教程\" -n 10
  media yt search \"rust\" --sort popularity --filter week
  media xhs search 咖啡 --sort popular --filter video
  media x search \"AI agent\" --sort latest --since 1d -f csv > tweets.csv
  media bili search 影视飓风 -t user
  media zhihu search rust -t topic

Values of --sort / --filter differ per platform: see `media guide <platform>`.";

const EX_COMMENTS: &str = "\
Examples:
  media bili comments BV1xx411c7mD -n 50
  media bili comments '#1' --sort time
  media reddit comments 1wsxldl --all --replies -f jsonl > thread.jsonl
  media yt comments dQw4w9WgXcQ -n 500 --since 30d -f csv > comments.csv

--all fetches every comment; --replies also every reply under each (CSV and
JSON Lines rows carry depth and parent_id). More: media guide analysis";

const EX_INSIGHTS: &str = "\
Examples:
  media bili insights                   # your account, last 30 days
  media bili insights --days 7 -f csv > week.csv
  media bili insights BV1xx411c7mD      # one of your videos
  media x insights 2104695667180380260  # one of your tweets

Totals, daily series and breakdowns (traffic sources, audience ...); warnings
say what a platform withholds. Others' posts get public counters.
More: media guide analysis, media guide <platform>";

const EX_USER_POSTS: &str = "\
Examples:
  media bili user-posts 946974 -n 100 --since 90d -f csv > videos.csv
  media x user-posts NASA -n 50
  media yt user-posts @NASA -n 20
  media bili user-posts                 # your own videos";

const EX_POST: &str = "\
Examples:
  media x post \"Hello\" -i photo.jpg
  media x post \"Nice!\" --reply-to 2104695667180380260
  media zhihu post \"今天的想法\" -i a.png -i b.png
  media xhs post \"正文 #话题\" --title 标题 -i cover.jpg
  media reddit post \"Body text\" --title Title --topic r/test
  cat draft.md | media bili post -

What `post` creates differs per platform: see `media guide interact`.";

const EX_DOWNLOAD: &str = "\
Examples:
  media bili download BV1xx411c7mD -o ~/Videos
  media bili download BV1xx411c7mD --audio-only --split 25
  media xhs download '#1'
  media yt download dQw4w9WgXcQ --audio-only

Needs ffmpeg for merging video and audio, audio extraction and --split.
More: media guide download";
