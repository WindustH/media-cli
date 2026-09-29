//! Random identifiers: trace ids and sharding (xhshow `utils/random_gen.py`,
//! `utils/sharding.py`), device cookies (`xhs_cli/qr_login.py`) and search ids.

use rand::Rng;
use rand::seq::IndexedRandom;

use super::config::{
  HEX_CHARS, TRACE_ID_LENGTH, XRAY_TRACE_ID_SEQ_MAX, XRAY_TRACE_ID_TIMESTAMP_SHIFT,
};

fn pick(charset: &[u8], len: usize) -> String {
  let mut rng = rand::rng();
  (0..len)
    .map(|_| *charset.choose(&mut rng).expect("charset") as char)
    .collect()
}

/// x-b3-traceid: 16 random hex characters.
pub fn b3_trace_id() -> String {
  pick(HEX_CHARS, TRACE_ID_LENGTH)
}

/// x-xray-traceid: `(ms << 23 | seq)` as 16 hex digits, then 16 random hex characters.
pub fn xray_trace_id(ts_ms: u64) -> String {
  let seq = rand::rng().random_range(0..=XRAY_TRACE_ID_SEQ_MAX);
  let head = (u128::from(ts_ms) << XRAY_TRACE_ID_TIMESTAMP_SHIFT) | u128::from(seq);
  format!("{head:016x}{}", pick(HEX_CHARS, TRACE_ID_LENGTH))
}

/// xy-direction without a user id: a random shard in `10..=100`.
pub fn sharding_key() -> u32 {
  rand::rng().random_range(10..=100)
}

/// A fresh `a1` cookie: 24 hex characters, the millisecond time, 15 hex characters.
pub fn a1(now_ms: u64) -> String {
  const HEX: &[u8] = b"0123456789abcdef";
  format!("{}{now_ms}{}", pick(HEX, 24), pick(HEX, 15))
}

/// A fresh `webId` cookie: 32 hex characters.
pub fn web_id() -> String {
  pick(b"0123456789abcdef", 32)
}

/// Search session id: `(ms << 64) + random` in base 36, as the web client's
/// `BigInt(...).toString(36)` (xhshow `generate_search_id`).
pub fn search_id(now_ms: u64) -> String {
  const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
  let mut n = (u128::from(now_ms) << 64) + rand::rng().random_range(1..=0x7FFF_FFFEu128);
  let mut out = Vec::new();
  while n > 0 {
    out.push(DIGITS[(n % 36) as usize]);
    n /= 36;
  }
  out.reverse();
  String::from_utf8(out).expect("ascii")
}

/// `request_id` of the search prewarm calls: `{random}-{ms}`.
pub fn search_request_id(now_ms: u64) -> String {
  let r = rand::rng().random_range(1_000_000_000u32..=2_147_483_647);
  format!("{r}-{now_ms}")
}
