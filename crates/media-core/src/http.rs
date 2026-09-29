//! Shared HTTP transport.
//!
//! One Chrome-fingerprinted client per platform with its own cookie jar,
//! request throttling, retry with backoff and streaming downloads. Platforms
//! build requests with [`Http::get`] / [`Http::post`] and interpret the
//! response envelope themselves.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use rand::Rng;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use wreq::header::{HeaderMap, HeaderName, HeaderValue};
use wreq::{Method, StatusCode};

use crate::error::{Error, ErrorCode, Result};

/// Chrome build the TLS/HTTP2 fingerprint imitates; keep headers consistent with it.
pub const CHROME_VERSION: u32 = 149;
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36";
pub const SEC_CH_UA: &str = r#""Google Chrome";v="149", "Chromium";v="149", "Not)A;Brand";v="24""#;
pub const SEC_CH_UA_PLATFORM: &str = r#""macOS""#;

pub type Cookies = BTreeMap<String, String>;

#[derive(Debug, Clone)]
pub struct HttpConfig {
  pub proxy: Option<String>,
  pub timeout: Duration,
  /// Minimum gap between two throttled requests (jitter is added on top).
  pub min_interval: Duration,
}

impl Default for HttpConfig {
  fn default() -> Self {
    Self {
      proxy: None,
      timeout: Duration::from_secs(30),
      min_interval: Duration::ZERO,
    }
  }
}

pub struct Http {
  client: wreq::Client,
  headers: HeaderMap,
  jar: RefCell<Cookies>,
  dirty: Cell<bool>,
  last: Cell<Option<Instant>>,
  min_interval: Duration,
}

impl Http {
  pub fn new(cfg: &HttpConfig, cookies: Cookies) -> Result<Self> {
    let emulation = wreq_util::Emulation::builder()
      .profile(wreq_util::Profile::Chrome149)
      .platform(wreq_util::Platform::MacOS)
      .headers(false)
      .build();
    let mut builder = wreq::Client::builder()
      .emulation(emulation)
      .timeout(cfg.timeout)
      .connect_timeout(Duration::from_secs(15))
      .redirect(wreq::redirect::Policy::limited(10));
    if let Some(proxy) = &cfg.proxy {
      builder = builder.proxy(
        wreq::Proxy::all(proxy.as_str()).map_err(|e| Error::input(format!("bad proxy: {e}")))?,
      );
    }
    let client = builder
      .build()
      .map_err(|e| Error::internal(format!("http client: {e}")))?;

    let mut headers = HeaderMap::new();
    for (k, v) in [
      ("user-agent", USER_AGENT),
      ("sec-ch-ua", SEC_CH_UA),
      ("sec-ch-ua-mobile", "?0"),
      ("sec-ch-ua-platform", SEC_CH_UA_PLATFORM),
      ("accept", "application/json, text/plain, */*"),
      ("accept-language", "zh-CN,zh;q=0.9,en;q=0.8"),
    ] {
      headers.insert(HeaderName::from_static(k), HeaderValue::from_static(v));
    }
    Ok(Self {
      client,
      headers,
      jar: RefCell::new(cookies),
      dirty: Cell::new(false),
      last: Cell::new(None),
      min_interval: cfg.min_interval,
    })
  }

  /// Set (or with an empty value, remove) a header sent with every request.
  pub fn set_header(&mut self, name: &str, value: &str) {
    let name = HeaderName::from_bytes(name.as_bytes()).expect("valid header name");
    if value.is_empty() {
      self.headers.remove(name);
    } else {
      self.headers.insert(
        name,
        HeaderValue::from_str(value).expect("valid header value"),
      );
    }
  }

  pub fn get(&self, url: impl Into<String>) -> Req<'_> {
    self.request(Method::GET, url)
  }

  pub fn post(&self, url: impl Into<String>) -> Req<'_> {
    self.request(Method::POST, url)
  }

  pub fn request(&self, method: Method, url: impl Into<String>) -> Req<'_> {
    let retries = if method == Method::GET { 2 } else { 0 };
    Req {
      http: self,
      method,
      url: url.into(),
      query: Vec::new(),
      headers: HeaderMap::new(),
      body: Body::Empty,
      cookies: true,
      throttle: true,
      retries,
    }
  }

  // ── cookie jar ────────────────────────────────────────────────────────

  pub fn cookie(&self, name: &str) -> Option<String> {
    self
      .jar
      .borrow()
      .get(name)
      .filter(|v| !v.is_empty())
      .cloned()
  }

  pub fn has_cookie(&self, name: &str) -> bool {
    self.cookie(name).is_some()
  }

  pub fn set_cookie(&self, name: &str, value: &str) {
    let mut jar = self.jar.borrow_mut();
    if jar.get(name).map(String::as_str) != Some(value) {
      jar.insert(name.to_owned(), value.to_owned());
      self.dirty.set(true);
    }
  }

  pub fn remove_cookie(&self, name: &str) {
    if self.jar.borrow_mut().remove(name).is_some() {
      self.dirty.set(true);
    }
  }

  pub fn cookies(&self) -> Cookies {
    self.jar.borrow().clone()
  }

  /// Replace the whole jar (after a login).
  pub fn replace_cookies(&self, cookies: Cookies) {
    *self.jar.borrow_mut() = cookies;
    self.dirty.set(true);
  }

  /// `a=1; b=2` for the current jar.
  pub fn cookie_header(&self) -> String {
    self
      .jar
      .borrow()
      .iter()
      .map(|(k, v)| format!("{k}={v}"))
      .collect::<Vec<_>>()
      .join("; ")
  }

  /// Whether the jar changed since the session was loaded.
  pub fn cookies_changed(&self) -> bool {
    self.dirty.get()
  }

  // ── pacing ────────────────────────────────────────────────────────────

  /// Sleep a random duration in `[min, max]`; used before write operations.
  pub async fn pause(&self, min: Duration, max: Duration) {
    let ms = rand::rng()
      .random_range(min.as_millis() as u64..=max.as_millis().max(min.as_millis()) as u64);
    tokio::time::sleep(Duration::from_millis(ms)).await;
  }

  async fn throttle(&self) {
    if self.min_interval.is_zero() {
      return;
    }
    if let Some(last) = self.last.get() {
      let elapsed = last.elapsed();
      if elapsed < self.min_interval {
        let mut rng = rand::rng();
        // Mostly short jitter, occasionally a longer "reading" pause.
        let mut jitter = rng.random_range(100..500);
        if rng.random_bool(0.05) {
          jitter += rng.random_range(2000..5000);
        }
        tokio::time::sleep(self.min_interval - elapsed + Duration::from_millis(jitter)).await;
      }
    }
  }

  fn absorb_cookies(&self, resp: &wreq::Response) {
    let mut jar = self.jar.borrow_mut();
    for c in resp.cookies() {
      let expired = c.max_age().is_some_and(|a| a.is_zero());
      if c.value().is_empty() || c.value() == "deleted" || expired {
        if jar.remove(c.name()).is_some() {
          self.dirty.set(true);
        }
      } else if jar.get(c.name()).map(String::as_str) != Some(c.value()) {
        jar.insert(c.name().to_owned(), c.value().to_owned());
        self.dirty.set(true);
      }
    }
  }
}

#[derive(Clone)]
enum Body {
  Empty,
  Bytes(Vec<u8>, String),
  Multipart(Vec<Part>),
}

/// One field of a multipart form.
#[derive(Clone)]
pub struct Part {
  pub name: String,
  pub data: Vec<u8>,
  pub file_name: Option<String>,
  pub mime: Option<String>,
}

impl Part {
  pub fn text(name: &str, value: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      data: value.into().into_bytes(),
      file_name: None,
      mime: None,
    }
  }

  pub fn file(name: &str, data: Vec<u8>, file_name: &str, mime: &str) -> Self {
    Self {
      name: name.into(),
      data,
      file_name: Some(file_name.into()),
      mime: Some(mime.into()),
    }
  }
}

/// A request under construction. Cheap to rebuild, so retries resend it.
pub struct Req<'a> {
  http: &'a Http,
  method: Method,
  url: String,
  query: Vec<(String, String)>,
  headers: HeaderMap,
  body: Body,
  cookies: bool,
  throttle: bool,
  retries: u32,
}

impl<'a> Req<'a> {
  pub fn query(mut self, key: &str, value: impl ToString) -> Self {
    self.query.push((key.to_owned(), value.to_string()));
    self
  }

  pub fn queries<K: AsRef<str>, V: ToString>(
    mut self,
    pairs: impl IntoIterator<Item = (K, V)>,
  ) -> Self {
    for (k, v) in pairs {
      self.query.push((k.as_ref().to_owned(), v.to_string()));
    }
    self
  }

  pub fn header(mut self, name: &str, value: impl AsRef<str>) -> Self {
    if let (Ok(n), Ok(v)) = (
      HeaderName::from_bytes(name.as_bytes()),
      HeaderValue::from_str(value.as_ref()),
    ) {
      self.headers.insert(n, v);
    }
    self
  }

  /// Compact JSON body.
  pub fn json<T: Serialize + ?Sized>(self, body: &T) -> Self {
    let text = serde_json::to_string(body).expect("serializable body");
    self.json_text(text)
  }

  /// JSON body sent byte-for-byte (when a signature covers the exact text).
  pub fn json_text(mut self, text: impl Into<String>) -> Self {
    self.body = Body::Bytes(
      text.into().into_bytes(),
      "application/json;charset=UTF-8".into(),
    );
    self
  }

  /// `application/x-www-form-urlencoded` body.
  pub fn form<K: AsRef<str>, V: AsRef<str>>(
    mut self,
    pairs: impl IntoIterator<Item = (K, V)>,
  ) -> Self {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
      ser.append_pair(k.as_ref(), v.as_ref());
    }
    self.body = Body::Bytes(
      ser.finish().into_bytes(),
      "application/x-www-form-urlencoded".into(),
    );
    self
  }

  pub fn bytes(mut self, data: Vec<u8>, content_type: &str) -> Self {
    self.body = Body::Bytes(data, content_type.into());
    self
  }

  pub fn multipart(mut self, parts: Vec<Part>) -> Self {
    self.body = Body::Multipart(parts);
    self
  }

  /// Do not send the platform cookies (CDN, object storage, third-party hosts).
  pub fn no_cookies(mut self) -> Self {
    self.cookies = false;
    self
  }

  /// Skip the per-platform pacing for this request.
  pub fn no_throttle(mut self) -> Self {
    self.throttle = false;
    self
  }

  /// How many times to retry on network errors, 429 and 5xx (GET defaults to 2, others to 0).
  pub fn retries(mut self, n: u32) -> Self {
    self.retries = n;
    self
  }

  fn full_url(&self) -> Result<String> {
    if self.query.is_empty() {
      return Ok(self.url.clone());
    }
    let mut url = url::Url::parse(&self.url)
      .map_err(|e| Error::internal(format!("bad url {}: {e}", self.url)))?;
    url.query_pairs_mut().extend_pairs(self.query.iter());
    Ok(url.into())
  }

  fn build(&self, url: &str) -> Result<wreq::RequestBuilder> {
    let mut headers = self.http.headers.clone();
    for (k, v) in &self.headers {
      headers.insert(k.clone(), v.clone());
    }
    if self.cookies {
      let cookie = self.http.cookie_header();
      if !cookie.is_empty() {
        headers.insert(
          "cookie",
          HeaderValue::from_str(&cookie).map_err(|e| Error::internal(e.to_string()))?,
        );
      }
    }
    let mut rb = self.http.client.request(self.method.clone(), url);
    match &self.body {
      Body::Empty => {}
      Body::Bytes(data, ct) => {
        if !headers.contains_key("content-type") {
          headers.insert(
            "content-type",
            HeaderValue::from_str(ct).map_err(|e| Error::internal(e.to_string()))?,
          );
        }
        rb = rb.body(data.clone());
      }
      Body::Multipart(parts) => {
        let mut form = wreq::multipart::Form::new();
        for p in parts {
          let mut part = wreq::multipart::Part::bytes(p.data.clone());
          if let Some(f) = &p.file_name {
            part = part.file_name(f.clone());
          }
          if let Some(m) = &p.mime {
            part = part
              .mime_str(m)
              .map_err(|e| Error::internal(e.to_string()))?;
          }
          form = form.part(p.name.clone(), part);
        }
        rb = rb.multipart(form);
      }
    }
    Ok(rb.headers(headers))
  }

  async fn execute(&self) -> Result<wreq::Response> {
    let url = self.full_url()?;
    let mut attempt = 0;
    loop {
      if self.throttle {
        self.http.throttle().await;
      }
      let started = Instant::now();
      let result = self.build(&url)?.send().await;
      self.http.last.set(Some(Instant::now()));
      match result {
        Ok(resp) => {
          tracing::debug!(
            "{} {} -> {} ({} ms)",
            self.method,
            redact(&url),
            resp.status(),
            started.elapsed().as_millis()
          );
          self.http.absorb_cookies(&resp);
          let status = resp.status();
          let retryable = status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
          if retryable && attempt < self.retries {
            attempt += 1;
            backoff(attempt).await;
            continue;
          }
          return Ok(resp);
        }
        Err(e) => {
          tracing::debug!("{} {} failed: {e}", self.method, redact(&url));
          if attempt < self.retries {
            attempt += 1;
            backoff(attempt).await;
            continue;
          }
          return Err(Error::network(format!(
            "{} {}: {e}",
            self.method,
            redact(&url)
          )));
        }
      }
    }
  }

  pub async fn send(self) -> Result<Resp> {
    let resp = self.execute().await?;
    let status = resp.status();
    let headers = resp.headers().clone();
    let url = resp.uri().to_string();
    let body = resp
      .bytes()
      .await
      .map_err(|e| Error::network(e.to_string()))?
      .to_vec();
    Ok(Resp {
      status,
      headers,
      body,
      url,
    })
  }

  /// Send, require a 2xx status and parse the body as JSON.
  pub async fn value(self) -> Result<Value> {
    self.send().await?.check()?.value()
  }

  /// Stream the response body into `path`; `progress(done, total)` is called per chunk.
  pub async fn save_to(
    self,
    path: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
  ) -> Result<u64> {
    let resp = self.execute().await?;
    let status = resp.status();
    if !status.is_success() {
      return Err(status_error(status, ""));
    }
    let total = resp.content_length();
    let mut file = tokio::fs::File::create(path).await?;
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
      let chunk = chunk.map_err(|e| Error::network(e.to_string()))?;
      file.write_all(&chunk).await?;
      done += chunk.len() as u64;
      progress(done, total);
    }
    file.flush().await?;
    Ok(done)
  }
}

async fn backoff(attempt: u32) {
  let base = 1000u64 << (attempt - 1).min(4);
  let jitter = rand::rng().random_range(0..1000);
  tokio::time::sleep(Duration::from_millis(base + jitter)).await;
}

/// Drop the query string from logged URLs; it may carry tokens.
fn redact(url: &str) -> &str {
  url.split('?').next().unwrap_or(url)
}

/// A fully read response.
pub struct Resp {
  pub status: StatusCode,
  pub headers: HeaderMap,
  pub body: Vec<u8>,
  /// Final URL after redirects.
  pub url: String,
}

impl Resp {
  pub fn header(&self, name: &str) -> Option<&str> {
    self.headers.get(name).and_then(|v| v.to_str().ok())
  }

  pub fn text(&self) -> String {
    String::from_utf8_lossy(&self.body).into_owned()
  }

  pub fn json<T: DeserializeOwned>(&self) -> Result<T> {
    serde_json::from_slice(&self.body).map_err(|e| {
      Error::upstream(format!(
        "unexpected response ({e}): {}",
        crate::text::truncate(&self.text(), 200)
      ))
    })
  }

  pub fn value(&self) -> Result<Value> {
    self.json()
  }

  /// Map common failure statuses to error codes; 2xx passes through.
  pub fn check(self) -> Result<Self> {
    if self.status.is_success() {
      Ok(self)
    } else {
      Err(status_error(self.status, &self.text()))
    }
  }
}

pub fn status_error(status: StatusCode, body: &str) -> Error {
  let snippet = crate::text::truncate(&crate::text::one_line(body), 200);
  let code = match status.as_u16() {
    401 => ErrorCode::NotAuthenticated,
    403 => ErrorCode::PermissionDenied,
    404 => ErrorCode::NotFound,
    429 => ErrorCode::RateLimited,
    _ => ErrorCode::UpstreamError,
  };
  Error::new(code, format!("HTTP {status}: {snippet}"))
}
