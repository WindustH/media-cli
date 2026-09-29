//! Per-platform files: the saved session, a TTL cache and the short-index list.
//!
//! Layout (override the root with `MEDIA_CLI_HOME`):
//! - `~/.config/media-cli/<platform>/session.json` (mode 0600)
//! - `~/.cache/media-cli/<platform>/*.json`

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use jiff::Timestamp;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::http::Cookies;

/// What `login` saves: cookies plus platform extras (tokens, device ids ...).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Session {
  #[serde(default)]
  pub cookies: Cookies,
  #[serde(default)]
  pub extra: BTreeMap<String, String>,
  #[serde(default)]
  pub source: Option<String>,
  #[serde(default)]
  pub saved_at: Option<Timestamp>,
}

/// Which short-index list an argument refers to.
#[derive(Debug, Clone, Copy)]
pub enum RefKind {
  Post,
  User,
}

impl RefKind {
  fn file(self) -> &'static str {
    match self {
      RefKind::Post => "last-posts.json",
      RefKind::User => "last-users.json",
    }
  }
}

#[derive(Debug, Clone)]
pub struct Store {
  config_dir: PathBuf,
  cache_dir: PathBuf,
}

impl Store {
  pub fn new(platform: &str) -> Self {
    let (config, cache) = match std::env::var_os("MEDIA_CLI_HOME") {
      Some(home) => {
        let home = PathBuf::from(home);
        (home.join("config"), home.join("cache"))
      }
      None => (
        dirs::config_dir()
          .unwrap_or_else(|| PathBuf::from("."))
          .join("media-cli"),
        dirs::cache_dir()
          .unwrap_or_else(std::env::temp_dir)
          .join("media-cli"),
      ),
    };
    Self {
      config_dir: config.join(platform),
      cache_dir: cache.join(platform),
    }
  }

  pub fn config_dir(&self) -> &Path {
    &self.config_dir
  }

  pub fn cache_dir(&self) -> &Path {
    &self.cache_dir
  }

  fn session_path(&self) -> PathBuf {
    self.config_dir.join("session.json")
  }

  pub fn load_session(&self) -> Result<Option<Session>> {
    match fs::read(self.session_path()) {
      Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| {
        Error::internal(format!(
          "corrupt session file {}: {e}",
          self.session_path().display()
        ))
      }),
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
      Err(e) => Err(e.into()),
    }
  }

  pub fn save_session(&self, session: &Session) -> Result<()> {
    fs::create_dir_all(&self.config_dir)?;
    write_private(&self.session_path(), &serde_json::to_vec_pretty(session)?)
  }

  /// Remove the saved session; returns whether one existed.
  pub fn clear_session(&self) -> Result<bool> {
    match fs::remove_file(self.session_path()) {
      Ok(()) => Ok(true),
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
      Err(e) => Err(e.into()),
    }
  }

  /// A cached value younger than `ttl`.
  pub fn cache_get<T: DeserializeOwned>(&self, key: &str, ttl: Duration) -> Option<T> {
    let path = self.cache_dir.join(format!("{key}.json"));
    let age = fs::metadata(&path).ok()?.modified().ok()?;
    if SystemTime::now()
      .duration_since(age)
      .unwrap_or(Duration::MAX)
      > ttl
    {
      return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
  }

  /// Best effort: a failed cache write never fails the command. Files are
  /// private to the user (caches may hold tokens).
  pub fn cache_put<T: Serialize>(&self, key: &str, value: &T) {
    let write = || -> Result<()> {
      fs::create_dir_all(&self.cache_dir)?;
      write_private(
        &self.cache_dir.join(format!("{key}.json")),
        &serde_json::to_vec(value)?,
      )?;
      Ok(())
    };
    if let Err(e) = write() {
      tracing::debug!("cache write {key} failed: {e}");
    }
  }

  /// Remember the references of the last printed list, for `#N` arguments.
  pub fn remember(&self, kind: RefKind, refs: Vec<String>) {
    if !refs.is_empty() {
      self.cache_put(kind.file().trim_end_matches(".json"), &refs);
    }
  }

  /// Resolve `#N` / `N` (1-3 digits) against the last list; anything else passes through.
  pub fn resolve(&self, kind: RefKind, arg: &str) -> Result<String> {
    let arg = arg.trim();
    let (digits, explicit) = match arg.strip_prefix('#') {
      Some(rest) => (rest, true),
      None => (arg, false),
    };
    let is_index =
      !digits.is_empty() && digits.len() <= 3 && digits.bytes().all(|b| b.is_ascii_digit());
    if !is_index {
      return Ok(arg.to_owned());
    }
    let refs: Option<Vec<String>> = self.cache_get(
      kind.file().trim_end_matches(".json"),
      Duration::from_secs(7 * 86400),
    );
    let index: usize = digits.parse().unwrap_or(0);
    match refs.and_then(|r| r.get(index.wrapping_sub(1)).cloned()) {
      Some(r) => Ok(r),
      None if explicit => Err(Error::input(format!(
        "no item #{index} in the last list; run a listing command first"
      ))),
      None => Ok(arg.to_owned()),
    }
  }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
  let tmp = path.with_extension("tmp");
  #[cfg(unix)]
  {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = fs::OpenOptions::new()
      .write(true)
      .create(true)
      .truncate(true)
      .mode(0o600)
      .open(&tmp)?;
    f.write_all(bytes)?;
  }
  #[cfg(not(unix))]
  fs::write(&tmp, bytes)?;
  fs::rename(tmp, path)?;
  Ok(())
}
