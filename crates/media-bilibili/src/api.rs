//! Bilibili transport: base headers, device cookies, WBI signing and the
//! `{code, message, data}` envelope.
//!
//! Requests are built with [`get`] / [`post`] and sent with [`Call::send`],
//! which returns the envelope's `data` or a mapped [`Error`].

use std::time::Duration;

use media_core::http::Part;
use media_core::{Ctx, Error, ErrorCode, Http, Result, Value, ValueExt};

use crate::{device, sign};

const HOME: &str = "https://www.bilibili.com";
const NAV: &str = "https://api.bilibili.com/x/web-interface/nav";
const WBI_TTL: Duration = Duration::from_secs(12 * 3600);

/// Session cookies of a logged-in account.
pub const LOGIN_COOKIES: &[&str] = &["SESSDATA", "bili_jct"];

/// Headers every Bilibili request carries. The referer is the bare origin on
/// purpose: with a trailing `/` some endpoints (ranking) answer -352.
pub fn configure(http: &mut Http) {
  http.set_header("referer", HOME);
  http.set_header("origin", HOME);
}

pub fn get<'a>(ctx: &'a Ctx, url: &str) -> Call<'a> {
  Call::new(ctx, url, false)
}

/// A write: form body with `csrf`, requires a login, paced.
pub fn post<'a>(ctx: &'a Ctx, url: &str) -> Call<'a> {
  Call::new(ctx, url, true)
}

pub fn csrf(ctx: &Ctx) -> Result<String> {
  ctx.require_login(LOGIN_COOKIES)?;
  ctx
    .http
    .cookie("bili_jct")
    .ok_or_else(|| Error::auth("missing cookie `bili_jct`"))
}

#[derive(Clone)]
enum Body {
  Form(Vec<(String, String)>),
  Json(Value),
  Multipart(Vec<Part>),
}

#[derive(Clone)]
pub struct Call<'a> {
  ctx: &'a Ctx,
  url: String,
  query: Vec<(String, String)>,
  headers: Vec<(&'static str, String)>,
  body: Option<Body>,
  write: bool,
  wbi: bool,
  dm: bool,
}

impl<'a> Call<'a> {
  fn new(ctx: &'a Ctx, url: &str, write: bool) -> Self {
    Self {
      ctx,
      url: url.to_owned(),
      query: Vec::new(),
      headers: Vec::new(),
      body: write.then(|| Body::Form(Vec::new())),
      write,
      wbi: false,
      dm: false,
    }
  }

  /// Query parameter (GET) or form field (POST).
  pub fn arg(mut self, key: &str, value: impl ToString) -> Self {
    let pair = (key.to_owned(), value.to_string());
    match &mut self.body {
      Some(Body::Form(form)) => form.push(pair),
      _ => self.query.push(pair),
    }
    self
  }

  /// Per-request header (e.g. the referer of another Bilibili site).
  pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
    self.headers.push((name, value.into()));
    self
  }

  /// Sign the query with WBI (`w_rid` / `wts`).
  pub fn wbi(mut self) -> Self {
    self.wbi = true;
    self
  }

  /// Add the `dm_img_*` telemetry parameters (checked by some space / reply endpoints).
  pub fn dm(mut self) -> Self {
    self.dm = true;
    self
  }

  /// Send a JSON body instead of a form (`csrf` goes into the query).
  pub fn json(mut self, body: Value) -> Self {
    self.body = Some(Body::Json(body));
    self
  }

  pub fn multipart(mut self, parts: Vec<Part>) -> Self {
    self.body = Some(Body::Multipart(parts));
    self
  }

  /// Send and return the envelope's `data` (or `result`). A WBI call rejected
  /// as unsigned is retried once with freshly fetched keys.
  pub async fn send(self) -> Result<Value> {
    let (ctx, wbi) = (self.ctx, self.wbi);
    let retry = wbi.then(|| self.clone());
    match (envelope(self.send_raw().await?, wbi), retry) {
      (Err(e), Some(call)) if e.code == ErrorCode::SignatureError => {
        mixin_key(ctx, true).await?;
        envelope(call.send_raw().await?, wbi)
      }
      (result, _) => result,
    }
  }

  /// Send and return the whole JSON response, whatever its `code`.
  pub async fn send_raw(mut self) -> Result<Value> {
    let ctx = self.ctx;
    device::ensure(ctx).await;
    if self.write {
      self.add_csrf()?;
      ctx
        .http
        .pause(Duration::from_millis(400), Duration::from_millis(1200))
        .await;
    }
    let url = self.full_url().await?;
    let mut req = match self.body.take() {
      None => ctx.http.get(url),
      Some(Body::Form(form)) => ctx.http.post(url).form(form),
      Some(Body::Json(body)) => ctx.http.post(url).json(&body),
      Some(Body::Multipart(parts)) => ctx.http.post(url).multipart(parts),
    };
    for (name, value) in &self.headers {
      req = req.header(name, value);
    }
    let resp = req.send().await?;
    if resp.status.as_u16() == 412 {
      return Err(
        Error::new(
          ErrorCode::RateLimited,
          "request blocked by Bilibili (HTTP 412)",
        )
        .with_hint("wait a while before retrying, or log in"),
      );
    }
    resp.check()?.value()
  }

  /// Writes carry the `bili_jct` cookie as `csrf` (in the query for JSON bodies).
  fn add_csrf(&mut self) -> Result<()> {
    let csrf = csrf(self.ctx)?;
    match &mut self.body {
      Some(Body::Form(form)) => {
        form.push(("csrf".into(), csrf.clone()));
        form.push(("csrf_token".into(), csrf));
      }
      Some(Body::Multipart(parts)) => {
        parts.push(Part::text("csrf", csrf.clone()));
        parts.push(Part::text("csrf_token", csrf));
      }
      Some(Body::Json(_)) | None => self.query.push(("csrf".into(), csrf)),
    }
    Ok(())
  }

  /// The URL with its query string, encoded (and signed) like the web client.
  async fn full_url(&mut self) -> Result<String> {
    let mut query = std::mem::take(&mut self.query);
    if self.dm {
      query.extend(sign::dm_params());
    }
    let query = if self.wbi {
      let key = mixin_key(self.ctx, false).await?;
      sign::sign(query, &key, jiff::Timestamp::now().as_second())
    } else {
      sign::query_string(&query)
    };
    let sep = if self.url.contains('?') { '&' } else { '?' };
    Ok(if query.is_empty() {
      self.url.clone()
    } else {
      format!("{}{sep}{query}", self.url)
    })
  }
}

/// `{code, message, data}` -> `data`, or an error with a shared code.
fn envelope(v: Value, wbi: bool) -> Result<Value> {
  let code = v.i64("code").unwrap_or(0);
  if code == 0 {
    // Risk control answers "success" with only a captcha voucher as data.
    if v.str("data.v_voucher").is_some() {
      return Err(
        Error::new(
          ErrorCode::VerificationRequired,
          "blocked by Bilibili risk control (v_voucher)",
        )
        .with_hint("wait a while before retrying, or log in"),
      );
    }
    let data = match v.at("data") {
      Value::Null => v.at("result").clone(),
      d => d.clone(),
    };
    return Ok(data);
  }
  let message = v
    .first_str(&["message", "msg"])
    .unwrap_or_else(|| "unknown error".into());
  let text = format!("{message} ({code})");
  Err(match code {
    -101 | -111 | -2 | 61000 => Error::auth(text).with_hint("run `media bili login`"),
    -352 | 12015 => Error::new(ErrorCode::VerificationRequired, text)
      .with_hint("Bilibili asked for a captcha; retry later or log in"),
    -412 | -509 | -799 | 34004 | 12051 | 12052 => Error::new(ErrorCode::RateLimited, text),
    -403 if wbi => Error::new(ErrorCode::SignatureError, text),
    -403 | -10403 | 12002 | 62012 | 53013 => Error::new(ErrorCode::PermissionDenied, text),
    -404 | 62002 | 62004 | 12022 | 4101131 | 4128002 => Error::not_found(text),
    -400 | -405 => Error::input(text),
    _ => Error::upstream(text),
  })
}

/// The WBI mixin key, cached for 12 hours (refreshed from `nav` when stale).
pub async fn mixin_key(ctx: &Ctx, refresh: bool) -> Result<String> {
  if !refresh && let Some(key) = ctx.store.cache_get::<String>("wbi", WBI_TTL) {
    return Ok(key);
  }
  let nav = ctx.http.get(NAV).value().await?;
  remember_wbi(ctx, &nav)
    .ok_or_else(|| Error::new(ErrorCode::SignatureError, "nav did not announce WBI keys"))
}

/// Cache the WBI keys found in a `nav` response.
pub fn remember_wbi(ctx: &Ctx, nav: &Value) -> Option<String> {
  let img = nav.str("data.wbi_img.img_url")?;
  let sub = nav.str("data.wbi_img.sub_url")?;
  let mixin = sign::mixin_key(sign::key_from_url(&img), sign::key_from_url(&sub));
  ctx.store.cache_put("wbi", &mixin);
  Some(mixin)
}
