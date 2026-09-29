//! The shared command set and how it is dispatched to a [`Platform`].
//!
//! Every platform gets the same subcommands; the ones it does not support are
//! hidden from `--help`, and platform-specific values (sort orders, hot
//! categories ...) are injected into the shared options from [`Choices`].

use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgMatches, FromArgMatches, Subcommand, builder::PossibleValuesParser};

use crate::account;
use crate::download::{self, DownloadOpts};
use crate::error::{Error, Result};
use crate::http::{Http, HttpConfig};
use crate::model::Data;
use crate::output::{self, Format};
use crate::paging::collect;
use crate::platform::{Cap, Ctx, Draft, Platform, PlatformInfo, Query};
use crate::store::{RefKind, Store};

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
    #[command(flatten)]
    page: PageArgs,
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

/// Which capability a shared subcommand needs; `None` means always available.
fn required_cap(name: &str) -> Option<&'static [Cap]> {
  Some(match name {
    "search" => &[Cap::Search, Cap::SearchUsers, Cap::SearchTopics],
    "hot" => &[Cap::Hot],
    "feed" => &[Cap::Feed],
    "read" => &[Cap::Read],
    "comments" => &[Cap::Comments],
    "replies" => &[Cap::Replies],
    "user" => &[Cap::User],
    "user-posts" => &[Cap::UserPosts],
    "followers" => &[Cap::Followers],
    "following" => &[Cap::Following],
    "collections" => &[Cap::Collections],
    "favorites" => &[Cap::Favorites],
    "likes" => &[Cap::Likes],
    "history" => &[Cap::History],
    "notifications" => &[Cap::Notifications],
    "unread" => &[Cap::Unread],
    "like" | "unlike" => &[Cap::Like],
    "favorite" | "unfavorite" => &[Cap::Favorite],
    "comment" => &[Cap::Comment],
    "delete-comment" => &[Cap::DeleteComment],
    "follow" | "unfollow" => &[Cap::Follow],
    "post" => &[Cap::Publish],
    "delete" => &[Cap::Delete],
    "download" => &[Cap::Download],
    _ => return None,
  })
}

fn with_choices(
  cmd: clap::Command,
  sub: &str,
  arg: &str,
  values: &'static [&'static str],
) -> clap::Command {
  cmd.mut_subcommand(sub, |c| {
    c.mut_arg(arg, |a| {
      if values.is_empty() {
        a.hide(true)
      } else {
        a.value_parser(PossibleValuesParser::new(values.iter().copied()))
      }
    })
  })
}

/// The clap command of one platform: shared commands plus its extras.
pub fn command<P: Platform>() -> clap::Command {
  let info = P::INFO;
  let mut cmd = clap::Command::new(info.id);
  cmd = CommonCommand::augment_subcommands(cmd);
  cmd = <P::Extra as Subcommand>::augment_subcommands(cmd);
  // Set after augmenting: derived enums would overwrite `about` with their doc comment.
  cmd = cmd
    .about(info.about)
    .long_about(None)
    .visible_aliases(info.aliases.iter().copied())
    .subcommand_required(true)
    .arg_required_else_help(true);
  let names: Vec<String> = cmd
    .get_subcommands()
    .map(|s| s.get_name().to_owned())
    .collect();
  for name in names {
    if let Some(caps) = required_cap(&name)
      && !caps.iter().any(|c| info.supports(*c))
    {
      cmd = cmd.mut_subcommand(&name, |c| c.hide(true));
    }
  }
  let ch = info.choices;
  cmd = with_choices(cmd, "search", "sort", ch.search_sort);
  cmd = with_choices(cmd, "search", "filter", ch.search_filter);
  cmd = with_choices(cmd, "hot", "category", ch.hot_category);
  cmd = with_choices(cmd, "feed", "kind", ch.feed_kind);
  cmd = with_choices(cmd, "comments", "sort", ch.comment_sort);
  with_choices(cmd, "notifications", "kind", ch.notification_kind)
}

/// Names of the platform-only commands.
pub fn extra_commands<P: Platform>() -> Vec<String> {
  <P::Extra as Subcommand>::augment_subcommands(clap::Command::new(""))
    .get_subcommands()
    .map(|s| s.get_name().to_owned())
    .collect()
}

/// Run one platform command and print the result.
pub async fn run<P: Platform>(global: GlobalArgs, matches: ArgMatches) -> ExitCode {
  let format = global.output_format();
  match execute::<P>(&global, &matches).await {
    Ok((mut data, code)) => {
      if !global.raw {
        data.strip_raw();
      }
      output::emit(format, Some(P::INFO.id), &data);
      code
    }
    Err(e) => {
      output::emit_error(format, Some(P::INFO.id), &e);
      ExitCode::from(e.code.exit_code())
    }
  }
}

async fn execute<P: Platform>(
  global: &GlobalArgs,
  matches: &ArgMatches,
) -> Result<(Data, ExitCode)> {
  let info: PlatformInfo = P::INFO;
  let store = Store::new(info.id);
  let loaded = account::load_session(&info, &store)?;
  let config = HttpConfig {
    proxy: global.proxy.clone(),
    timeout: std::time::Duration::from_secs(global.timeout),
    min_interval: info.min_interval,
  };
  let http = Http::new(&config, loaded.session.cookies.clone())?;
  let platform = P::new(Ctx::new(http, store, loaded.session.extra.clone()))?;

  let name = matches.subcommand_name().unwrap_or_default();
  let (data, code, persist) = if CommonCommand::has_subcommand(name) {
    let cmd = CommonCommand::from_arg_matches(matches).map_err(|e| Error::input(e.to_string()))?;
    common(&platform, cmd, &loaded).await?
  } else {
    let cmd = <P::Extra as FromArgMatches>::from_arg_matches(matches)
      .map_err(|e| Error::input(e.to_string()))?;
    (platform.run_extra(cmd).await?, ExitCode::SUCCESS, true)
  };

  let ctx = platform.ctx();
  // Only logged-in sessions are saved; guest cookies are not worth keeping.
  let logged_in = info.required_cookies.iter().all(|c| ctx.http.has_cookie(c));
  if persist && logged_in && !loaded.from_env && ctx.session_changed() {
    let mut session = ctx.session();
    session.source = loaded.session.source.clone();
    session.saved_at = loaded.session.saved_at;
    ctx.store.save_session(&session)?;
  }
  match &data {
    Data::Posts(page) => ctx.store.remember(
      RefKind::Post,
      page
        .items
        .iter()
        .map(|p| p.reference().to_owned())
        .collect(),
    ),
    Data::Users(page) => ctx.store.remember(
      RefKind::User,
      page
        .items
        .iter()
        .map(|u| u.reference().to_owned())
        .collect(),
    ),
    _ => {}
  }
  Ok((data, code))
}

fn confirm(yes: bool, what: &str) -> Result<()> {
  if yes {
    return Ok(());
  }
  if !std::io::stdin().is_terminal() {
    return Err(Error::input(format!("refusing to {what} without --yes")));
  }
  eprint!("{what}? [y/N] ");
  let mut answer = String::new();
  std::io::stdin().read_line(&mut answer)?;
  if matches!(answer.trim(), "y" | "Y" | "yes") {
    Ok(())
  } else {
    Err(Error::input("cancelled"))
  }
}

fn read_text(text: Option<String>) -> Result<String> {
  match text.as_deref() {
    Some("-") => {
      let mut buf = String::new();
      std::io::stdin().read_to_string(&mut buf)?;
      Ok(buf.trim_end().to_owned())
    }
    Some(t) => Ok(t.to_owned()),
    None => Ok(String::new()),
  }
}

/// Returns the data, the exit code and whether a changed session should be saved.
async fn common<P: Platform>(
  p: &P,
  cmd: CommonCommand,
  loaded: &account::Loaded,
) -> Result<(Data, ExitCode, bool)> {
  use CommonCommand as C;
  let ctx = p.ctx();
  let ok = |data: Data| Ok((data, ExitCode::SUCCESS, true));
  match cmd {
    C::Login(args) => account::login(p, args)
      .await
      .map(|d| (d, ExitCode::SUCCESS, false)),
    C::Logout => account::logout::<P>(ctx).map(|d| (d, ExitCode::SUCCESS, false)),
    C::Status => account::status(p, loaded)
      .await
      .map(|(d, code)| (d, code, true)),
    C::Whoami => ok(Data::User(Box::new(p.whoami().await?))),
    C::Search(a) => {
      let q = Query {
        keyword: a.query,
        sort: a.sort,
        filter: a.filter,
      };
      let (limit, cursor) = (a.page.limit, a.page.cursor);
      ok(match a.kind {
        SearchKind::Post => {
          Data::Posts(collect(limit, cursor, async |r| p.search(&q, &r).await).await?)
        }
        SearchKind::User => {
          Data::Users(collect(limit, cursor, async |r| p.search_users(&q, &r).await).await?)
        }
        SearchKind::Topic => {
          Data::Collections(collect(limit, cursor, async |r| p.search_topics(&q, &r).await).await?)
        }
      })
    }
    C::Hot { category, page } => {
      let category = category.as_deref();
      ok(Data::Posts(
        collect(page.limit, page.cursor, async |r| p.hot(category, &r).await).await?,
      ))
    }
    C::Feed { kind, page } => {
      let kind = kind.as_deref();
      ok(Data::Posts(
        collect(page.limit, page.cursor, async |r| p.feed(kind, &r).await).await?,
      ))
    }
    C::Read { post } => ok(Data::Post(Box::new(p.read(&ctx.post_ref(&post)?).await?))),
    C::Comments {
      post,
      sort,
      all,
      page,
    } => {
      let (post, sort) = (ctx.post_ref(&post)?, sort.as_deref());
      let limit = if all { usize::MAX } else { page.limit };
      ok(Data::Comments(
        collect(limit, page.cursor, async |r| {
          p.comments(&post, sort, &r).await
        })
        .await?,
      ))
    }
    C::Replies {
      post,
      comment,
      page,
    } => {
      let post = ctx.post_ref(&post)?;
      ok(Data::Comments(
        collect(page.limit, page.cursor, async |r| {
          p.replies(&post, &comment, &r).await
        })
        .await?,
      ))
    }
    C::User { user } => ok(Data::User(Box::new(p.user(&ctx.user_ref(&user)?).await?))),
    C::UserPosts { user, page } => {
      let user = ctx.user_ref(&user)?;
      ok(Data::Posts(
        collect(page.limit, page.cursor, async |r| {
          p.user_posts(&user, &r).await
        })
        .await?,
      ))
    }
    C::Followers { user, page } => {
      let user = ctx.user_ref(&user)?;
      ok(Data::Users(
        collect(page.limit, page.cursor, async |r| {
          p.followers(&user, &r).await
        })
        .await?,
      ))
    }
    C::Following { user, page } => {
      let user = ctx.user_ref(&user)?;
      ok(Data::Users(
        collect(page.limit, page.cursor, async |r| {
          p.following(&user, &r).await
        })
        .await?,
      ))
    }
    C::Collections { user, page } => {
      let user = user.map(|u| ctx.user_ref(&u)).transpose()?;
      let user = user.as_deref();
      ok(Data::Collections(
        collect(page.limit, page.cursor, async |r| {
          p.collections(user, &r).await
        })
        .await?,
      ))
    }
    C::Favorites { user, folder, page } => {
      let user = user.map(|u| ctx.user_ref(&u)).transpose()?;
      let (user, folder) = (user.as_deref(), folder.as_deref());
      ok(Data::Posts(
        collect(page.limit, page.cursor, async |r| {
          p.favorites(user, folder, &r).await
        })
        .await?,
      ))
    }
    C::Likes { user, page } => {
      let user = user.map(|u| ctx.user_ref(&u)).transpose()?;
      let user = user.as_deref();
      ok(Data::Posts(
        collect(page.limit, page.cursor, async |r| p.likes(user, &r).await).await?,
      ))
    }
    C::History { page } => ok(Data::Posts(
      collect(page.limit, page.cursor, async |r| p.history(&r).await).await?,
    )),
    C::Notifications { kind, page } => {
      let kind = kind.as_deref();
      ok(Data::Notifications(
        collect(page.limit, page.cursor, async |r| {
          p.notifications(kind, &r).await
        })
        .await?,
      ))
    }
    C::Unread => ok(Data::Counts(p.unread().await?)),
    C::Like { post, undo } => ok(Data::Action(p.like(&ctx.post_ref(&post)?, undo).await?)),
    C::Unlike { post } => ok(Data::Action(p.like(&ctx.post_ref(&post)?, true).await?)),
    C::Favorite { post, folder, undo } => ok(Data::Action(
      p.favorite(&ctx.post_ref(&post)?, folder.as_deref(), undo)
        .await?,
    )),
    C::Unfavorite { post, folder } => ok(Data::Action(
      p.favorite(&ctx.post_ref(&post)?, folder.as_deref(), true)
        .await?,
    )),
    C::Comment {
      post,
      text,
      reply_to,
    } => ok(Data::Action(
      p.comment(&ctx.post_ref(&post)?, &text, reply_to.as_deref())
        .await?,
    )),
    C::DeleteComment { post, comment, yes } => {
      confirm(yes, &format!("delete comment {comment}"))?;
      ok(Data::Action(
        p.delete_comment(&ctx.post_ref(&post)?, &comment).await?,
      ))
    }
    C::Follow { user, undo } => ok(Data::Action(p.follow(&ctx.user_ref(&user)?, undo).await?)),
    C::Unfollow { user } => ok(Data::Action(p.follow(&ctx.user_ref(&user)?, true).await?)),
    C::Publish(a) => {
      let draft = Draft {
        title: a.title,
        text: read_text(a.text)?,
        images: a.images,
        reply_to: a.reply_to.map(|r| ctx.post_ref(&r)).transpose()?,
        quote: a.quote.map(|q| ctx.post_ref(&q)).transpose()?,
        topics: a.topics,
      };
      if draft.text.is_empty() && draft.title.is_none() && draft.images.is_empty() {
        return Err(Error::input(
          "nothing to publish: give a text, --title or --image",
        ));
      }
      for image in &draft.images {
        if !image.is_file() {
          return Err(Error::input(format!(
            "image not found: {}",
            image.display()
          )));
        }
      }
      ok(Data::Action(p.publish(&draft).await?))
    }
    C::Delete { post, yes } => {
      let post = ctx.post_ref(&post)?;
      confirm(yes, &format!("delete {post}"))?;
      ok(Data::Action(p.delete(&post).await?))
    }
    C::Download(a) => {
      let audio_only = a.audio_only || a.split.is_some();
      let (post, media) = p.media(&ctx.post_ref(&a.post)?, audio_only).await?;
      let opts = DownloadOpts {
        dir: a.dir,
        audio_only: a.audio_only,
        split: a.split,
      };
      ok(Data::Downloads(
        download::download(ctx, P::INFO.home, &post, &media, &opts).await?,
      ))
    }
  }
}
