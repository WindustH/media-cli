//! `x-client-transaction-id`, ported from the `xclienttransaction` package.
//!
//! The web client derives a key from two parts of the web app page: the
//! `twitter-site-verification` meta tag (key bytes) and one of the four
//! `loading-x-anim` SVG frames, animated to a time picked by key bytes whose
//! indices are read from the `ondemand.s` script. Each request id is then
//! `base64(r ‖ (key ‖ time ‖ sha256(method!path!time…)[..16] ‖ 3) ^ r)`.

mod anim;

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use media_core::{Error, ErrorCode, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Epoch of the id's 32-bit clock (2023-05-01).
const EPOCH_SECS: i64 = 1_682_924_400;
const KEYWORD: &str = "obfiowerehiring";
const TRAILER: u8 = 3;

/// Derived per-page material; small and cacheable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Material {
  /// The `twitter-site-verification` value (base64).
  key: String,
  animation_key: String,
}

fn sign_error(what: &str) -> Error {
  Error::new(
    ErrorCode::SignatureError,
    format!("x-client-transaction-id: {what}"),
  )
}

impl Material {
  /// Derive from the web app page and its `ondemand.s` script.
  pub fn derive(page: &str, ondemand: &str) -> Result<Self> {
    let (row_index, time_indices) = indices(ondemand)?;
    let key = site_key(page).ok_or_else(|| sign_error("no site verification key"))?;
    let bytes = STANDARD
      .decode(&key)
      .map_err(|_| sign_error("bad site verification key"))?;
    let at = |i: usize| {
      bytes
        .get(i)
        .map(|b| u32::from(*b))
        .ok_or_else(|| sign_error("key index out of range"))
    };
    let frame =
      frame_path(page, at(5)? as usize % 4).ok_or_else(|| sign_error("no animation frames"))?;
    let rows = frame_rows(&frame);
    let row = rows
      .get(at(row_index)? as usize % 16)
      .ok_or_else(|| sign_error("animation row out of range"))?;
    let mut product = 1u32;
    for i in time_indices {
      product *= at(i)? % 16;
    }
    let frame_time = js_round(f64::from(product) / 10.0) * 10.0;
    let animation_key =
      anim::animate(row, frame_time / 4096.0).ok_or_else(|| sign_error("short animation row"))?;
    Ok(Self { key, animation_key })
  }

  /// A fresh id for one request (`path` without the query string).
  pub fn transaction_id(&self, method: &str, path: &str) -> String {
    let now_ms = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_millis() as i64)
      .unwrap_or_default();
    let time = (now_ms - EPOCH_SECS * 1000).div_euclid(1000);
    self.id_at(method, path, time, rand::random())
  }

  fn id_at(&self, method: &str, path: &str, time: i64, random: u8) -> String {
    let key = STANDARD.decode(&self.key).unwrap_or_default();
    let hash = Sha256::digest(format!(
      "{method}!{path}!{time}{KEYWORD}{}",
      self.animation_key
    ));
    let time_bytes = (0..4).map(|i| ((time >> (i * 8)) & 0xff) as u8);
    let mut out = vec![random];
    out.extend(
      key
        .into_iter()
        .chain(time_bytes)
        .chain(hash[..16].iter().copied())
        .chain([TRAILER])
        .map(|b| b ^ random),
    );
    STANDARD_NO_PAD.encode(out)
  }
}

/// URL of the `ondemand.s` chunk named in the page's webpack chunk map.
pub fn ondemand_url(page: &str) -> Option<String> {
  let index = Regex::new(r#",(\d+):["']ondemand\.s["']"#)
    .ok()?
    .captures(page)?
    .get(1)?
    .as_str()
    .to_owned();
  let hash = Regex::new(&format!(r#",{index}:"([0-9a-f]+)""#))
    .ok()?
    .captures(page)?
    .get(1)?
    .as_str()
    .to_owned();
  Some(format!(
    "https://abs.twimg.com/responsive-web/client-web/ondemand.s.{hash}a.js"
  ))
}

/// Byte indices from `(x[N], 16)` calls: the row index first, then the time factors.
fn indices(ondemand: &str) -> Result<(usize, Vec<usize>)> {
  let re = Regex::new(r"(\(\w\[(\d{1,2})\],\s*16\))+").expect("valid regex");
  let found: Vec<usize> = re
    .captures_iter(ondemand)
    .filter_map(|c| c.get(2)?.as_str().parse().ok())
    .collect();
  match found.split_first() {
    Some((row, rest)) => Ok((*row, rest.to_vec())),
    None => Err(sign_error("no key byte indices in ondemand.s")),
  }
}

/// `content` of `<meta name="twitter-site-verification">`.
fn site_key(page: &str) -> Option<String> {
  let tag = Regex::new(r#"<meta[^>]*name=["']twitter-site-verification["'][^>]*>"#)
    .ok()?
    .find(page)?
    .as_str();
  attr(tag, "content")
}

fn attr(tag: &str, name: &str) -> Option<String> {
  let re = Regex::new(&format!(r#"\s{name}=["']([^"']*)["']"#)).ok()?;
  Some(re.captures(tag)?.get(1)?.as_str().to_owned())
}

/// `d` of the second `<path>` of the `n`-th `loading-x-anim` element (document order).
fn frame_path(page: &str, n: usize) -> Option<String> {
  let start = Regex::new(r#"\sid=["']loading-x-anim[^"']*["']"#)
    .ok()?
    .find_iter(page)
    .nth(n)?
    .end();
  let rest = &page[start..];
  let rest = &rest[..rest.find("</svg>").unwrap_or(rest.len())];
  let path = Regex::new(r"<path[^>]*>").ok()?.find_iter(rest).nth(1)?;
  attr(path.as_str(), "d")
}

/// Rows of numbers of a frame path: `d[9..]` split at `C`, digits runs as integers.
fn frame_rows(d: &str) -> Vec<Vec<u32>> {
  let digits = Regex::new(r"\d+").expect("valid regex");
  d.get(9..)
    .unwrap_or_default()
    .split('C')
    .map(|part| {
      digits
        .find_iter(part)
        .filter_map(|m| m.as_str().parse().ok())
        .collect()
    })
    .collect()
}

/// JavaScript `Math.round` as the reference emulates it.
fn js_round(x: f64) -> f64 {
  let floor = x.floor();
  let r = if x - floor >= 0.5 { x.ceil() } else { floor };
  r.copysign(x)
}
