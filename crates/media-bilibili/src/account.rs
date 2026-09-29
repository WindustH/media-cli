//! Login (web QR code), the logged-in account and its mid.

use std::collections::BTreeMap;

use media_core::{Ctx, Error, QrStatus, QrTicket, Result, User, ValueExt};

use crate::{api, parse};

const NAV: &str = "https://api.bilibili.com/x/web-interface/nav";
const QR_GENERATE: &str =
  "https://passport.bilibili.com/x/passport-login/web/qrcode/generate?source=main-fe-header";
const QR_POLL: &str = "https://passport.bilibili.com/x/passport-login/web/qrcode/poll";

/// Cookies the QR login hands over in the confirmation URL.
const QR_COOKIES: &[&str] = &["SESSDATA", "bili_jct", "DedeUserID", "DedeUserID__ckMd5"];

pub async fn whoami(ctx: &Ctx) -> Result<User> {
  ctx.require_login(api::LOGIN_COOKIES)?;
  let nav = api::get(ctx, NAV).send_raw().await?;
  api::remember_wbi(ctx, &nav);
  let data = nav.at("data");
  if nav.i64("code") != Some(0) || data.bool("isLogin") != Some(true) {
    return Err(Error::auth(
      "the session is not logged in (nav returned -101)",
    ));
  }
  let mut user = parse::user(data);
  if let Some(coins) = data.count("money") {
    user.stats.other.insert("coins".into(), coins);
  }
  Ok(user)
}

/// The logged-in account's mid: from the `DedeUserID` cookie, else from `nav`.
pub async fn my_mid(ctx: &Ctx) -> Result<String> {
  ctx.require_login(api::LOGIN_COOKIES)?;
  match ctx.http.cookie("DedeUserID") {
    Some(mid) => Ok(mid),
    None => Ok(whoami(ctx).await?.id),
  }
}

pub async fn qr_start(ctx: &Ctx) -> Result<QrTicket> {
  let data = api::get(ctx, QR_GENERATE).send().await?;
  match (data.str("url"), data.str("qrcode_key")) {
    (Some(url), Some(token)) => Ok(QrTicket {
      url,
      token,
      extra: BTreeMap::new(),
    }),
    _ => Err(Error::upstream("QR login: no qrcode_key in the response")),
  }
}

pub async fn qr_poll(ctx: &Ctx, ticket: &QrTicket) -> Result<QrStatus> {
  let data = api::get(ctx, QR_POLL)
    .arg("qrcode_key", &ticket.token)
    .send()
    .await?;
  Ok(match data.i64("code") {
    Some(86101) => QrStatus::Waiting,
    Some(86090) => QrStatus::Scanned,
    Some(86038) => QrStatus::Expired,
    Some(0) => {
      // Cookies arrive via set-cookie; the redirect URL repeats them (still URL-encoded).
      let url = data.str("url").unwrap_or_default();
      let query = url.split_once('?').map(|(_, q)| q).unwrap_or_default();
      for (k, v) in query.split('&').filter_map(|kv| kv.split_once('=')) {
        if QR_COOKIES.contains(&k) && !ctx.http.has_cookie(k) {
          ctx.http.set_cookie(k, v);
        }
      }
      if let Some(token) = data.str("refresh_token") {
        ctx.set_extra("refresh_token", &token);
      }
      QrStatus::Confirmed
    }
    other => {
      return Err(Error::upstream(format!(
        "QR login: {} ({})",
        data.str("message").unwrap_or_default(),
        other.unwrap_or(-1)
      )));
    }
  })
}
