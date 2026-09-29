//! Bilibili-only commands.

use media_core::cli::PageArgs;
use media_core::{Ctx, Data, Result};

use crate::refs::{self, Video};
use crate::{account, creator, dynamic, play, user, video};

#[derive(Debug, clap::Subcommand)]
pub enum Extra {
  /// Give coins to a video
  Coin {
    video: String,
    /// Number of coins
    #[arg(short = 'n', long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=2))]
    count: u8,
    /// Also like the video
    #[arg(long)]
    like: bool,
  },
  /// Like, coin and favorite a video at once
  Triple { video: String },
  /// Subtitles of a video (most tracks need a login)
  Subtitle {
    video: String,
    /// Track language, e.g. zh-CN, ai-zh, en
    #[arg(long)]
    lang: Option<String>,
  },
  /// AI summary and outline of a video
  Summary { video: String },
  /// Danmaku (bullet comments) of a video
  Danmaku { video: String },
  /// Videos related to a video
  Related {
    video: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Site-wide ranking
  Rank {
    #[command(flatten)]
    page: PageArgs,
  },
  /// Your watch-later list
  WatchLater,
  /// Your videos side by side with creator-center numbers (3-second bounce,
  /// share watched, follows, click-through rank ...)
  #[command(visible_alias = "my-videos")]
  VideoStats {
    /// Only these videos (at most 10); default: the latest ones
    videos: Vec<String>,
    /// How many of the latest videos
    #[arg(short = 'n', long, default_value_t = 10, value_parser = clap::value_parser!(u16).range(1..=50))]
    limit: u16,
  },
  /// Dynamics of a user (yours when USER is omitted)
  #[command(visible_alias = "my-dynamics")]
  Dynamics {
    user: Option<String>,
    #[command(flatten)]
    page: PageArgs,
  },
}

async fn video_ref(ctx: &Ctx, arg: &str) -> Result<Video> {
  refs::video(ctx, &ctx.post_ref(arg)?).await
}

pub async fn run(ctx: &Ctx, command: Extra) -> Result<Data> {
  use Extra as E;
  Ok(match command {
    E::Coin { video, count, like } => {
      let v = video_ref(ctx, &video).await?;
      Data::Action(video::coin(ctx, &v, count, like).await?)
    }
    E::Triple { video } => Data::Action(video::triple(ctx, &video_ref(ctx, &video).await?).await?),
    E::Subtitle { video, lang } => {
      let v = video_ref(ctx, &video).await?;
      Data::Transcript(play::subtitle(ctx, &v, lang.as_deref()).await?)
    }
    E::Summary { video } => Data::Value(play::summary(ctx, &video_ref(ctx, &video).await?).await?),
    E::Danmaku { video } => {
      Data::Transcript(play::danmaku(ctx, &video_ref(ctx, &video).await?).await?)
    }
    E::Related { video, page } => {
      let v = video_ref(ctx, &video).await?;
      Data::Posts(
        page
          .collect_dated(async |_| video::related(ctx, &v).await)
          .await?,
      )
    }
    E::Rank { page } => Data::Posts(
      page
        .collect_dated(async |_| video::ranking(ctx, 0, "all").await)
        .await?,
    ),
    E::WatchLater => Data::Posts(video::watch_later(ctx).await?),
    E::VideoStats { videos, limit } => {
      let mut refs = Vec::new();
      for v in &videos {
        refs.push(video_ref(ctx, v).await?);
      }
      Data::Posts(creator::compare(ctx, &refs, limit.into()).await?)
    }
    E::Dynamics { user, page } => {
      let mid = match user {
        Some(u) => user::mid(ctx, &ctx.user_ref(&u)?).await?,
        None => account::my_mid(ctx).await?,
      };
      Data::Posts(
        page
          .collect_dated(async |r| dynamic::of_user(ctx, &mid, &r).await)
          .await?,
      )
    }
  })
}
