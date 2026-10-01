//! Running one shared command against a platform.

use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use super::args::{CommonCommand, SearchKind};
use crate::account;
use crate::download::{self, DownloadOpts};
use crate::error::{Error, Result};
use crate::model::{Comment, Data};
use crate::paging::collect;
use crate::platform::{Cap, Draft, Platform, Query};

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

/// A text argument, or standard input for `-`.
pub fn read_text(text: Option<String>) -> Result<String> {
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

/// Fails on the first image path that is not a file.
pub fn check_images(images: &[PathBuf]) -> Result<()> {
  match images.iter().find(|i| !i.is_file()) {
    Some(missing) => Err(Error::input(format!(
      "image not found: {}",
      missing.display()
    ))),
    None => Ok(()),
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
      let page = a.page;
      ok(match a.kind {
        SearchKind::Post => {
          Data::Posts(page.collect_dated(async |r| p.search(&q, &r).await).await?)
        }
        SearchKind::User => {
          Data::Users(page.collect(async |r| p.search_users(&q, &r).await).await?)
        }
        SearchKind::Topic => Data::Collections(
          page
            .collect(async |r| p.search_topics(&q, &r).await)
            .await?,
        ),
      })
    }
    C::Hot { category, page } => {
      let category = category.as_deref();
      ok(Data::Posts(
        page
          .collect_dated(async |r| p.hot(category, &r).await)
          .await?,
      ))
    }
    C::Feed { kind, page } => {
      let kind = kind.as_deref();
      ok(Data::Posts(
        page.collect_dated(async |r| p.feed(kind, &r).await).await?,
      ))
    }
    C::Read { post } => ok(Data::Post(Box::new(p.read(&ctx.post_ref(&post)?).await?))),
    C::Comments {
      post,
      sort,
      all,
      replies,
      mut page,
    } => {
      let (post, sort) = (ctx.post_ref(&post)?, sort.as_deref());
      if all {
        page.limit = usize::MAX;
      }
      let mut comments = page
        .collect_dated(async |r| p.comments(&post, sort, &r).await)
        .await?;
      if replies {
        complete_replies(p, &post, &mut comments.items, 0).await?;
      }
      ok(Data::Comments(comments))
    }
    C::Replies {
      post,
      comment,
      page,
    } => {
      let post = ctx.post_ref(&post)?;
      ok(Data::Comments(
        page
          .collect_dated(async |r| p.replies(&post, &comment, &r).await)
          .await?,
      ))
    }
    C::Likers { post, page } => {
      let post = ctx.post_ref(&post)?;
      ok(Data::Users(
        page.collect(async |r| p.likers(&post, &r).await).await?,
      ))
    }
    C::Reposts { post, page } => {
      let post = ctx.post_ref(&post)?;
      ok(Data::Posts(
        page
          .collect_dated(async |r| p.reposts(&post, &r).await)
          .await?,
      ))
    }
    C::Insights { post, days } => {
      let post = post.map(|x| ctx.post_ref(&x)).transpose()?;
      ok(Data::Insights(Box::new(
        p.insights(post.as_deref(), days.max(1)).await?,
      )))
    }
    C::User { user } => ok(Data::User(Box::new(p.user(&ctx.user_ref(&user)?).await?))),
    C::UserPosts { user, page } => {
      let user = user_or_me(p, user).await?;
      ok(Data::Posts(
        page
          .collect_dated(async |r| p.user_posts(&user, &r).await)
          .await?,
      ))
    }
    C::Followers { user, page } => {
      let user = user_or_me(p, user).await?;
      ok(Data::Users(
        page.collect(async |r| p.followers(&user, &r).await).await?,
      ))
    }
    C::Following { user, page } => {
      let user = user_or_me(p, user).await?;
      ok(Data::Users(
        page.collect(async |r| p.following(&user, &r).await).await?,
      ))
    }
    C::Collections { user, page } => {
      let user = user.map(|u| ctx.user_ref(&u)).transpose()?;
      let user = user.as_deref();
      ok(Data::Collections(
        page
          .collect(async |r| p.collections(user, &r).await)
          .await?,
      ))
    }
    C::Favorites { user, folder, page } => {
      let user = user.map(|u| ctx.user_ref(&u)).transpose()?;
      let folder = folder.map(|f| ctx.collection_ref(&f)).transpose()?;
      let (user, folder) = (user.as_deref(), folder.as_deref());
      ok(Data::Posts(
        page
          .collect_dated(async |r| p.favorites(user, folder, &r).await)
          .await?,
      ))
    }
    C::Likes { user, page } => {
      let user = user.map(|u| ctx.user_ref(&u)).transpose()?;
      let user = user.as_deref();
      ok(Data::Posts(
        page
          .collect_dated(async |r| p.likes(user, &r).await)
          .await?,
      ))
    }
    C::History { page } => ok(Data::Posts(
      page.collect_dated(async |r| p.history(&r).await).await?,
    )),
    C::Notifications { kind, page } => {
      let kind = kind.as_deref();
      ok(Data::Notifications(
        page
          .collect_dated(async |r| p.notifications(kind, &r).await)
          .await?,
      ))
    }
    C::Unread => ok(Data::Counts(p.unread().await?)),
    C::Like { post, undo } => ok(Data::Action(p.like(&ctx.post_ref(&post)?, undo).await?)),
    C::Unlike { post } => ok(Data::Action(p.like(&ctx.post_ref(&post)?, true).await?)),
    C::Favorite { post, folder, undo } => {
      let folder = folder.map(|f| ctx.collection_ref(&f)).transpose()?;
      ok(Data::Action(
        p.favorite(&ctx.post_ref(&post)?, folder.as_deref(), undo)
          .await?,
      ))
    }
    C::Unfavorite { post, folder } => {
      let folder = folder.map(|f| ctx.collection_ref(&f)).transpose()?;
      ok(Data::Action(
        p.favorite(&ctx.post_ref(&post)?, folder.as_deref(), true)
          .await?,
      ))
    }
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
      check_images(&draft.images)?;
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

/// How deep `comments --replies` follows nested threads.
const REPLY_DEPTH: usize = 3;

/// Fetch the full reply thread of every comment whose inline replies are
/// incomplete (platforms usually inline only a few), down to [`REPLY_DEPTH`].
async fn complete_replies<P: Platform>(
  p: &P,
  post: &str,
  comments: &mut [Comment],
  depth: usize,
) -> Result<()> {
  if !P::INFO.supports(Cap::Replies) || depth >= REPLY_DEPTH {
    return Ok(());
  }
  for c in comments.iter_mut() {
    let expected = c.reply_count.unwrap_or(0) as usize;
    if expected > 0 && c.replies.len() < expected {
      let id = c.id.clone();
      let all = collect(usize::MAX, None, async |r| p.replies(post, &id, &r).await).await?;
      c.replies = all.items;
    }
    Box::pin(complete_replies(p, post, &mut c.replies, depth + 1)).await?;
  }
  Ok(())
}

/// A user argument, or the logged-in account when it is omitted.
async fn user_or_me<P: Platform>(p: &P, user: Option<String>) -> Result<String> {
  match user {
    Some(u) => p.ctx().user_ref(&u),
    None => Ok(p.whoami().await?.reference().to_owned()),
  }
}
