//! The `b1` browser fingerprint inside x-s-common (xhshow `generators/fingerprint.py`).
//!
//! xhshow builds a full ~80-field fingerprint (user agent, GPU, screen ...) but
//! `generate_b1` only encodes the 18 fields below; the rest never leaves the
//! process, so only these are generated here.

use rand::Rng;
use serde_json::json;

use super::config::B1_SECRET_KEY;
use super::encoder;

/// `b1` for a fingerprint generated at `now_ms`.
pub fn b1(now_ms: u64) -> String {
  let fp = json!({
    "x33": "0",
    "x34": "0",
    "x35": "0",
    "x36": rand::rng().random_range(1..=20).to_string(),
    "x37": "0|0|0|0|0|0|0|0|0|1|0|0|0|0|0|0|0|0|1|0|0|0|0|0",
    "x38": "0|0|1|0|1|0|0|0|0|0|1|0|1|0|1|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0|0",
    "x39": 0,
    "x42": "3.4.4",
    "x43": "742cc32c",
    "x44": now_ms.to_string(),
    "x45": "__SEC_CAV__1-1-1-1-1|__SEC_WSA__|",
    "x46": "false",
    "x48": "",
    "x49": "{list:[],type:}",
    "x50": "",
    "x51": "",
    "x52": "",
    "x82": "_0x17a2|_0x1954",
  });
  let cipher = rc4(B1_SECRET_KEY, fp.to_string().as_bytes());
  encoder::encode(quote_bytes(&cipher))
}

/// What xhshow derives from the RC4 output: the bytes are read as Latin-1,
/// `urllib.parse.quote`d (UTF-8, safe `!*'()~_-`), and the `%XX` chunks are
/// turned back into bytes. That equals the UTF-8 form of the Latin-1 text,
/// minus the unescaped characters before the first `%` (its `split("%")[1:]`
/// drops them).
fn quote_bytes(cipher: &[u8]) -> Vec<u8> {
  let mut utf8 = Vec::with_capacity(cipher.len() * 2);
  for &b in cipher {
    if b < 0x80 {
      utf8.push(b);
    } else {
      utf8.extend_from_slice(&[0xC0 | (b >> 6), 0x80 | (b & 0x3F)]);
    }
  }
  let safe = |b: &u8| b.is_ascii_alphanumeric() || b"_.-~!*'()".contains(b);
  let start = utf8.iter().position(|b| !safe(b)).unwrap_or(utf8.len());
  utf8.split_off(start)
}

pub fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
  let mut s: [u8; 256] = std::array::from_fn(|i| i as u8);
  let mut j = 0u8;
  for i in 0..256 {
    j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
    s.swap(i, j as usize);
  }
  let (mut i, mut j) = (0u8, 0u8);
  data
    .iter()
    .map(|b| {
      i = i.wrapping_add(1);
      j = j.wrapping_add(s[i as usize]);
      s.swap(i as usize, j as usize);
      b ^ s[s[i as usize].wrapping_add(s[j as usize]) as usize]
    })
    .collect()
}
