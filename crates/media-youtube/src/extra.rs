//! YouTube-only commands: transcripts, playlists, a channel's Shorts, live
//! streams and community posts, related videos, dislikes and the one-time
//! OAuth grant for analytics.

use media_core::cli::PageArgs;
use media_core::{Action, Data, Result, json};

use crate::api::Api;
use crate::channel::{self, Tab};
use crate::{caption, insights, library, refs, video};

#[derive(Debug, clap::Subcommand)]
pub enum Command {
  /// Transcript (captions) of a video
  #[command(visible_alias = "subtitle")]
  Transcript {
    video: String,
    /// Language code, e.g. en, de, zh-Hans; other languages are machine-translated
    #[arg(long)]
    lang: Option<String>,
  },
  /// Videos of a playlist
  Playlist {
    /// Playlist id or link
    playlist: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Shorts of a channel
  Shorts {
    user: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Live streams (past and upcoming) of a channel
  Streams {
    user: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Community posts of a channel
  #[command(visible_alias = "posts-tab")]
  Community {
    user: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Videos YouTube suggests next to a video
  Related {
    video: String,
    #[command(flatten)]
    page: PageArgs,
  },
  /// Grant read access to YouTube Analytics once (prints YOUTUBE_REFRESH_TOKEN)
  Oauth,
  /// Dislike a video (only you see it)
  Dislike {
    video: String,
    /// Remove the dislike instead
    #[arg(long)]
    undo: bool,
  },
}

async fn tab(api: &Api, user: &str, tab: Tab, page: &PageArgs) -> Result<Data> {
  let user = api.ctx.user_ref(user)?;
  Ok(Data::Posts(
    page
      .collect_dated(async |r| {
        let l = channel::tab(api, &user, tab, &r).await?;
        Ok(media_core::Page::new(l.posts, l.next))
      })
      .await?,
  ))
}

pub async fn run(api: &Api, command: Command) -> Result<Data> {
  use Command as C;
  let ctx = &api.ctx;
  Ok(match command {
    C::Transcript { video, lang } => {
      Data::Transcript(caption::transcript(api, &ctx.post_ref(&video)?, lang.as_deref()).await?)
    }
    C::Playlist { playlist, page } => {
      let id = refs::playlist(&playlist)?;
      Data::Posts(
        page
          .collect_dated(async |r| library::playlist(api, &id, &r).await)
          .await?,
      )
    }
    C::Shorts { user, page } => tab(api, &user, Tab::Shorts, &page).await?,
    C::Streams { user, page } => tab(api, &user, Tab::Streams, &page).await?,
    C::Community { user, page } => tab(api, &user, Tab::Posts, &page).await?,
    C::Related { video, page } => {
      let v = ctx.post_ref(&video)?;
      Data::Posts(
        page
          .collect_dated(async |r| video::related(api, &v, &r).await)
          .await?,
      )
    }
    C::Oauth => Data::Value(insights::authorize(ctx).await?),
    C::Dislike { video, undo } => {
      let id = refs::video(&ctx.post_ref(&video)?)?;
      let (path, name) = if undo {
        ("like/removelike", "undislike")
      } else {
        ("like/dislike", "dislike")
      };
      api
        .write(path, json!({ "target": { "videoId": id } }))
        .await?;
      Data::Action(Action::done(name, &id).with_url(refs::video_url(&id)))
    }
  })
}
