//! Account: the logged-in user, helper cookies, QR login and unread counters.

use std::collections::BTreeMap;

use media_core::{Ctx, Error, QrStatus, QrTicket, Result, User, Value, ValueExt};

use crate::api::{self, V3, V4, WWW};
use crate::parse;

const QR_API: &str = "https://www.zhihu.com/api/v3/account/api/login/qrcode";

pub async fn whoami(ctx: &Ctx) -> Result<User> {
  let v = api::call(
    ctx,
    api::get(ctx, &format!("{V4}/me")).query(
      "include",
      "answer_count,articles_count,pins_count,question_count,follower_count,following_count,voteup_count,description",
    ),
  )
  .await?;
  match v.str("id") {
    Some(id) if id != "0" => Ok(parse::user(&v)),
    _ => Err(Error::auth("Zhihu did not accept the session")),
  }
}

/// The logged-in user's `url_token`.
pub async fn my_token(ctx: &Ctx) -> Result<String> {
  whoami(ctx)
    .await?
    .handle
    .ok_or_else(|| Error::upstream("the account has no url_token"))
}

/// Fetch `_xsrf` and `d_c0` when the session lacks them (write endpoints need `_xsrf`).
pub async fn helper_cookies(ctx: &Ctx) -> Result<()> {
  if !ctx.http.has_cookie("_xsrf") {
    // The home page sets `_xsrf`; its status does not matter.
    let _ = ctx
      .http
      .get(format!("{WWW}/"))
      .header("accept", "text/html,application/xhtml+xml")
      .send()
      .await?;
  }
  if !ctx.http.has_cookie("d_c0") {
    let _ = api::post(ctx, &format!("{WWW}/udid"))
      .json(&serde_json::json!({}))
      .send()
      .await?;
  }
  Ok(())
}

pub async fn qr_start(ctx: &Ctx) -> Result<QrTicket> {
  // Start from a clean jar: an expired `z_c0` would confuse the flow.
  ctx.http.replace_cookies(Default::default());
  let _ = ctx
    .http
    .get(format!("{WWW}/signin"))
    .header("accept", "text/html,application/xhtml+xml")
    .send()
    .await?;
  helper_cookies(ctx).await?;
  // Issues the `capsion_ticket` cookie the confirmation step checks.
  let _ = api::get(ctx, &format!("{V3}/oauth/captcha/v2"))
    .query("type", "captcha_sign_in")
    .send()
    .await?;
  let v = api::call_public(
    ctx,
    api::post(ctx, QR_API)
      .header("referer", format!("{WWW}/signin"))
      .header("origin", WWW)
      .json(&serde_json::json!({})),
  )
  .await?;
  let token = v
    .first_str(&["token", "qrcode_token"])
    .ok_or_else(|| Error::upstream("the QR code API returned no token"))?;
  let url = v
    .str("link")
    .ok_or_else(|| Error::upstream("the QR code API returned no link"))?;
  Ok(QrTicket {
    url,
    token,
    extra: BTreeMap::new(),
  })
}

pub async fn qr_poll(ctx: &Ctx, ticket: &QrTicket) -> Result<QrStatus> {
  let resp = api::get(ctx, &format!("{QR_API}/{}/scan_info", ticket.token))
    .header("referer", format!("{WWW}/signin?next=%2F"))
    .header("accept", "*/*")
    .header("sec-fetch-dest", "empty")
    .header("sec-fetch-mode", "cors")
    .header("sec-fetch-site", "same-origin")
    .header("x-zse-93", "101_3_3.0")
    .send()
    .await?;
  let v: Value = serde_json::from_slice(&resp.body).unwrap_or(Value::Null);
  let message = v.str("error.message").unwrap_or_default();
  if ["过期", "失效", "expire"]
    .iter()
    .any(|w| message.contains(w))
  {
    return Ok(QrStatus::Expired);
  }
  if !resp.status.is_success() {
    // Transient failures: keep polling until the core gives up.
    return Ok(QrStatus::Waiting);
  }
  absorb_cookies(ctx, &v);
  let confirmed = v.str("access_token").is_some() || v.str("user_id").is_some();
  match v.i64("status") {
    _ if ctx.http.has_cookie("z_c0") => Ok(QrStatus::Confirmed),
    Some(0) => Ok(QrStatus::Waiting),
    Some(1) => Ok(QrStatus::Scanned),
    _ if confirmed => {
      // The cookie usually arrives with the confirmation; ask once more if it did not.
      let _ = api::get(ctx, &format!("{V4}/me")).send().await?;
      if ctx.http.has_cookie("z_c0") {
        Ok(QrStatus::Confirmed)
      } else {
        Err(Error::auth(
          "login confirmed but Zhihu issued no z_c0 cookie",
        ))
      }
    }
    _ => Ok(QrStatus::Waiting),
  }
}

/// Cookies some scan_info responses carry in the body instead of `Set-Cookie`.
fn absorb_cookies(ctx: &Ctx, v: &Value) {
  if let Some(raw) = v
    .first_str(&["cookie", "cookies"])
    .filter(|c| c.contains("z_c0"))
  {
    for part in raw.split(';') {
      if let Some((k, val)) = part.trim().split_once('=')
        && !k.is_empty()
      {
        ctx.http.set_cookie(k.trim(), val.trim());
      }
    }
  }
  if let Some(z) = v.str("z_c0") {
    ctx.http.set_cookie("z_c0", &z);
  }
}

/// Unread counters, as reported on the logged-in account.
pub async fn unread(ctx: &Ctx) -> Result<BTreeMap<String, u64>> {
  let v = api::call(
    ctx,
    api::get(ctx, &format!("{V4}/me")).query(
      "include",
      "default_notifications_count,follow_notifications_count,vote_thank_notifications_count,messages_count",
    ),
  )
  .await?;
  Ok(
    [
      ("notifications", "default_notifications_count"),
      ("follows", "follow_notifications_count"),
      ("likes", "vote_thank_notifications_count"),
      ("messages", "messages_count"),
    ]
    .into_iter()
    .filter_map(|(key, field)| v.count(field).map(|n| (key.to_owned(), n)))
    .collect(),
  )
}
