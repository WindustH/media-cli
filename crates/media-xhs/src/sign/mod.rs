//! Request signatures, ported from the `xhshow` library as `xhs_cli/signing.py`
//! configures it (XYS format, macOS, session simulation), plus the creator
//! XYW signature.
//!
//! Main API headers: `x-s`, `x-s-common`, `x-t`, `x-b3-traceid`,
//! `x-xray-traceid`, `x-mns`, `xy-direction` (xhshow `client.py::sign_headers`),
//! and `x-rap-param` for the endpoints that require it.

mod common;
pub mod config;
mod creator;
mod crypto;
mod encoder;
mod fingerprint;
mod hash;
pub mod random;
mod session;
mod xrap;

use md5::{Digest, Md5};
use serde_json::json;

pub use creator::xyw;
pub use session::SignSession;
pub use xrap::x_rap_param;

use config::{APP_ID, PLATFORM, SDK_VERSION, X3_PREFIX, XYS_PREFIX};

/// The content xhshow signs for a GET: `path?k=v&...`, each value
/// percent-encoded except `,` (`urllib.parse.quote(v, safe=",")`). The same
/// string is sent as the request target, so the signature covers it exactly.
pub fn get_content(path: &str, params: &[(&str, &str)]) -> String {
  if params.is_empty() {
    return path.to_owned();
  }
  let query: Vec<String> = params
    .iter()
    .map(|(k, v)| format!("{k}={}", quote(v, b",")))
    .collect();
  format!("{path}?{}", query.join("&"))
}

/// Python's `urllib.parse.quote(s, safe)`: UTF-8, uppercase `%XX`.
pub fn quote(s: &str, safe: &[u8]) -> String {
  let mut out = String::with_capacity(s.len());
  for &b in s.as_bytes() {
    if b.is_ascii_alphanumeric() || b"_.-~".contains(&b) || safe.contains(&b) {
      out.push(b as char);
    } else {
      out.push_str(&format!("%{b:02X}"));
    }
  }
  out
}

/// Signing headers for one main-API request.
///
/// `content` is [`get_content`] for GET, or `path` followed by the exact JSON
/// body for POST.
pub fn main_headers(
  session: &mut SignSession,
  post: bool,
  path: &str,
  content: &str,
  a1: &str,
  now_ms: u64,
) -> Vec<(&'static str, String)> {
  let d: [u8; 16] = Md5::digest(content.as_bytes()).into();
  let m: [u8; 16] = if post {
    Md5::digest(path.as_bytes()).into()
  } else {
    d
  };
  let state = session.next(content);
  let x3 = crypto::x3(&d, &m, a1, APP_ID, now_ms, &state, rand::random());
  let data = json!({
    "x0": SDK_VERSION,
    "x1": APP_ID,
    "x2": PLATFORM,
    "x3": format!("{X3_PREFIX}{x3}"),
    "x4": "",
  });
  vec![
    (
      "x-s",
      format!("{XYS_PREFIX}{}", encoder::encode(data.to_string())),
    ),
    ("x-s-common", common::xs_common(a1, now_ms)),
    ("x-t", now_ms.to_string()),
    ("x-b3-traceid", random::b3_trace_id()),
    ("x-xray-traceid", random::xray_trace_id(now_ms)),
    ("x-mns", "unload".to_owned()),
    ("xy-direction", random::sharding_key().to_string()),
  ]
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis() as u64)
    .unwrap_or_default()
}
