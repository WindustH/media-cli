//! Xiaohongshu-only commands.

use media_core::cli::PageArgs;
use media_core::{Data, Result};

use crate::api::Client;
use crate::creator;

#[derive(Debug, clap::Subcommand)]
pub enum XhsCommand {
  /// Your own published notes (creator center)
  MyNotes {
    #[command(flatten)]
    page: PageArgs,
  },
  /// Your notes with impressions, views, CTR, watch time and follows (data center)
  NoteStats {
    #[command(flatten)]
    page: PageArgs,
  },
  /// Your fans who interacted most (data center)
  ActiveFans {
    /// Window in days: up to 7 means the last 7, otherwise the last 30
    #[arg(short, long, default_value_t = 30)]
    days: u32,
  },
}

pub async fn run(c: &Client, command: XhsCommand) -> Result<Data> {
  Ok(match command {
    XhsCommand::MyNotes { page } => Data::Posts(
      page
        .collect_dated(async |r| creator::my_notes(c, &r).await)
        .await?,
    ),
    XhsCommand::NoteStats { page } => Data::Posts(
      page
        .collect_dated(async |r| creator::note_stats(c, &r).await)
        .await?,
    ),
    XhsCommand::ActiveFans { days } => Data::Users(creator::active_fans(c, days).await?),
  })
}
