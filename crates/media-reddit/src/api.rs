//! Reddit transport. The one place that knows which of two modes is active:
//!
//! - **Web** (default): `https://www.reddit.com/<path>.json?raw_json=1`, with
//!   the browser session cookies (`reddit_session`) when logged in and
//!   anonymously otherwise. Writes use the web app's own bearer token, the
//!   `token_v2` cookie (a JWT living about a day, renewed by loading the home
//!   page), against `oauth.reddit.com`: the classic cookie API answers
//!   submissions with `BAD_CAPTCHA`. Without a usable `token_v2`, writes fall
//!   back to the classic API on www with the account's `modhash` (from
//!   `/api/me.json`) as `X-Modhash` header and `uh` field.
//! - **OAuth**: with `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`,
//!   `REDDIT_USERNAME` and `REDDIT_PASSWORD` set, a password-grant token
//!   ([`crate::oauth`]) goes as `Authorization: bearer` to
//!   `https://oauth.reddit.com/<path>`. It wins over saved cookies.
//!
//! Paths are the same in both modes, so callers only name the endpoint.
//! Failures are mapped to `media_core::Error`: 429, Reddit's "blocked by
//! network security" page, private / quarantined / banned communities and the
//! `{"json": {"errors": [...]}}` envelope of writes.

use std::cell::RefCell;
use std::time::Duration;

use media_core::http::{Method, Req, Resp, status_error};
use media_core::{Ctx, Error, ErrorCode, Result, Value, ValueExt};

use crate::oauth::Credentials;

pub const WWW: &str = "https://www.reddit.com";
const OAUTH: &str = "https://oauth.reddit.com";
/// Cookie of a logged-in browser session.
pub const SESSION_COOKIE: &str = "reddit_session";
/// Session extra holding the account name, so listings of your own saved /
/// upvoted posts need no extra lookup.
const USER_KEY: &str = "username";
const BLOCK_HINT: &str = "Reddit refuses anonymous API requests from some networks: log in \
   (`media reddit login --browser`), set REDDIT_CLIENT_ID / REDDIT_CLIENT_SECRET / \
   REDDIT_USERNAME / REDDIT_PASSWORD, or retry through another network with --proxy";

/// Query parameters or form fields.
pub type Params<'a> = Vec<(&'a str, String)>;

enum Body<'a> {
  Form(Params<'a>),
  Json(Value),
}

pub struct Api {
  pub ctx: Ctx,
  oauth: Option<Credentials>,
  modhash: RefCell<Option<String>>,
}

impl Api {
  pub fn new(mut ctx: Ctx) -> Self {
    ctx.http.set_header("accept-language", "en-US,en;q=0.9");
    Self {
      ctx,
      oauth: Credentials::from_env(),
      modhash: RefCell::new(None),
    }
  }

  pub fn logged_in(&self) -> bool {
    self.oauth.is_some() || self.ctx.http.has_cookie(SESSION_COOKIE)
  }

  pub fn require_login(&self) -> Result<()> {
    match self.oauth {
      Some(_) => Ok(()),
      None => self.ctx.require_login(&[SESSION_COOKIE]),
    }
  }

  /// GET an endpoint given without host and `.json` (`/r/rust/hot`).
  pub async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
    let resp = self
      .request(Method::GET, path)
      .await?
      .queries(query.iter().map(|(k, v)| (*k, v)))
      .query("raw_json", 1)
      .send()
      .await?;
    // Unknown communities redirect to a community search.
    if path.starts_with("/r/") && resp.url.contains("/subreddits/search") {
      let name = path.split('/').nth(2).unwrap_or_default();
      return Err(Error::not_found(format!("no such subreddit: r/{name}")));
    }
    self.check(resp)
  }

  /// POST a form to the classic API: a write (login, pause, CSRF, `api_type=json`).
  pub async fn post(&self, path: &str, form: Params<'_>) -> Result<Value> {
    self.write(path, Body::Form(form)).await
  }

  /// POST a JSON body (endpoints such as `/api/submit_gallery_post.json`).
  pub async fn post_json(&self, path: &str, body: Value) -> Result<Value> {
    self.write(path, Body::Json(body)).await
  }

  async fn write(&self, path: &str, body: Body<'_>) -> Result<Value> {
    self.require_login()?;
    match self.send_write(path, &body, self.web_token().await).await {
      // The classic cookie API wants a captcha for some writes (submissions);
      // its responses often hand out the `token_v2` bearer, so retry with it.
      // Nothing was created by the refused attempt.
      Err(e) if e.message.contains("BAD_CAPTCHA") => match self.web_token().await {
        Some(token) => self.send_write(path, &body, Some(token)).await,
        None => Err(e),
      },
      other => other,
    }
  }

  /// One write: with the web bearer when given, else with the active mode.
  async fn send_write(&self, path: &str, body: &Body<'_>, token: Option<String>) -> Result<Value> {
    let (mut req, modhash) = match token {
      Some(token) => (self.bearer(Method::POST, path, &token), None),
      None => {
        let modhash = match self.oauth {
          Some(_) => None,
          None => Some(self.modhash().await?),
        };
        (self.request(Method::POST, path).await?, modhash)
      }
    };
    req = match body {
      Body::Json(v) => req.json(v),
      Body::Form(form) => {
        let mut form = form.clone();
        form.push(("api_type", "json".into()));
        if let Some(m) = &modhash {
          form.push(("uh", m.clone()));
        }
        req.form(form)
      }
    };
    if let Some(m) = &modhash {
      req = req.header("x-modhash", m);
    }
    self
      .ctx
      .http
      .pause(Duration::from_millis(600), Duration::from_millis(1800))
      .await;
    let v = self.check(req.send().await?)?;
    api_errors(&v)?;
    Ok(v)
  }

  /// A request to the active host with the active credentials.
  async fn request(&self, method: Method, path: &str) -> Result<Req<'_>> {
    let http = &self.ctx.http;
    if let Some(o) = &self.oauth {
      return Ok(
        self
          .bearer(method, path, &o.token(&self.ctx).await?)
          .header("user-agent", o.user_agent()),
      );
    }
    Ok(if method == Method::GET {
      http.get(format!("{WWW}{}.json", path.trim_end_matches('/')))
    } else {
      http
        .request(method, format!("{WWW}{path}"))
        .header("origin", WWW)
        .header("referer", format!("{WWW}/"))
    })
  }

  /// A request to `oauth.reddit.com` carrying `token`.
  fn bearer(&self, method: Method, path: &str, token: &str) -> Req<'_> {
    self
      .ctx
      .http
      .request(method, format!("{OAUTH}{path}"))
      .no_cookies()
      .header("authorization", format!("bearer {token}"))
  }

  /// The web app's bearer token (`token_v2`) of a cookie session while it is
  /// valid; an expired one is renewed by loading the home page once.
  async fn web_token(&self) -> Option<String> {
    if self.oauth.is_some() || !self.ctx.http.has_cookie(SESSION_COOKIE) {
      return None;
    }
    let valid = || self.ctx.http.cookie("token_v2").filter(|t| !jwt_expired(t));
    if let Some(token) = valid() {
      return Some(token);
    }
    tracing::debug!(
      "token_v2 {}; loading the home page to renew it",
      if self.ctx.http.has_cookie("token_v2") {
        "expired"
      } else {
        "missing"
      }
    );
    let renew = self
      .ctx
      .http
      .get(format!("{WWW}/"))
      .header("accept", "text/html,application/xhtml+xml")
      .send()
      .await;
    if let Err(e) = renew {
      tracing::debug!("token_v2 renewal failed: {e}");
    }
    let token = valid();
    tracing::debug!(
      "token_v2 after renewal: {}",
      if token.is_some() {
        "valid"
      } else {
        "unavailable"
      }
    );
    token
  }

  /// The logged-in account: `/api/me` (cookie session) or `/api/v1/me` (OAuth).
  pub async fn me(&self) -> Result<Value> {
    self.require_login()?;
    let me = match self.oauth {
      Some(_) => self.get("/api/v1/me", &[]).await?,
      None => self.get("/api/me", &[]).await?.at("data").clone(),
    };
    // An expired cookie session answers `{}`.
    let Some(name) = me.str("name") else {
      return Err(
        Error::auth("Reddit did not accept this session").with_hint(self.ctx.login_hint()),
      );
    };
    if let Some(m) = me.str("modhash") {
      *self.modhash.borrow_mut() = Some(m);
    }
    if self.oauth.is_none() {
      self.ctx.set_extra(USER_KEY, &name);
    }
    Ok(me)
  }

  /// Name of the logged-in account.
  pub async fn username(&self) -> Result<String> {
    if let Some(o) = &self.oauth {
      return Ok(o.username.clone());
    }
    self.require_login()?;
    match self.ctx.extra(USER_KEY).filter(|n| !n.is_empty()) {
      Some(name) => Ok(name),
      None => Ok(self.me().await?.str("name").unwrap_or_default()),
    }
  }

  /// Forget values of the previous account before new cookies are verified.
  pub fn reset_account(&self) {
    self.ctx.set_extra(USER_KEY, "");
    *self.modhash.borrow_mut() = None;
  }

  async fn modhash(&self) -> Result<String> {
    if self.modhash.borrow().is_none() {
      self.me().await?;
    }
    self.modhash.borrow().clone().ok_or_else(|| {
      Error::auth("Reddit sent no modhash for this session").with_hint(self.ctx.login_hint())
    })
  }

  /// Response → JSON, or a shared error.
  fn check(&self, resp: Resp) -> Result<Value> {
    let status = resp.status.as_u16();
    // Some writes answer `202 Accepted` without a body.
    if resp.status.is_success() && resp.body.iter().all(u8::is_ascii_whitespace) {
      return Ok(Value::Null);
    }
    let json: Option<Value> = serde_json::from_slice(&resp.body).ok();
    match json {
      Some(v) if resp.status.is_success() => return Ok(v),
      None if resp.status.is_success() => return Err(web_page(&resp)),
      _ => {}
    }
    if status == 401 {
      let hint = match &self.oauth {
        Some(o) => {
          o.forget(&self.ctx);
          "the OAuth token was refused; the next run asks for a new one".to_owned()
        }
        None => self.ctx.login_hint(),
      };
      return Err(Error::auth("Reddit did not accept the credentials (HTTP 401)").with_hint(hint));
    }
    Err(match (status, json) {
      (429, _) => Error::new(
        ErrorCode::RateLimited,
        "Reddit rate limit reached (HTTP 429)",
      )
      .with_hint("wait a minute before retrying; logged-in requests get a higher limit"),
      (_, None) if is_block_page(&resp) => blocked(),
      (_, Some(v)) => {
        refused(status, &v).unwrap_or_else(|| status_error(resp.status, &resp.text()))
      }
      _ => status_error(resp.status, &resp.text()),
    })
  }
}

fn is_block_page(resp: &Resp) -> bool {
  let text = resp.text().to_ascii_lowercase();
  text.contains("blocked by network security") || text.contains("blocked due to a network policy")
}

fn blocked() -> Error {
  Error::new(
    ErrorCode::VerificationRequired,
    "Reddit blocked this request (network security)",
  )
  .with_hint(BLOCK_HINT)
}

/// A web page where JSON was expected: a block page or a login redirect.
fn web_page(resp: &Resp) -> Error {
  if is_block_page(resp) {
    return blocked();
  }
  if resp.url.contains("/login") {
    return Error::auth("Reddit asked for a login").with_hint(BLOCK_HINT);
  }
  Error::upstream(format!(
    "unexpected response from {}: {}",
    resp.url.split('?').next().unwrap_or_default(),
    media_core::text::truncate(&media_core::text::one_line(&resp.text()), 120)
  ))
}

/// `{"reason": "private", "message": "Forbidden", "error": 403}` and friends.
fn refused(status: u16, v: &Value) -> Option<Error> {
  let reason = v.str("reason")?;
  let denied = |message: &str| Error::new(ErrorCode::PermissionDenied, message);
  Some(match reason.as_str() {
    "private" => denied("this community is private"),
    "quarantined" | "gated" => denied(&format!("this community is {reason}"))
      .with_hint("opt in to view it on the website first, then log in here"),
    "gold_only" => denied("this community is for Reddit Premium members only"),
    "banned" => Error::not_found("this community was banned"),
    _ => Error::upstream(format!(
      "Reddit refused the request: {reason} (HTTP {status})"
    )),
  })
}

/// The `{"json": {"errors": [[CODE, message, field], ...]}}` envelope of writes.
fn api_errors(v: &Value) -> Result<()> {
  let Some(e) = v.list("json.errors").first() else {
    return Ok(());
  };
  let code = e.str("0").unwrap_or_default();
  let text = match e.str("1") {
    Some(message) => format!("{message} ({code})"),
    None => code.clone(),
  };
  Err(match code.as_str() {
    "RATELIMIT" => Error::new(ErrorCode::RateLimited, text),
    "USER_REQUIRED" | "INVALID_USER" => Error::auth(text),
    "BAD_CAPTCHA" => Error::new(ErrorCode::VerificationRequired, text),
    "SUBREDDIT_NOEXIST" | "NO_THING_ID" | "DELETED_COMMENT" | "DELETED_LINK"
    | "USER_DOESNT_EXIST" => Error::not_found(text),
    "THREAD_LOCKED"
    | "TOO_OLD"
    | "SUBREDDIT_NOTALLOWED"
    | "SUBREDDIT_NO_ACCESS"
    | "BANNED_FROM_SUBREDDIT"
    | "NOT_AUTHOR"
    | "NOT_ALLOWED" => Error::new(ErrorCode::PermissionDenied, text),
    _ => Error::input(text),
  })
}

/// Whether a JWT's `exp` is less than a minute away (or unreadable).
fn jwt_expired(token: &str) -> bool {
  use base64::Engine;
  use base64::engine::general_purpose::URL_SAFE_NO_PAD;
  let exp = token
    .split('.')
    .nth(1)
    .and_then(|p| URL_SAFE_NO_PAD.decode(p.trim_end_matches('=')).ok())
    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
    .and_then(|v| v.i64("exp"));
  exp.is_none_or(|exp| exp - 60 <= jiff::Timestamp::now().as_second())
}
