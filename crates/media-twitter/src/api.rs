//! Transport: web-client headers (bearer, csrf, guest token, transaction id),
//! GraphQL / REST calls with one retry on stale ids or guest tokens, and
//! upstream errors mapped to `media_core::Error`.

use std::cell::RefCell;
use std::time::Duration;

use media_core::http::{Req, Resp, status_error};
use media_core::{Ctx, Error, ErrorCode, Result, Value, ValueExt, json};

use crate::graphql::Op;
use crate::web::Web;

/// The public bearer token of the web client.
const BEARER: &str = "Bearer AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";
const GRAPHQL: &str = "https://x.com/i/api/graphql";
pub const REST: &str = "https://x.com/i/api/1.1";
const GUEST_ACTIVATE: &str = "https://api.x.com/1.1/guest/activate.json";
const GUEST_KEY: &str = "guest-token";
const GUEST_TTL: Duration = Duration::from_secs(2 * 3600);
const LOGIN_HINT: &str = "log in with `media twitter login --cookie 'auth_token=...; ct0=...'`";

pub struct Api {
  pub ctx: Ctx,
  web: Web,
  guest: RefCell<Option<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
  Get,
  Post,
}

impl Api {
  pub fn new(ctx: Ctx) -> Self {
    Self {
      ctx,
      web: Web::default(),
      guest: RefCell::new(None),
    }
  }

  pub fn logged_in(&self) -> bool {
    self.ctx.http.has_cookie("auth_token")
  }

  pub fn require_login(&self) -> Result<()> {
    self.ctx.require_login(&["auth_token", "ct0"])
  }

  /// The logged-in user id, from the `twid` cookie (`u=123`) when present.
  pub fn own_id(&self) -> Option<String> {
    let twid = self.ctx.http.cookie("twid")?;
    let twid = twid.replace("%3D", "=");
    let id = twid.trim_matches('"').strip_prefix("u=")?;
    id.bytes()
      .all(|b| b.is_ascii_digit())
      .then(|| id.to_owned())
  }

  /// Random pause before a write, as the reference client does.
  pub async fn write_pause(&self) {
    self
      .ctx
      .http
      .pause(Duration::from_millis(1500), Duration::from_millis(4000))
      .await;
  }

  /// Run a GraphQL operation; `data` of the response on success.
  pub async fn graphql(&self, op: &Op, variables: Value) -> Result<Value> {
    if !self.logged_in() && !op.guest {
      self.require_login()?;
    }
    let mut refreshed = false;
    let mut renewed = false;
    loop {
      let id = self.web.query_id(&self.ctx, op);
      let url = format!("{GRAPHQL}/{id}/{}", op.name);
      let req = if op.post {
        let mut body = json!({ "variables": variables, "queryId": id });
        if !op.features.is_empty() {
          body["features"] = op.features(false);
        }
        self.request(Method::Post, &url).await?.json(&body)
      } else {
        let mut req = self
          .request(Method::Get, &url)
          .await?
          .query("variables", &variables);
        if !op.features.is_empty() {
          req = req.query("features", op.features(true));
        }
        if let Some(t) = op.field_toggles() {
          req = req.query("fieldToggles", t);
        }
        req
      };
      let resp = req.send().await?;
      if resp.status.as_u16() == 404 && !refreshed {
        refreshed = true;
        if self.web.refresh(&self.ctx).await {
          tracing::debug!("{} returned 404; retrying with refreshed ids", op.name);
          continue;
        }
      }
      if self.should_renew_guest(&resp, &mut renewed) {
        continue;
      }
      if resp.status.as_u16() == 404 {
        let hint = if self.logged_in() {
          "its query id may be outdated; try again later"
        } else {
          "it may need a logged-in session: `media twitter login`"
        };
        return Err(
          Error::not_found(format!("X rejected `{}` (HTTP 404)", op.name)).with_hint(hint),
        );
      }
      return check(resp);
    }
  }

  /// GET a REST (v1.1) endpoint.
  pub async fn get(&self, url: &str, query: &[(&str, &str)]) -> Result<Value> {
    let mut renewed = false;
    loop {
      let resp = self
        .request(Method::Get, url)
        .await?
        .queries(query.iter().copied())
        .send()
        .await?;
      if self.should_renew_guest(&resp, &mut renewed) {
        continue;
      }
      return check(resp);
    }
  }

  /// POST a form to a REST (v1.1 / upload) endpoint.
  pub async fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<Value> {
    let resp = self
      .request(Method::Post, url)
      .await?
      .form(form.iter().copied())
      .send()
      .await?;
    check(resp)
  }

  /// A request carrying the web client's headers.
  async fn request(&self, method: Method, url: &str) -> Result<Req<'_>> {
    let http = &self.ctx.http;
    let (req, verb) = match method {
      Method::Get => (http.get(url), "GET"),
      Method::Post => (http.post(url), "POST"),
    };
    let parsed = url::Url::parse(url).ok();
    let path = parsed.as_ref().map(|u| u.path()).unwrap_or_default();
    let site = match parsed.as_ref().and_then(|u| u.host_str()) {
      Some("x.com") => "same-origin",
      Some(h) if h.ends_with(".x.com") => "same-site",
      _ => "cross-site",
    };
    let mut req = req
      .header("authorization", BEARER)
      .header("accept", "*/*")
      .header("origin", "https://x.com")
      .header("referer", "https://x.com/")
      .header("x-twitter-active-user", "yes")
      .header("x-twitter-client-language", "en")
      .header("sec-fetch-dest", "empty")
      .header("sec-fetch-mode", "cors")
      .header("sec-fetch-site", site);
    if self.logged_in() {
      let csrf = http.cookie("ct0").unwrap_or_default();
      req = req
        .header("x-csrf-token", csrf)
        .header("x-twitter-auth-type", "OAuth2Session");
    } else {
      req = req.header("x-guest-token", self.guest_token().await?);
    }
    if let Some(m) = self.web.material(&self.ctx).await {
      req = req.header("x-client-transaction-id", m.transaction_id(verb, path));
    }
    Ok(req)
  }

  async fn guest_token(&self) -> Result<String> {
    if let Some(t) = self.guest.borrow().clone() {
      return Ok(t);
    }
    let token = match self.ctx.store.cache_get::<String>(GUEST_KEY, GUEST_TTL) {
      Some(t) => t,
      None => {
        let v = self
          .ctx
          .http
          .post(GUEST_ACTIVATE)
          .header("authorization", BEARER)
          .send()
          .await?;
        let token = check(v)?
          .str("guest_token")
          .ok_or_else(|| Error::upstream("no guest token in the activation response"))?;
        self.ctx.store.cache_put(GUEST_KEY, &token);
        token
      }
    };
    *self.guest.borrow_mut() = Some(token.clone());
    Ok(token)
  }

  /// A guest token was refused: drop it so the retry activates a new one (once).
  fn should_renew_guest(&self, resp: &Resp, renewed: &mut bool) -> bool {
    let refused = matches!(resp.status.as_u16(), 401 | 403) || error_code(&resp.body) == Some(239);
    if self.logged_in() || *renewed || !refused {
      return false;
    }
    *renewed = true;
    *self.guest.borrow_mut() = None;
    self.ctx.store.cache_put(GUEST_KEY, &Value::Null);
    true
  }
}

fn error_code(body: &[u8]) -> Option<i64> {
  let v: Value = serde_json::from_slice(body).ok()?;
  v.i64("errors.0.code")
}

/// Map an upstream response to data or a shared error.
pub fn check(resp: Resp) -> Result<Value> {
  if resp.status.is_success() && resp.body.iter().all(u8::is_ascii_whitespace) {
    return Ok(Value::Null);
  }
  let body: Option<Value> = serde_json::from_slice(&resp.body).ok();
  if let Some(err) = body
    .as_ref()
    .and_then(|v| known_error(v, !resp.status.is_success()))
  {
    return Err(err);
  }
  if !resp.status.is_success() {
    let text = resp.text();
    return Err(status_error(resp.status, &text));
  }
  let v = body.ok_or_else(|| {
    Error::upstream(format!(
      "unexpected response: {}",
      media_core::text::truncate(&resp.text(), 200)
    ))
  })?;
  Ok(v)
}

/// The upstream error of a failed response, a response without data, or a
/// mutation payload (`data.<op>.errors`). Top-level errors next to data are
/// partial GraphQL errors (hidden fields ...) and are ignored.
fn known_error(v: &Value, failed: bool) -> Option<Error> {
  let inner = v
    .at("data")
    .as_object()
    .into_iter()
    .flatten()
    .find_map(|(_, d)| d.list("errors").first());
  let empty_data = match v.at("data") {
    Value::Object(m) => m.is_empty(),
    Value::Null => true,
    _ => false,
  };
  let err = match (v.list("errors").first(), inner) {
    (Some(e), _) if failed || empty_data => e,
    (_, Some(e)) => e,
    _ => return None,
  };
  let code = err.i64("code").unwrap_or_default();
  let message = err.str("message").unwrap_or_else(|| "unknown error".into());
  let mapped = match code {
    88 | 185 | 344 | 348 | 349 => Error::new(ErrorCode::RateLimited, message),
    226 => Error::new(ErrorCode::VerificationRequired, message).with_hint(
      "X flagged this request as automated; open x.com in a browser, pass any check there and try again later",
    ),
    326 => Error::new(ErrorCode::VerificationRequired, message)
      .with_hint("the account is locked; unlock it at x.com"),
    32 | 89 | 215 | 239 | 353 => Error::auth(message).with_hint(LOGIN_HINT),
    64 | 37 | 179 | 220 => Error::new(ErrorCode::PermissionDenied, message),
    186 => Error::input(format!("the text is too long ({message})")),
    187 => Error::input(format!("duplicate post ({message})")),
    139 | 327 => Error::input(message),
    8 | 34 | 50 | 63 | 144 | 421 => Error::not_found(message),
    // Let the HTTP status speak for unknown codes of failed responses.
    _ if failed => return None,
    _ => Error::upstream(message),
  };
  Some(mapped)
}
