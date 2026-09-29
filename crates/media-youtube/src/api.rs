//! InnerTube transport: `POST https://www.youtube.com/youtubei/v1/<endpoint>`
//! with a JSON body carrying the client `context`. No API key is needed.
//!
//! - [`Client::Web`] answers everything a browser shows. With a cookie
//!   session it carries the session cookies and the `SAPISIDHASH`
//!   authorization ([`crate::sign`]) the web app sends.
//! - [`Client::AndroidVr`] (the Oculus app) answers `player` with plain,
//!   unciphered stream URLs and caption tracks that return bytes, without a
//!   proof-of-origin token. It does not take cookies, so it always runs
//!   anonymously, with a visitor id: without one YouTube answers
//!   "Sign in to confirm you're not a bot".
//!
//! The visitor id of anonymous calls (`responseContext.visitorData`) is kept
//! in the cache and sent with them; a cookie session keeps its own.
//! Failures map to `media_core::Error`: the `{error: {code, status, message}}`
//! envelope, `ERROR` alerts and the bot check.

use std::cell::RefCell;
use std::time::Duration;

use media_core::http::Resp;
use media_core::{Ctx, Error, ErrorCode, Result, Value, ValueExt};

use crate::sign;

pub const WWW: &str = "https://www.youtube.com";
const HL: &str = "en";
const GL: &str = "US";
const VISITOR_KEY: &str = "visitor";
const VISITOR_TTL: Duration = Duration::from_secs(30 * 86400);
const BOT_HINT: &str = "YouTube asks this network to prove it is not a bot: log in \
  (`media youtube login --browser`), wait a while, or use another network with --proxy";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
  Web,
  AndroidVr,
}

const WEB_VERSION: &str = "2.20260623.01.00";
const VR_VERSION: &str = "1.65.10";
const VR_UA: &str = "com.google.android.apps.youtube.vr.oculus/1.65.10 \
  (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip";

impl Client {
  /// `X-Youtube-Client-Name` and `-Version`; they must match the context.
  fn id(self) -> (&'static str, &'static str) {
    match self {
      Client::Web => ("1", WEB_VERSION),
      Client::AndroidVr => ("28", VR_VERSION),
    }
  }

  fn context(self, visitor: Option<&str>) -> Value {
    let mut client = match self {
      Client::Web => serde_json::json!({
        "clientName": "WEB",
        "clientVersion": WEB_VERSION,
        "platform": "DESKTOP",
      }),
      Client::AndroidVr => serde_json::json!({
        "clientName": "ANDROID_VR",
        "clientVersion": VR_VERSION,
        "androidSdkVersion": 32,
        "deviceMake": "Oculus",
        "deviceModel": "Quest 3",
        "osName": "Android",
        "osVersion": "12L",
        "platform": "MOBILE",
        "userAgent": VR_UA,
      }),
    };
    client["hl"] = HL.into();
    client["gl"] = GL.into();
    if let Some(v) = visitor {
      client["visitorData"] = v.into();
    }
    serde_json::json!({ "client": client, "user": { "lockedSafetyMode": false } })
  }
}

pub struct Api {
  pub ctx: Ctx,
  visitor: RefCell<Option<String>>,
}

impl Api {
  pub fn new(mut ctx: Ctx) -> Self {
    ctx.http.set_header("accept-language", "en-US,en;q=0.9");
    let visitor = ctx.store.cache_get::<String>(VISITOR_KEY, VISITOR_TTL);
    Self {
      ctx,
      visitor: RefCell::new(visitor),
    }
  }

  pub fn logged_in(&self) -> bool {
    sign::SIGNING_COOKIES
      .iter()
      .any(|c| self.ctx.http.has_cookie(c))
  }

  pub fn require_login(&self) -> Result<()> {
    if self.logged_in() {
      Ok(())
    } else {
      Err(Error::auth("not logged in (missing cookie `SAPISID`)").with_hint(self.ctx.login_hint()))
    }
  }

  /// A read as the web client.
  pub async fn call(&self, endpoint: &str, body: Value) -> Result<Value> {
    self.send(Client::Web, endpoint, body, 2).await
  }

  /// A read as another client.
  pub async fn call_as(&self, client: Client, endpoint: &str, body: Value) -> Result<Value> {
    if client == Client::AndroidVr && self.visitor.borrow().is_none() {
      // Cheap (a few hundred bytes); the mobile player refuses callers without one.
      self
        .call("visitor_id", Value::Object(Default::default()))
        .await?;
    }
    self.send(client, endpoint, body, 2).await
  }

  /// A write of the logged-in account: paced, never retried.
  pub async fn write(&self, endpoint: &str, body: Value) -> Result<Value> {
    self.require_login()?;
    self
      .ctx
      .http
      .pause(Duration::from_millis(800), Duration::from_millis(2000))
      .await;
    self.send(Client::Web, endpoint, body, 0).await
  }

  async fn send(
    &self,
    client: Client,
    endpoint: &str,
    mut body: Value,
    retries: u32,
  ) -> Result<Value> {
    // A cookie session has its own visitor; the cached one is for anonymous calls.
    let signed = client == Client::Web && self.logged_in();
    let visitor = self.visitor.borrow().clone().filter(|_| !signed);
    body["context"] = client.context(visitor.as_deref());
    let (id, version) = client.id();
    let http = &self.ctx.http;
    let mut req = http
      .post(format!("{WWW}/youtubei/v1/{endpoint}?prettyPrint=false"))
      .json(&body)
      .header("x-youtube-client-name", id)
      .header("x-youtube-client-version", version)
      .header("origin", WWW)
      .header("referer", format!("{WWW}/"))
      .retries(retries);
    if let Some(v) = &visitor {
      req = req.header("x-goog-visitor-id", v);
    }
    match client {
      Client::Web => {
        if let Some(auth) = sign::authorization(http, WWW) {
          req = req
            .header("authorization", auth)
            .header("x-origin", WWW)
            .header("x-goog-authuser", "0");
        }
      }
      Client::AndroidVr => req = req.no_cookies().header("user-agent", VR_UA),
    }
    let v = self.check(req.send().await?)?;
    if !signed {
      self.remember_visitor(&v);
    }
    Ok(v)
  }

  fn remember_visitor(&self, v: &Value) {
    if self.visitor.borrow().is_some() {
      return;
    }
    if let Some(data) = v.str("responseContext.visitorData") {
      let data = url::form_urlencoded::parse(format!("v={data}").as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or(data);
      self.ctx.store.cache_put(VISITOR_KEY, &data);
      *self.visitor.borrow_mut() = Some(data);
    }
  }

  /// Response → JSON, or a shared error.
  fn check(&self, resp: Resp) -> Result<Value> {
    let json: Option<Value> = serde_json::from_slice(&resp.body).ok();
    if resp.status.is_success() {
      let v = json.ok_or_else(|| {
        Error::upstream(format!(
          "unexpected response: {}",
          media_core::text::truncate(&media_core::text::one_line(&resp.text()), 120)
        ))
      })?;
      return match alert(&v) {
        Some(text) => Err(Error::not_found(text)),
        None => Ok(v),
      };
    }
    let v = json.unwrap_or_default();
    let status = v.str("error.status").unwrap_or_default();
    let message = v
      .str("error.message")
      .unwrap_or_else(|| format!("HTTP {}", resp.status));
    let code = resp.status.as_u16();
    Err(match (code, status.as_str()) {
      (401, _) | (_, "UNAUTHENTICATED") => {
        Error::auth(format!("YouTube did not accept the session: {message}"))
          .with_hint(self.ctx.login_hint())
      }
      (403, _) | (_, "PERMISSION_DENIED") => Error::new(ErrorCode::PermissionDenied, message),
      (404, _) | (_, "NOT_FOUND") => Error::not_found(message),
      (429, _) | (_, "RESOURCE_EXHAUSTED") => Error::new(
        ErrorCode::RateLimited,
        format!("YouTube rate limit: {message}"),
      )
      .with_hint("wait a few minutes before retrying"),
      _ => Error::upstream(format!(
        "YouTube refused the request ({code} {status}): {message}"
      )),
    })
  }
}

/// The text of an `ERROR` alert (a channel or playlist that does not exist).
fn alert(v: &Value) -> Option<String> {
  v.list("alerts").iter().find_map(|a| {
    let a = a.at("alertRenderer");
    (a.str("type").as_deref() == Some("ERROR"))
      .then(|| crate::parse::text(a.at("text")))
      .flatten()
  })
}

/// Whether YouTube served the request to a logged-in account.
pub fn served_logged_in(v: &Value) -> bool {
  v.list("responseContext.serviceTrackingParams")
    .iter()
    .flat_map(|s| s.list("params"))
    .any(|p| p.str("key").as_deref() == Some("logged_in") && p.str("value").as_deref() == Some("1"))
}

/// `playabilityStatus` of a player response: `Ok` when it plays, else an error.
pub fn playable(v: &Value) -> Result<()> {
  let p = v.at("playabilityStatus");
  let status = p.str("status").unwrap_or_default();
  let reason = p
    .str("reason")
    .or_else(|| crate::parse::text(p.at("errorScreen.playerErrorMessageRenderer.subreason")))
    .unwrap_or_else(|| status.to_lowercase());
  match status.as_str() {
    "OK" => Ok(()),
    "LOGIN_REQUIRED" if reason.contains("bot") => {
      Err(Error::new(ErrorCode::VerificationRequired, reason).with_hint(BOT_HINT))
    }
    "LOGIN_REQUIRED" => Err(Error::auth(reason)),
    "ERROR" => Err(Error::not_found(reason)),
    _ => Err(Error::new(ErrorCode::PermissionDenied, reason)),
  }
}
