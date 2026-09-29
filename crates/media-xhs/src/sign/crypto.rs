//! The `x3` payload of x-s (xhshow `core/crypto.py` and `utils/bit_ops.py`):
//! a 148-byte little-endian record of timestamps, session counters, the
//! request digest, a1 and a custom hash, XOR-masked and Base64-encoded.

use super::config::{
  A1_LENGTH, A3_PREFIX, APP_ID_LENGTH, ENV_CHECKS_DEFAULT, ENV_TABLE, HASH_IV, HEX_KEY,
  MD5_XOR_LENGTH, PAYLOAD_LENGTH, VERSION_BYTES,
};
use super::encoder;
use super::session::SignState;

/// Encoded `x3` for one request.
///
/// `d` is the MD5 of the signed content (URI plus query or body), `m` the MD5
/// of the path for POST requests (equal to `d` for GET).
pub fn x3(
  d: &[u8; 16],
  m: &[u8; 16],
  a1: &str,
  app_id: &str,
  ts_ms: u64,
  state: &SignState,
  seed: u32,
) -> String {
  let payload = build_payload(d, m, a1, app_id, ts_ms, state, seed);
  let mut masked = xor_transform(&payload);
  masked.truncate(PAYLOAD_LENGTH);
  encoder::encode_x3(masked)
}

/// `build_payload_array` with a session state (the variant `signing.py` uses).
pub fn build_payload(
  d: &[u8; 16],
  m: &[u8; 16],
  a1: &str,
  app_id: &str,
  ts_ms: u64,
  state: &SignState,
  seed: u32,
) -> Vec<u8> {
  let seed_byte = seed as u8;
  let ts_bytes = ts_ms.to_le_bytes();
  let mut p = Vec::with_capacity(PAYLOAD_LENGTH + 4);
  p.extend_from_slice(&VERSION_BYTES);
  p.extend_from_slice(&seed.to_le_bytes());
  p.extend_from_slice(&ts_bytes);
  p.extend_from_slice(&state.page_load_timestamp.to_le_bytes());
  p.extend_from_slice(&state.sequence_value.to_le_bytes());
  p.extend_from_slice(&state.window_props_length.to_le_bytes());
  p.extend_from_slice(&state.uri_length.to_le_bytes());
  p.extend(d[..MD5_XOR_LENGTH].iter().map(|b| b ^ seed_byte));
  push_fixed(&mut p, a1.as_bytes(), A1_LENGTH);
  push_fixed(&mut p, app_id.as_bytes(), APP_ID_LENGTH);
  p.push(1);
  p.push(seed_byte ^ ENV_TABLE[0]);
  p.extend((1..15).map(|i| ENV_TABLE[i] ^ ENV_CHECKS_DEFAULT[i]));
  let mut hash_input = ts_bytes.to_vec();
  hash_input.extend_from_slice(m);
  p.extend_from_slice(&A3_PREFIX);
  p.extend(custom_hash_v2(&hash_input).iter().map(|b| b ^ seed_byte));
  p
}

/// Length byte, then the bytes cut or zero-padded to `len`.
fn push_fixed(p: &mut Vec<u8>, bytes: &[u8], len: usize) {
  let mut field = bytes[..bytes.len().min(len)].to_vec();
  field.resize(len, 0);
  p.push(len as u8);
  p.extend_from_slice(&field);
}

/// XOR the first 144 bytes with the fixed key; later bytes pass through.
pub fn xor_transform(src: &[u8]) -> Vec<u8> {
  let key = hex_key();
  src
    .iter()
    .enumerate()
    .map(|(i, b)| key.get(i).map_or(*b, |k| b ^ k))
    .collect()
}

fn hex_key() -> Vec<u8> {
  (0..HEX_KEY.len())
    .step_by(2)
    .map(|i| u8::from_str_radix(&HEX_KEY[i..i + 2], 16).expect("hex key"))
    .collect()
}

/// 16-byte hash of the `a3` field; the input length must be a multiple of 8.
pub fn custom_hash_v2(input: &[u8]) -> [u8; 16] {
  let [mut s0, mut s1, mut s2, mut s3] = HASH_IV;
  let len = input.len() as u32;
  s0 ^= len;
  s1 ^= len << 8;
  s2 ^= len << 16;
  s3 ^= len << 24;
  for chunk in input.as_chunks::<8>().0 {
    let v0 = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    let v1 = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
    s0 = (s0.wrapping_add(v0) ^ s2).rotate_left(7);
    s1 = ((v0 ^ s1).wrapping_add(s3)).rotate_left(11);
    s2 = (s2.wrapping_add(v1) ^ s0).rotate_left(13);
    s3 = ((s3 ^ v1).wrapping_add(s1)).rotate_left(17);
  }
  let t0 = s0 ^ len;
  let t1 = s1 ^ t0;
  let t2 = s2.wrapping_add(t1);
  let t3 = s3 ^ t2;
  let (r0, r1, r2, r3) = (
    t0.rotate_left(9),
    t1.rotate_left(13),
    t2.rotate_left(17),
    t3.rotate_left(19),
  );
  let s0 = r0.wrapping_add(r2);
  let s1 = r1 ^ r3;
  let s2 = r2.wrapping_add(s0);
  let s3 = r3 ^ s1;
  let mut out = [0u8; 16];
  for (i, s) in [s0, s1, s2, s3].into_iter().enumerate() {
    out[i * 4..i * 4 + 4].copy_from_slice(&s.to_le_bytes());
  }
  out
}
