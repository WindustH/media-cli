//! Transport for `edith.xiaohongshu.com` and the creator APIs: base headers,
//! signing, device / guest cookies and the `{success, code, msg, data}`
//! envelope mapped onto `media_core::Error`.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use media_core::http::{Req, Resp, status_error};
use media_core::{Ctx, Error, ErrorCode, Result, Value, ValueExt};

use crate::sign::{self, SignSession};

pub const HOME: &str = "https://www.xiaohongshu.com";
pub const EDITH: &str = "https://edith.xiaohongshu.com";
pub const CREATOR: &str = "https://creator.xiaohongshu.com";
const UPLOAD: &str = "https://ros-upload.xiaohongshu.com";

const ACTIVATE: &str = "/api/sns/web/v1/login/activate";

/// Endpoints guarded by `x-rap-param` (xhshow: feed, search and publishing).
const RAP_PATHS: &[&str] = &[
  "/api/sns/web/v1/homefeed",
  "/api/sns/web/v1/feed",
  "/api/sns/web/v1/search/notes",
  "/web_api/sns/v2/note",
];

/// A main-API request: GET parameters or a JSON body sent as signed.
enum Body<'a> {
  Query(&'a [(&'a str, &'a str)]),
  Json(String),
}

pub struct Client {
  pub ctx: Ctx,
  session: RefCell<SignSession>,
  /// Consecutive captcha responses, for the cooldown.
  verify_count: Cell<u32>,
}

impl Client {
  pub fn new(ctx: Ctx) -> Self {
    Self {
      ctx,
      session: RefCell::new(SignSession::new(sign::now_ms())),
      verify_count: Cell::new(0),
    }
  }

  /// Pre-check for endpoints that need an account (visitors only get captchas).
  pub fn require_login(&self) -> Result<()> {
    self
      .ctx
      .require_login(&["a1", "web_session"])
      .map_err(|e| e.with_hint("run `media xhs login`"))
  }

  // ── main API (x-s signing) ───────────────────────────────────────────

  /// Signed GET; `params` keep their order, as they are part of the signature.
  pub async fn get(&self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
    self.send_main(path, Body::Query(params), &[]).await
  }

  pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
    self.post_with(path, body, &[]).await
  }

  /// POST with extra or overriding headers.
  pub async fn post_with(
    &self,
    path: &str,
    body: &Value,
    headers: &[(&str, &str)],
  ) -> Result<Value> {
    self
      .send_main(path, Body::Json(body.to_string()), headers)
      .await
  }

  async fn send_main(&self, path: &str, body: Body<'_>, headers: &[(&str, &str)]) -> Result<Value> {
    let a1 = self.device();
    let now = sign::now_ms();
    // What gets signed: the request target for GET, path + exact body for POST.
    let (target, content, rap_data) = match &body {
      Body::Query(params) => {
        let target = sign::get_content(path, params);
        let data: serde_json::Map<String, Value> = params
          .iter()
          .map(|(k, v)| ((*k).to_owned(), Value::from(*v)))
          .collect();
        (target.clone(), target, Value::Object(data).to_string())
      }
      Body::Json(json) => (path.to_owned(), format!("{path}{json}"), json.clone()),
    };
    let post = matches!(body, Body::Json(_));
    let mut signed = sign::main_headers(
      &mut self.session.borrow_mut(),
      post,
      path,
      &content,
      &a1,
      now,
    );
    if RAP_PATHS.contains(&path) {
      let api = format!("//edith.xiaohongshu.com{path}");
      signed.push(("x-rap-param", sign::x_rap_param(&api, &rap_data, now)));
    }
    let url = format!("{EDITH}{target}");
    let mut req = match body {
      Body::Json(json) => self.ctx.http.post(url).json_text(json),
      Body::Query(_) => self.ctx.http.get(url),
    };
    req = base_headers(req, HOME);
    for (k, v) in signed {
      req = req.header(k, v);
    }
    for (k, v) in headers {
      req = req.header(k, v);
    }
    self.finish(req.send().await?).await
  }

  // ── creator APIs (XYW signing) ───────────────────────────────────────

  pub async fn creator_get(&self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
    let target = sign::get_content(path, params);
    self.send_creator(path, &target, None).await
  }

  pub async fn creator_post(&self, path: &str, body: &Value) -> Result<Value> {
    self.send_creator(path, path, Some(body.to_string())).await
  }

  async fn send_creator(&self, path: &str, target: &str, body: Option<String>) -> Result<Value> {
    let a1 = self.device();
    let now = sign::now_ms();
    let content = format!("url={target}{}", body.as_deref().unwrap_or_default());
    let host = if path.starts_with("/api/galaxy/") {
      CREATOR
    } else {
      EDITH
    };
    let url = format!("{host}{target}");
    let req = match body {
      Some(json) => self.ctx.http.post(url).json_text(json),
      None => self.ctx.http.get(url),
    };
    let req = base_headers(req, CREATOR)
      .header("x-s", sign::xyw(&content, &a1, now))
      .header("x-t", now.to_string());
    self.finish(req.send().await?).await
  }

  /// PUT one file to the upload host with a permit token.
  pub async fn upload(&self, file_id: &str, token: &str, data: Vec<u8>, mime: &str) -> Result<()> {
    self
      .ctx
      .http
      .request(http::Method::PUT, format!("{UPLOAD}/{file_id}"))
      .header("x-cos-security-token", token)
      .bytes(data, mime)
      .no_cookies()
      .send()
      .await?
      .check()
      .map(drop)
  }

  // ── cookies ──────────────────────────────────────────────────────────

  /// The `a1` cookie, created together with `webId` when missing (signing
  /// needs one), like `xhs_cli/qr_login.py`.
  pub fn device(&self) -> String {
    let http = &self.ctx.http;
    if let Some(a1) = http.cookie("a1") {
      return a1;
    }
    let a1 = sign::random::a1(sign::now_ms());
    http.set_cookie("a1", &a1);
    if !http.has_cookie("webId") {
      http.set_cookie("webId", &sign::random::web_id());
    }
    a1
  }

  /// Fresh device cookies and a guest session, as a visitor's browser has
  /// before a QR login. Activation failures are ignored, like the reference.
  pub async fn start_guest(&self) {
    self.ctx.http.replace_cookies(Default::default());
    self.device();
    match self.post(ACTIVATE, &serde_json::json!({})).await {
      Ok(data) => self.apply_session(&data),
      Err(e) => tracing::debug!("guest activation failed: {e}"),
    }
  }

  /// Store the session cookies carried by activation / QR login payloads.
  pub fn apply_session(&self, data: &Value) {
    let http = &self.ctx.http;
    if let Some(s) = data.first_str(&["session", "login_info.session"]) {
      http.set_cookie("web_session", &s);
    }
    if let Some(s) = data.first_str(&["secure_session", "login_info.secure_session"]) {
      http.set_cookie("web_session_sec", &s);
    }
  }

  // ── responses ────────────────────────────────────────────────────────

  async fn finish(&self, resp: Resp) -> Result<Value> {
    let status = resp.status.as_u16();
    if matches!(status, 461 | 471) {
      let n = self.verify_count.get() + 1;
      self.verify_count.set(n);
      // Same cooldown as the reference client: 5 s, doubling, capped at 30 s.
      let cooldown = (5u64 << (n - 1).min(3)).min(30);
      tracing::warn!("captcha triggered (count {n}), cooling down {cooldown} s");
      tokio::time::sleep(Duration::from_secs(cooldown)).await;
      let kind = resp.header("verifytype").unwrap_or("unknown");
      let uuid = resp.header("verifyuuid").unwrap_or("unknown");
      return Err(
        Error::new(
          ErrorCode::VerificationRequired,
          format!("Xiaohongshu requires a captcha (type {kind}, uuid {uuid})"),
        )
        .with_hint(
          "open xiaohongshu.com in a browser with this account, pass the check, then retry",
        ),
      );
    }
    self.verify_count.set(0);
    if resp.body.is_empty() {
      return if resp.status.is_success() {
        Ok(Value::Null)
      } else {
        Err(status_error(resp.status, ""))
      };
    }
    match resp.value() {
      Ok(v) => envelope(v),
      Err(_) if !resp.status.is_success() => Err(status_error(resp.status, &resp.text())),
      Err(e) => Err(e),
    }
  }
}

/// Headers of `xhs_cli/client.py::_base_headers` not already sent by core.
fn base_headers<'a>(req: Req<'a>, origin: &str) -> Req<'a> {
  req
    .header("content-type", "application/json;charset=UTF-8")
    .header("origin", origin)
    .header("referer", format!("{origin}/"))
    .header("sec-fetch-dest", "empty")
    .header("sec-fetch-mode", "cors")
    .header("sec-fetch-site", "same-site")
    .header("dnt", "1")
    .header("priority", "u=1, i")
}

/// `data` of a successful envelope, or the matching error.
fn envelope(v: Value) -> Result<Value> {
  if v.bool("success") == Some(true) {
    return Ok(v.get("data").cloned().unwrap_or(Value::Null));
  }
  let code = v.i64("code");
  let msg = v.first_str(&["msg", "message"]).unwrap_or_default();
  let detail = format!("{msg} (code {})", code.unwrap_or_default());
  Err(match code {
    Some(300012) => Error::new(
      ErrorCode::IpBlocked,
      format!("IP blocked by Xiaohongshu: {detail}"),
    )
    .with_hint("try a different network"),
    Some(300013) => Error::new(
      ErrorCode::RateLimited,
      format!("too many requests: {detail}"),
    ),
    Some(300015) => Error::new(
      ErrorCode::SignatureError,
      format!("signature rejected: {detail}"),
    ),
    // -100: session expired; -101: no login information.
    Some(-100 | -101) => {
      Error::auth(format!("not logged in: {detail}")).with_hint("run `media xhs login`")
    }
    _ => Error::upstream(format!("Xiaohongshu API error: {detail}")),
  })
}
