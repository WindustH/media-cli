//! The shared command set and how it is dispatched to a [`Platform`].
//!
//! Every platform gets the same subcommands; the ones it does not support are
//! hidden from `--help`, and platform-specific values (sort orders, hot
//! categories ...) are injected into the shared options from
//! [`Choices`](crate::platform::Choices).

mod args;
mod dispatch;

use std::process::ExitCode;

use clap::{ArgMatches, FromArgMatches, Subcommand, builder::PossibleValuesParser};

pub use self::args::*;
use crate::account;
use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::http::{Http, HttpConfig};
use crate::model::Data;
use crate::output;
use crate::platform::{Cap, Platform, PlatformInfo};
use crate::store::{RefKind, Store};

/// Which capability a shared subcommand needs; `None` means always available.
fn required_cap(name: &str) -> Option<&'static [Cap]> {
  Some(match name {
    "search" => &[Cap::Search, Cap::SearchUsers, Cap::SearchTopics],
    "hot" => &[Cap::Hot],
    "feed" => &[Cap::Feed],
    "read" => &[Cap::Read],
    "comments" => &[Cap::Comments],
    "replies" => &[Cap::Replies],
    "likers" => &[Cap::Likers],
    "reposts" => &[Cap::Reposts],
    "insights" => &[Cap::AccountInsights, Cap::PostInsights],
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
  let platform = P::new(Ctx::new(info, http, store, loaded.session.extra.clone()))?;

  let name = matches.subcommand_name().unwrap_or_default();
  let (data, code, persist) = if CommonCommand::has_subcommand(name) {
    let cmd = CommonCommand::from_arg_matches(matches).map_err(|e| Error::input(e.to_string()))?;
    dispatch::common(&platform, cmd, &loaded).await?
  } else {
    let cmd = <P::Extra as FromArgMatches>::from_arg_matches(matches)
      .map_err(|e| Error::input(e.to_string()))?;
    (platform.run_extra(cmd).await?, ExitCode::SUCCESS, true)
  };

  let ctx = platform.ctx();
  // Only logged-in sessions are saved; guest cookies are not worth keeping.
  if persist && platform.logged_in() && !loaded.from_env && ctx.session_changed() {
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
