//! OAuth "script" app credentials: a password-grant token for
//! `https://oauth.reddit.com`, cached until shortly before it expires.
//!
//! Active when `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `REDDIT_USERNAME`
//! and `REDDIT_PASSWORD` are all set (an app of type "script" from
//! <https://www.reddit.com/prefs/apps>). The token is never logged.

use std::cell::RefCell;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use media_core::{Ctx, Error, Result, ValueExt};
use serde::{Deserialize, Serialize};

const TOKEN_URL: &str = "https://www.reddit.com/api/v1/access_token";
/// Upper bound for the cache file's age; the stored expiry decides first.
const CACHE_TTL: Duration = Duration::from_secs(24 * 3600);
/// Renew this long before the upstream expiry.
const MARGIN_SECS: i64 = 120;
const ENV_HINT: &str = "check REDDIT_CLIENT_ID, REDDIT_CLIENT_SECRET, REDDIT_USERNAME and \
   REDDIT_PASSWORD (a \"script\" app from https://www.reddit.com/prefs/apps)";

pub struct Credentials {
  client_id: String,
  secret: String,
  pub username: String,
  password: String,
  token: RefCell<Option<String>>,
}

#[derive(Serialize, Deserialize)]
struct Cached {
  token: String,
  /// Unix seconds.
  expires: i64,
}

impl Credentials {
  pub fn from_env() -> Option<Self> {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    Some(Self {
      client_id: var("REDDIT_CLIENT_ID")?,
      secret: var("REDDIT_CLIENT_SECRET")?,
      username: var("REDDIT_USERNAME")?.trim().to_owned(),
      password: var("REDDIT_PASSWORD")?,
      token: RefCell::new(None),
    })
  }

  /// Reddit asks API clients for a unique, descriptive User-Agent.
  pub fn user_agent(&self) -> String {
    let name: String = self
      .username
      .chars()
      .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
      .collect();
    format!("cli:media-cli:{} (by /u/{name})", env!("CARGO_PKG_VERSION"))
  }

  fn cache_key(&self) -> String {
    let name: String = self
      .username
      .to_ascii_lowercase()
      .chars()
      .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
      .collect();
    format!("oauth-{name}")
  }

  /// A valid bearer token: from memory, the cache, or a new password grant.
  pub async fn token(&self, ctx: &Ctx) -> Result<String> {
    if let Some(t) = self.token.borrow().clone() {
      return Ok(t);
    }
    let now = jiff::Timestamp::now().as_second();
    let cached = ctx
      .store
      .cache_get::<Cached>(&self.cache_key(), CACHE_TTL)
      .filter(|c| c.expires - MARGIN_SECS > now && !c.token.is_empty());
    let token = match cached {
      Some(c) => c.token,
      None => self.grant(ctx, now).await?,
    };
    *self.token.borrow_mut() = Some(token.clone());
    Ok(token)
  }

  /// Forget the token (it was refused), so the next run asks for a new one.
  pub fn forget(&self, ctx: &Ctx) {
    *self.token.borrow_mut() = None;
    let empty = Cached {
      token: String::new(),
      expires: 0,
    };
    ctx.store.cache_put(&self.cache_key(), &empty);
  }

  async fn grant(&self, ctx: &Ctx, now: i64) -> Result<String> {
    let basic = STANDARD.encode(format!("{}:{}", self.client_id, self.secret));
    let resp = ctx
      .http
      .post(TOKEN_URL)
      .no_cookies()
      .header("authorization", format!("Basic {basic}"))
      .header("user-agent", self.user_agent())
      .form([
        ("grant_type", "password"),
        ("username", self.username.as_str()),
        ("password", self.password.as_str()),
      ])
      .send()
      .await?;
    // Bad app credentials answer 401; a bad password answers 200 with `error`.
    let v = resp.value().unwrap_or_default();
    let token = v.str("access_token").filter(|_| resp.status.is_success());
    let Some(token) = token else {
      let reason = v
        .str("error")
        .unwrap_or_else(|| format!("HTTP {}", resp.status));
      return Err(
        Error::auth(format!("Reddit refused the OAuth credentials ({reason})")).with_hint(ENV_HINT),
      );
    };
    let expires = now + v.i64("expires_in").unwrap_or(3600);
    ctx.store.cache_put(
      &self.cache_key(),
      &Cached {
        token: token.clone(),
        expires,
      },
    );
    Ok(token)
  }
}
