//! Twitter-only commands: retweet, quote, list timelines and bookmark folders.

use std::path::PathBuf;

use media_core::cli::PageArgs;
use media_core::paging::collect;
use media_core::{Action, Data, Draft, Error, Result};

use crate::api::Api;
use crate::{tweets, users, write};

#[derive(Debug, clap::Subcommand)]
pub enum Command {
  /// Retweet a tweet
  #[command(visible_alias = "repost")]
  Retweet {
    post: String,
    /// Undo the retweet instead
    #[arg(long)]
    undo: bool,
  },
  /// Quote a tweet with your own text
  Quote {
    post: String,
    text: String,
    /// Attach an image (repeatable, at most 4)
    #[arg(short = 'i', long = "image", value_name = "PATH")]
    images: Vec<PathBuf>,
  },
  /// Latest tweets of a list (id or x.com/i/lists/... URL; see `collections`)
  List {
    list: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Your bookmark folders (ids for `favorites --folder`)
  Folders {
    #[command(flatten)]
    page: PageArgs,
  },
}

pub async fn run(api: &Api, command: Command) -> Result<Data> {
  let ctx = &api.ctx;
  Ok(match command {
    Command::Retweet { post, undo } => {
      Data::Action(write::retweet(api, &ctx.post_ref(&post)?, undo).await?)
    }
    Command::Quote { post, text, images } => {
      Data::Action(quote(api, &ctx.post_ref(&post)?, &text, &images).await?)
    }
    Command::List { list, page } => Data::Posts(
      collect(page.limit, page.cursor, async |r| {
        tweets::list(api, &list, &r).await
      })
      .await?,
    ),
    Command::Folders { page } => Data::Collections(
      collect(page.limit, page.cursor, async |r| {
        users::folders(api, &r).await
      })
      .await?,
    ),
  })
}

async fn quote(api: &Api, post: &str, text: &str, images: &[PathBuf]) -> Result<Action> {
  if let Some(missing) = images.iter().find(|i| !i.is_file()) {
    return Err(Error::input(format!(
      "image not found: {}",
      missing.display()
    )));
  }
  let draft = Draft {
    text: text.to_owned(),
    images: images.to_vec(),
    quote: Some(post.to_owned()),
    ..Draft::default()
  };
  write::publish(api, &draft).await
}
