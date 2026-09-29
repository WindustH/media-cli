//! Zhihu transport: base URLs, request headers and the error envelope.
//!
//! Every request carries `x-requested-with: fetch` and, once the `_xsrf`
//! cookie is known, the matching `x-xsrftoken` that write endpoints require.
//! Upstream failures (`{"error": {"code", "message"}}`) map to shared error codes.
//!
//! Zhihu additionally signs web requests with `x-zse-93` / `x-zse-96`
//! (derived from the `d_c0` cookie and the path). Logged-in sessions work
//! without it; anonymous API calls are refused (`10003`), and after a few of
//! them the IP is flagged (`40352`). So [`call`] requires a session and only
//! the hot list and the login flow go through [`call_public`].

use media_core::http::{Method, Req};
use media_core::text::{one_line, truncate};
use media_core::{Ctx, Error, ErrorCode, Result, Value, ValueExt};

pub const WWW: &str = "https://www.zhihu.com";
pub const V4: &str = "https://www.zhihu.com/api/v4";
pub const V3: &str = "https://www.zhihu.com/api/v3";
pub const ZHUANLAN: &str = "https://zhuanlan.zhihu.com/api";
/// The mobile API host; serves the hot list without a login.
pub const MOBILE: &str = "https://api.zhihu.com";

/// Fail early without the `z_c0` session cookie.
pub fn need_login(ctx: &Ctx) -> Result<()> {
  ctx.require_login(&["z_c0"]).map_err(|e| Error {
    message: "Zhihu serves this to logged-in sessions only".into(),
    ..e
  })
}

pub fn request<'a>(ctx: &'a Ctx, method: Method, url: &str) -> Req<'a> {
  let req = ctx
    .http
    .request(method, url)
    .header("x-requested-with", "fetch");
  match ctx.http.cookie("_xsrf") {
    Some(xsrf) => req.header("x-xsrftoken", xsrf),
    None => req,
  }
}

pub fn get<'a>(ctx: &'a Ctx, url: &str) -> Req<'a> {
  request(ctx, Method::GET, url)
}

pub fn post<'a>(ctx: &'a Ctx, url: &str) -> Req<'a> {
  request(ctx, Method::POST, url)
}

pub fn delete<'a>(ctx: &'a Ctx, url: &str) -> Req<'a> {
  request(ctx, Method::DELETE, url)
}

/// Send with the logged-in session; see [`call_public`].
pub async fn call(ctx: &Ctx, req: Req<'_>) -> Result<Value> {
  need_login(ctx)?;
  call_public(ctx, req).await
}

/// Send and return the JSON body (`Null` for empty 2xx bodies), or the mapped error.
pub async fn call_public(ctx: &Ctx, req: Req<'_>) -> Result<Value> {
  let resp = req.send().await?;
  let body: Option<Value> = serde_json::from_slice(&resp.body).ok();
  if resp.status.is_success() && body.as_ref().is_none_or(|b| !b["error"].is_object()) {
    return match body {
      Some(v) => Ok(v),
      None if resp.body.iter().all(u8::is_ascii_whitespace) => Ok(Value::Null),
      None => Err(Error::upstream(format!(
        "unexpected response: {}",
        truncate(&one_line(&resp.text()), 200)
      ))),
    };
  }
  let body = body.unwrap_or(Value::Null);
  Err(error(ctx, resp.status.as_u16(), &body, &resp.text()))
}

fn error(ctx: &Ctx, status: u16, body: &Value, text: &str) -> Error {
  let code = body.i64("error.code");
  let message = body
    .first_str(&["error.message", "message"])
    .unwrap_or_else(|| format!("HTTP {status}: {}", truncate(&one_line(text), 200)));
  let logged_in = ctx.http.has_cookie("z_c0");
  let login_hint = ctx.login_hint();
  let need_login = body.bool("error.need_login") == Some(true);
  match (status, code) {
    (_, Some(40352 | 40362)) => {
      let hint = match body.str("error.redirect") {
        Some(url) if logged_in => format!("pass the check in a browser: {url}"),
        Some(url) => format!("{login_hint}, or pass the check in a browser: {url}"),
        None => login_hint,
      };
      Error::new(ErrorCode::VerificationRequired, message).with_hint(hint)
    }
    (_, Some(10003)) if !logged_in => Error::auth(format!(
      "{message} (Zhihu only answers signed x-zse-96 requests anonymously)"
    ))
    .with_hint(login_hint.clone()),
    (_, Some(10003)) => Error::new(ErrorCode::SignatureError, message),
    (401, _) | (_, Some(100 | 101)) => Error::auth(message).with_hint(login_hint.clone()),
    _ if need_login => Error::auth(message).with_hint(login_hint.clone()),
    (403, _) => Error::new(ErrorCode::PermissionDenied, message),
    (404, _) => Error::not_found(message),
    (429, _) => Error::new(ErrorCode::RateLimited, message),
    _ => Error::upstream(message),
  }
}

/// The value of `key` in `paging.next`, unless the listing has ended.
pub fn next_param(v: &Value, key: &str) -> Option<String> {
  let url = next_url(v)?;
  url::Url::parse(&url)
    .ok()?
    .query_pairs()
    .find(|(k, _)| k == key)
    .map(|(_, v)| v.into_owned())
    .filter(|v| !v.is_empty())
}

/// The query string of `paging.next`, unless the listing has ended.
pub fn next_query(v: &Value) -> Option<String> {
  let url = next_url(v)?;
  url.split_once('?').map(|(_, q)| q.to_owned())
}

fn next_url(v: &Value) -> Option<String> {
  if v.bool("paging.is_end").unwrap_or(true) || v.list("data").is_empty() {
    return None;
  }
  let next = v.str("paging.next")?;
  // Some listings return a path relative to the v4 API.
  Some(if next.starts_with('/') {
    format!("{V4}{next}")
  } else {
    next
  })
}
