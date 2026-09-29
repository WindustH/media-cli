//! Running one shared command against a platform.

use std::io::{IsTerminal, Read};
use std::process::ExitCode;

use super::args::{CommonCommand, SearchKind};
use crate::account;
use crate::download::{self, DownloadOpts};
use crate::error::{Error, Result};
use crate::model::Data;
use crate::paging::collect;
use crate::platform::{Draft, Platform, Query};

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
pub(super) async fn common<P: Platform>(
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
