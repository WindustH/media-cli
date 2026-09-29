//! WBI request signing (`w_rid` / `wts`) and the `dm_*` anti-bot parameters.
//!
//! The web client mixes the file names of two images announced by
//! `/x/web-interface/nav` into a 32-character key, appends it to the sorted,
//! URI-encoded query and sends the MD5 as `w_rid`.

use md5::{Digest, Md5};
use rand::seq::IndexedRandom;

/// Permutation applied to `img_key + sub_key`.
const MIXIN: [usize; 64] = [
  46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
  28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
  54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

/// `https://i0.hdslb.com/bfs/wbi/7cd0...077c.png` -> `7cd0...077c`.
pub fn key_from_url(url: &str) -> &str {
  let name = url.rsplit('/').next().unwrap_or(url);
  name.split('.').next().unwrap_or(name)
}

/// The 32-character mixin key for an `img_key` / `sub_key` pair.
pub fn mixin_key(img_key: &str, sub_key: &str) -> String {
  let raw: Vec<char> = format!("{img_key}{sub_key}").chars().collect();
  MIXIN.iter().filter_map(|&i| raw.get(i)).take(32).collect()
}

/// JavaScript `encodeURIComponent`.
fn encode(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  for b in s.bytes() {
    if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
      out.push(b as char);
    } else {
      out.push_str(&format!("%{b:02X}"));
    }
  }
  out
}

/// `k=v&k2=v2` in the given order, encoded like the web client.
pub fn query_string(params: &[(String, String)]) -> String {
  params
    .iter()
    .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
    .collect::<Vec<_>>()
    .join("&")
}

/// Sign `params` at time `wts`; returns the full query string ending in `&w_rid=...`.
///
/// Adds `wts` (and a default `web_location`), drops `!'()*` from values and
/// sorts by key, exactly as the web client does before hashing.
pub fn sign(mut params: Vec<(String, String)>, mixin_key: &str, wts: i64) -> String {
  params.retain(|(k, _)| k != "w_rid" && k != "wts");
  params.push(("wts".into(), wts.to_string()));
  if !params.iter().any(|(k, _)| k == "web_location") {
    params.push(("web_location".into(), "1550101".into()));
  }
  for (_, v) in &mut params {
    v.retain(|c| !"!'()*".contains(c));
  }
  params.sort_by(|a, b| a.0.cmp(&b.0));
  let query = query_string(&params);
  let w_rid = hex(&Md5::digest(format!("{query}{mixin_key}")));
  format!("{query}&w_rid={w_rid}")
}

/// Mouse / keyboard telemetry parameters some endpoints check (`dm_img_*`).
pub fn dm_params() -> Vec<(String, String)> {
  let letters: Vec<char> = "ABCDEFGHIJK".chars().collect();
  let pick = || -> String {
    letters
      .choose_multiple(&mut rand::rng(), 2)
      .collect::<String>()
  };
  vec![
    ("dm_img_list".into(), "[]".into()),
    ("dm_img_str".into(), pick()),
    ("dm_cover_img_str".into(), pick()),
    (
      "dm_img_inter".into(),
      r#"{"ds":[],"wh":[0,0,0],"of":[0,0,0]}"#.into(),
    ),
  ]
}

fn hex(bytes: &[u8]) -> String {
  bytes.iter().map(|b| format!("{b:02x}")).collect()
}
