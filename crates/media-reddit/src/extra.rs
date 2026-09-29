//! Reddit-only commands: browsing one subreddit, community details and
//! listings, downvotes, a user's comments, edits and marking the inbox read.

use clap::builder::PossibleValuesParser;
use media_core::cli::PageArgs;
use media_core::paging::collect;
use media_core::{Data, Page, Result};

use crate::api::Api;
use crate::posts::TIME_RANGES;
use crate::{inbox, posts, subs, users, write};

#[derive(Debug, clap::Subcommand)]
pub enum Command {
  /// Posts of one subreddit
  #[command(visible_alias = "r")]
  Sub {
    /// Name, r/name or link
    subreddit: String,
    /// Order of the posts
    #[arg(short, long, default_value = "hot",
      value_parser = ["hot", "new", "top", "rising", "controversial"])]
    sort: String,
    /// Time range of `top` and `controversial`
    #[arg(short, long, value_parser = PossibleValuesParser::new(TIME_RANGES.iter().copied()))]
    time: Option<String>,
    #[command(flatten)]
    page: PageArgs,
  },
  /// About a subreddit: description, subscribers and active users
  Subreddit { subreddit: String },
  /// Popular, new or default subreddits
  Subreddits {
    #[arg(default_value = "popular", value_parser = ["popular", "new", "default"])]
    which: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Downvote a post or comment (`t1_…` id or comment link)
  Downvote {
    post: String,
    /// Clear the vote instead
    #[arg(long)]
    undo: bool,
  },
  /// Comments written by a user
  UserComments {
    user: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Replace the text of your post or comment (`t1_…` id or comment link)
  Edit {
    post: String,
    /// New text (Markdown)
    text: String,
  },
  /// Mark every inbox item as read
  MarkRead,
}

pub async fn run(api: &Api, command: Command) -> Result<Data> {
  use Command as C;
  let ctx = &api.ctx;
  Ok(match command {
    C::Sub {
      subreddit,
      sort,
      time,
      page,
    } => Data::Posts(
      collect(page.limit, page.cursor, async |r| {
        posts::subreddit(api, &subreddit, &sort, time.as_deref(), &r).await
      })
      .await?,
    ),
    C::Subreddit { subreddit } => {
      Data::Collections(Page::last(vec![subs::about(api, &subreddit).await?]))
    }
    C::Subreddits { which, page } => Data::Collections(
      collect(page.limit, page.cursor, async |r| {
        subs::listed(api, &which, &r).await
      })
      .await?,
    ),
    C::Downvote { post, undo } => {
      let (dir, name) = if undo {
        (0, "unvote")
      } else {
        (-1, "downvote")
      };
      Data::Action(write::vote(api, &ctx.post_ref(&post)?, dir, name).await?)
    }
    C::UserComments { user, page } => {
      let user = ctx.user_ref(&user)?;
      Data::Comments(
        collect(page.limit, page.cursor, async |r| {
          users::comments(api, &user, &r).await
        })
        .await?,
      )
    }
    C::Edit { post, text } => Data::Action(write::edit(api, &ctx.post_ref(&post)?, &text).await?),
    C::MarkRead => Data::Action(inbox::mark_read(api).await?),
  })
}
