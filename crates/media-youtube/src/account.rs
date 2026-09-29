//! The logged-in account: `account/account_menu` names it and links its channel.

use media_core::{Error, Result, User, Value, ValueExt, json};

use crate::api::{self, Api};
use crate::{channel, parse, refs};

/// Session extra with the account's channel id, so "mine" needs no lookup.
const CHANNEL_KEY: &str = "channel_id";

/// Arguments meaning "the logged-in account".
pub fn is_me(arg: &str) -> bool {
  matches!(arg.trim(), "me" | "self" | "mine" | "@me")
}

/// Forget the previous account before new cookies are verified.
pub fn reset(api: &Api) {
  api.ctx.set_extra(CHANNEL_KEY, "");
}

pub async fn whoami(api: &Api) -> Result<User> {
  api.require_login()?;
  let body = json!({
    "deviceTheme": "DEVICE_THEME_SUPPORTED",
    "userInterfaceTheme": "USER_INTERFACE_THEME_DARK",
  });
  let v = api.call("account/account_menu", body).await?;
  if !api::served_logged_in(&v) {
    return Err(Error::auth("YouTube did not accept this session").with_hint(api.ctx.login_hint()));
  }
  let header = parse::first(&v, "activeAccountHeaderRenderer").unwrap_or(&Value::Null);
  let name = parse::text(header.at("accountName"));
  let handle = parse::text(header.at("channelHandle")).filter(|h| h.starts_with('@'));
  // "Your channel" links the channel page.
  let id = parse::find(&v, "browseEndpoint")
    .into_iter()
    .find_map(|b| b.str("browseId").filter(|id| refs::is_channel_id(id)));
  let Some(id) = id else {
    let name = name.unwrap_or_else(|| "YouTube account".into());
    let mut u = User {
      id: handle.clone().unwrap_or_else(|| name.clone()),
      avatar: parse::image(header.at("accountPhoto")),
      handle,
      name,
      ..User::default()
    };
    u.extra.insert("channel".into(), false.into());
    return Ok(u);
  };
  api.ctx.set_extra(CHANNEL_KEY, &id);
  let mut user = match channel::profile(api, &id, false).await {
    Ok(u) => u,
    Err(e) => {
      tracing::debug!("channel of the account: {e}");
      User {
        url: Some(refs::channel_url(&id)),
        id: id.clone(),
        ..User::default()
      }
    }
  };
  if let Some(n) = name {
    user.name = n;
  }
  user.handle = user.handle.or(handle);
  user.avatar = user
    .avatar
    .or_else(|| parse::image(header.at("accountPhoto")));
  Ok(user)
}

/// Channel id of the logged-in account.
pub async fn my_channel(api: &Api) -> Result<String> {
  api.require_login()?;
  if let Some(id) = api.ctx.extra(CHANNEL_KEY).filter(|id| !id.is_empty()) {
    return Ok(id);
  }
  let me = whoami(api).await?;
  if refs::is_channel_id(&me.id) {
    Ok(me.id)
  } else {
    Err(Error::input("this YouTube account has no channel"))
  }
}

/// Whether `arg` is the logged-in account (by alias or by channel).
pub async fn is_mine(api: &Api, arg: &str) -> Result<bool> {
  if is_me(arg) {
    return Ok(true);
  }
  if !api.logged_in() {
    return Ok(false);
  }
  let (mine, theirs) = (my_channel(api).await?, channel::id(api, arg).await?);
  Ok(mine == theirs)
}
