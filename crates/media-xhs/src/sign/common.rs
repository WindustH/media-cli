//! The x-s-common header (xhshow `core/common_sign.py` and `core/crc32_encrypt.py`).

use serde_json::json;

use super::config::{APP_ID, PLATFORM, SDK_VERSION, WEB_BUILD};
use super::{encoder, fingerprint};

pub fn xs_common(a1: &str, now_ms: u64) -> String {
  let b1 = fingerprint::b1(now_ms);
  let x9 = crc32_js(b1.as_bytes());
  let sign = json!({
    "s0": 5,
    "s1": "",
    "x0": "1",
    "x1": SDK_VERSION,
    "x2": PLATFORM,
    "x3": APP_ID,
    "x4": WEB_BUILD,
    "x5": a1,
    "x6": "",
    "x7": "",
    "x8": b1,
    "x9": x9,
    "x10": 0,
    "x11": "normal",
  });
  encoder::encode(sign.to_string())
}

const POLY: u32 = 0xEDB8_8320;

const TABLE: [u32; 256] = {
  let mut table = [0u32; 256];
  let mut d = 0;
  while d < 256 {
    let mut r = d as u32;
    let mut k = 0;
    while k < 8 {
      r = if r & 1 == 1 { (r >> 1) ^ POLY } else { r >> 1 };
      k += 1;
    }
    table[d] = r;
    d += 1;
  }
  table
};

/// Standard CRC-32 (`binascii.crc32`).
pub fn crc32(data: &[u8]) -> u32 {
  let mut c = u32::MAX;
  for &b in data {
    c = TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
  }
  !c
}

/// The JS variant `(-1 ^ c ^ 0xEDB88320) >>> 0` read as a signed 32-bit int,
/// where `c` is the CRC-32 state before the final inversion.
pub fn crc32_js(data: &[u8]) -> i32 {
  (crc32(data) ^ POLY) as i32
}
