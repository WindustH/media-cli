//! The `Authorization` header of a cookie session: `SAPISIDHASH`, a SHA-1 of
//! `"<unix seconds> <SAPISID> <origin>"`, recomputed for every request.
//!
//! As the web app (and yt-dlp's `_get_sid_authorization_header`) does, the
//! first-party and third-party variants follow when their cookies are there:
//! `SAPISID1PHASH` from `__Secure-1PAPISID`, `SAPISID3PHASH` from
//! `__Secure-3PAPISID`. Some exports lack `SAPISID` itself; the
//! `__Secure-3PAPISID` value (the same secret) stands in for it then.

use media_core::Http;
use sha1::{Digest, Sha1};

/// Cookies that can sign a request; either marks a logged-in session.
pub const SIGNING_COOKIES: &[&str] = &["SAPISID", "__Secure-3PAPISID"];

/// `SAPISIDHASH ...` for `origin` (`https://www.youtube.com`), or `None`
/// without a signing cookie.
pub fn authorization(http: &Http, origin: &str) -> Option<String> {
  let now = jiff::Timestamp::now().as_second();
  let sapisid = SIGNING_COOKIES.iter().find_map(|c| http.cookie(c))?;
  let mut parts = vec![hash("SAPISIDHASH", now, &sapisid, origin)];
  for (scheme, cookie) in [
    ("SAPISID1PHASH", "__Secure-1PAPISID"),
    ("SAPISID3PHASH", "__Secure-3PAPISID"),
  ] {
    if let Some(v) = http.cookie(cookie) {
      parts.push(hash(scheme, now, &v, origin));
    }
  }
  Some(parts.join(" "))
}

fn hash(scheme: &str, now: i64, secret: &str, origin: &str) -> String {
  let digest = Sha1::digest(format!("{now} {secret} {origin}").as_bytes());
  let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
  format!("{scheme} {now}_{hex}")
}
