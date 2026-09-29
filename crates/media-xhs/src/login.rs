//! QR login over plain HTTP (`xhs_cli/qr_login.py::_http_qrcode_login`):
//! fresh device cookies, a guest session, a QR code, then status polling
//! and session completion.

use std::cell::Cell;
use std::time::Duration;

use media_core::{Error, QrStatus, QrTicket, Result, Value, ValueExt, json};

use crate::api::Client;
use crate::people::ME;

const CREATE: &str = "/api/sns/web/v1/login/qrcode/create";
const USERINFO: &str = "/api/qrcode/userinfo";
const STATUS: &str = "/api/sns/web/v1/login/qrcode/status";

pub async fn qr_start(c: &Client) -> Result<QrTicket> {
  // New a1 / webId and a guest session, then the QR code.
  c.start_guest().await;
  let data = c.post(CREATE, &json!({"qr_type": 1})).await?;
  let (Some(qr_id), Some(code), Some(url)) = (data.str("qr_id"), data.str("code"), data.str("url"))
  else {
    return Err(Error::upstream(format!("unexpected QR payload: {data}")));
  };
  Ok(QrTicket {
    url,
    token: qr_id,
    extra: [("code".to_owned(), code)].into(),
  })
}

/// One poll. Transient errors count as "waiting" until three in a row.
pub async fn qr_poll(c: &Client, ticket: &QrTicket, errors: &Cell<u32>) -> Result<QrStatus> {
  let code = ticket
    .extra
    .get("code")
    .map(String::as_str)
    .unwrap_or_default();
  let body = json!({"qrId": ticket.token, "code": code});
  let status = match c
    .post_with(USERINFO, &body, &[("service-tag", "webcn")])
    .await
  {
    Ok(v) => {
      errors.set(0);
      v
    }
    Err(e) => {
      errors.set(errors.get() + 1);
      tracing::debug!("QR status check failed: {e}");
      return if errors.get() >= 3 {
        Err(e)
      } else {
        Ok(QrStatus::Waiting)
      };
    }
  };
  match status.i64("codeStatus") {
    Some(1) => Ok(QrStatus::Scanned),
    Some(2) => {
      let user = status
        .str("userId")
        .ok_or_else(|| Error::upstream("QR login confirmed without a user id"))?;
      complete(c, &ticket.token, code, &user).await?;
      Ok(QrStatus::Confirmed)
    }
    _ => Ok(QrStatus::Waiting),
  }
}

/// Finish the login until the session belongs to the confirmed user.
async fn complete(c: &Client, qr_id: &str, code: &str, user: &str) -> Result<()> {
  for attempt in 0..5 {
    let data = c.get(STATUS, &[("qr_id", qr_id), ("code", code)]).await?;
    c.apply_session(&data);
    if user_id(&data).as_deref() == Some(user) {
      return Ok(());
    }
    match c.get(ME, &[]).await {
      Ok(me) if user_id(&me).as_deref() == Some(user) => return Ok(()),
      Ok(_) => {}
      Err(e) => tracing::debug!("post-confirm self check failed: {e}"),
    }
    if attempt < 4 {
      tokio::time::sleep(Duration::from_secs(1)).await;
    }
  }
  Err(Error::auth(
    "QR login confirmed, but the session never switched to the confirmed user",
  ))
}

fn user_id(v: &Value) -> Option<String> {
  v.first_str(&[
    "login_info.user_id",
    "basic_info.user_id",
    "user_id",
    "userid",
  ])
}

/// After importing cookies: add the `webId` companion of `a1` when missing.
pub fn prepare(c: &Client) {
  let http = &c.ctx.http;
  if http.has_cookie("a1") && !http.has_cookie("webId") {
    http.set_cookie("webId", &crate::sign::random::web_id());
  }
}
