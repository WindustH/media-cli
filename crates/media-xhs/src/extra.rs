//! Xiaohongshu-only commands.

use std::path::PathBuf;

use media_core::cli::{PageArgs, check_images, read_text};
use media_core::{Data, Draft, Result};

use crate::api::Client;
use crate::{creator, events};

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
  /// Activities of the creator center (活动中心), optionally matching a keyword
  #[command(after_long_help = EX_EVENTS)]
  Events {
    /// Only activities whose name, reward or topic contains this
    keyword: Option<String>,
    /// Only the activities you kept (收藏)
    #[arg(long)]
    kept: bool,
    /// Latest first instead of the center's own order
    #[arg(long)]
    latest: bool,
  },
  /// One activity: rewards, period, topics and links
  Event {
    /// Activity id, page id, link or exact name
    event: String,
  },
  /// Keep (收藏) an activity in the activity center
  KeepEvent {
    /// Activity id, page id, link or exact name
    event: String,
    /// Remove it from your kept activities instead
    #[arg(long)]
    undo: bool,
  },
  /// Join an activity by publishing an image note for it
  #[command(after_long_help = EX_JOIN)]
  JoinEvent {
    /// Activity id, page id, link or exact name
    event: String,
    /// Note text; `-` reads it from stdin
    text: Option<String>,
    #[arg(long)]
    title: Option<String>,
    /// Attach an image (repeatable, at least one)
    #[arg(short = 'i', long = "image", value_name = "PATH", required = true)]
    images: Vec<PathBuf>,
    /// Another topic besides the activity's own (repeatable)
    #[arg(long = "topic", value_name = "TOPIC")]
    topics: Vec<String>,
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
    XhsCommand::Events {
      keyword,
      kept,
      latest,
    } => {
      let rows = events::list(c, kept, latest).await?;
      Data::Collections(events::page(&events::matching(rows, keyword.as_deref())))
    }
    XhsCommand::Event { event } => Data::Value(events::detail(&events::find(c, &event).await?)),
    XhsCommand::KeepEvent { event, undo } => Data::Action(events::keep(c, &event, undo).await?),
    XhsCommand::JoinEvent {
      event,
      text,
      title,
      images,
      topics,
    } => {
      check_images(&images)?;
      let draft = Draft {
        title,
        text: read_text(text)?,
        images,
        topics,
        ..Draft::default()
      };
      Data::Action(events::join(c, &event, &draft).await?)
    }
    XhsCommand::ActiveFans { days } => Data::Users(creator::active_fans(c, days).await?),
  })
}

const EX_EVENTS: &str = "\
Examples:
  media xhs events                 # everything running now
  media xhs events 动漫 --latest    # activities about anime, newest first
  media xhs events --kept          # the ones you kept";

const EX_JOIN: &str = "\
Examples:
  media xhs events 动漫
  media xhs event '#1'             # rewards, period and topics first
  media xhs join-event '#1' --title '十月新番' -i cover.jpg '这季最期待的三部'
  media xhs join-event 43010 --title 周末 -i a.jpg -i b.jpg - < note.txt";
