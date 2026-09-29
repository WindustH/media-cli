//! The logged-in account: the avatar menu (`account/account_menu`) names it
//! and links its channel; the channel switcher (`account/accounts_list`, as
//! YouTube.js `AccountManager.getInfo(true)` asks for it) is the fallback.

use media_core::{Error, ErrorCode, Result, User, Value, ValueExt, json};

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

/// What YouTube says about the session's account.
#[derive(Default)]
struct Identity {
  name: Option<String>,
  handle: Option<String>,
  photo: Option<String>,
  channel: Option<String>,
}

fn logged_in(api: &Api, v: &Value) -> Result<()> {
  if api::served_logged_in(v) {
    Ok(())
  } else {
    Err(Error::auth("YouTube did not accept this session").with_hint(api.ctx.login_hint()))
  }
}

fn handle(v: &Value) -> Option<String> {
  parse::text(v.at("channelHandle"))
    .filter(|h| h.starts_with('@'))
    .map(|h| h.trim_start_matches('@').to_owned())
}

async fn from_menu(api: &Api) -> Result<Identity> {
  let body = json!({
    "deviceTheme": "DEVICE_THEME_SUPPORTED",
    "userInterfaceTheme": "USER_INTERFACE_THEME_DARK",
  });
  let v = api.call("account/account_menu", body).await?;
  logged_in(api, &v)?;
  let header = parse::first(&v, "activeAccountHeaderRenderer").unwrap_or(&Value::Null);
  Ok(Identity {
    name: parse::text(header.at("accountName")),
    handle: handle(header),
    photo: parse::image(header.at("accountPhoto")),
    // "Your channel" links the channel page.
    channel: parse::find(&v, "browseEndpoint")
      .into_iter()
      .find_map(|b| b.str("browseId").filter(|id| refs::is_channel_id(id))),
  })
}

async fn from_switcher(api: &Api) -> Result<Identity> {
  let body = json!({
    "requestType": "ACCOUNTS_LIST_REQUEST_TYPE_CHANNEL_SWITCHER",
    "callCircumstance": "SWITCHING_USERS_FULL",
  });
  let v = api.call("account/accounts_list", body).await?;
  logged_in(api, &v)?;
  let items = parse::find(&v, "accountItem");
  let item = items
    .iter()
    .find(|i| i.bool("isSelected") == Some(true))
    .or(items.first())
    .ok_or_else(|| Error::upstream("YouTube listed no account for this session"))?;
  Ok(Identity {
    name: parse::text(item.at("accountName")),
    handle: handle(item),
    photo: parse::image(item.at("accountPhoto")),
    channel: None,
  })
}

pub async fn whoami(api: &Api) -> Result<User> {
  api.require_login()?;
  let who = match from_menu(api).await {
    Err(e) if e.code == ErrorCode::NotAuthenticated => return Err(e),
    Err(e) => {
      tracing::debug!("account menu: {e}; asking the channel switcher");
      from_switcher(api).await?
    }
    Ok(who) => who,
  };
  let id = match (who.channel, &who.handle) {
    (Some(id), _) => Some(id),
    (None, Some(h)) => channel::id(api, &format!("@{h}")).await.ok(),
    _ => None,
  };
  let Some(id) = id else {
    let name = who.name.unwrap_or_else(|| "YouTube account".into());
    let mut u = User {
      id: who.handle.clone().unwrap_or_else(|| name.clone()),
      avatar: who.photo,
      handle: who.handle,
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
  if let Some(n) = who.name {
    user.name = n;
  }
  user.handle = user.handle.or(who.handle);
  user.avatar = user.avatar.or(who.photo);
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
