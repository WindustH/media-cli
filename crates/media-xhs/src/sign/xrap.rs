//! The `x-rap-param` risk-control header (xhshow `core/xrap.py`): a TLV
//! record of the request hash and a browser environment snapshot, gzipped,
//! encrypted with an SM4-like block cipher (custom S-box, fixed round keys)
//! and wrapped in a checksummed envelope.

use std::io::Write;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use flate2::{Compression, GzBuilder};
use rand::Rng;
use rand::seq::IndexedRandom;

use super::hash::xxh32;

const SDK_VERSION: u32 = 10300;

const ROUND_KEYS: [[u32; 4]; 10] = [
  [0x6B714931, 0x44546377, 0x4B583930, 0x5A744179],
  [0x89314C98, 0xCD652FEF, 0x863D16DF, 0xDC4957A6],
  [0xC205330C, 0x0F601CE3, 0x895D0A3C, 0x55145D9A],
  [0xD205006E, 0xDD651C8D, 0x543816B1, 0x012C4B2B],
  [0x770C2B6F, 0xAA6937E2, 0xFE512153, 0xFF7D6A78],
  [0x7866FBF4, 0xD20FCC16, 0x2C5EED45, 0xD323873D],
  [0x90E9C67E, 0x42E60A68, 0x6EB8E72D, 0xBD9B6010],
  [0xE9C52BEE, 0xAB232186, 0xC59BC6AB, 0x7800A6BB],
  [0x13BA9A3E, 0xB899BBB8, 0x7D027D13, 0x0502DBA8],
  [0x50613270, 0xE8F889C8, 0x95FAF4DB, 0x90F82F73],
];
const LAST_ROUND_KEY: [u32; 4] = [0xF396B44F, 0x1B6E3D87, 0x8E94C95C, 0x1E6CE62F];

#[rustfmt::skip]
const SBOX: [u8; 256] = [
  0x7A, 0x01, 0x58, 0xE0, 0x50, 0x4E, 0x02, 0x79, 0x1D, 0x4B, 0x53, 0xDA, 0x6B, 0x48, 0xD4, 0x52,
  0xED, 0x77, 0x12, 0x21, 0x14, 0x15, 0xEC, 0x10, 0x18, 0xE5, 0xB9, 0xF1, 0x0C, 0x08, 0xFC, 0x7D,
  0xF9, 0xCD, 0xB5, 0xC8, 0xE6, 0x37, 0x26, 0x87, 0x56, 0xBA, 0xB8, 0x2B, 0xAD, 0xF0, 0x68, 0xF7,
  0x8B, 0x8D, 0xD3, 0x5E, 0x36, 0x4D, 0x2E, 0x92, 0x31, 0x82, 0xF2, 0x29, 0x70, 0x3D, 0x2D, 0xD7,
  0xB6, 0x40, 0xB2, 0x43, 0x44, 0x80, 0x78, 0xD2, 0x0D, 0x49, 0x4A, 0x09, 0x63, 0x6C, 0x07, 0x3A,
  0x9E, 0xD5, 0x06, 0xC6, 0xE1, 0x62, 0xF4, 0x34, 0x24, 0x59, 0xA9, 0x57, 0x2A, 0x00, 0x3E, 0x17,
  0x2C, 0x0A, 0x1A, 0x42, 0xFA, 0x93, 0xBE, 0xDC, 0xF5, 0xB3, 0x6A, 0x13, 0xE8, 0x03, 0xC7, 0x97,
  0xBB, 0x73, 0x76, 0x86, 0xE3, 0x46, 0x72, 0x47, 0xD0, 0x05, 0x4C, 0x38, 0x7C, 0x1F, 0x81, 0xAB,
  0x75, 0x51, 0xEB, 0xF3, 0x32, 0x74, 0x11, 0x8F, 0x84, 0x89, 0x9C, 0x71, 0x22, 0x7E, 0x9D, 0xCF,
  0x3F, 0x91, 0x69, 0x65, 0x3C, 0x6D, 0x96, 0xA2, 0x98, 0x99, 0x33, 0x39, 0x9A, 0xCA, 0xC3, 0x9F,
  0xA0, 0xBC, 0xE4, 0xA3, 0xA4, 0x54, 0x7F, 0xA7, 0xA8, 0x04, 0x6F, 0x5D, 0xAC, 0xB7, 0x27, 0xAF,
  0xB0, 0x28, 0x41, 0xAE, 0xB4, 0x6E, 0x0B, 0x1B, 0xDF, 0x8E, 0x30, 0xB1, 0xFE, 0x90, 0x61, 0x60,
  0xC0, 0xCB, 0x5C, 0x0E, 0xEF, 0x16, 0x83, 0xEA, 0x20, 0xE9, 0xC9, 0x55, 0xC4, 0x45, 0x85, 0xCC,
  0x1E, 0xAA, 0x67, 0x8A, 0x7B, 0x35, 0xD6, 0x19, 0xD8, 0xD9, 0xC2, 0xDB, 0x94, 0xDD, 0x1C, 0xDE,
  0xA6, 0xFF, 0xF8, 0xBF, 0x5B, 0x5A, 0x0F, 0xE7, 0xC1, 0xBD, 0xD1, 0x66, 0xC5, 0x25, 0xEE, 0x8C,
  0xE2, 0x5F, 0x88, 0xA1, 0x3B, 0xA5, 0xF6, 0xCE, 0x95, 0x2F, 0x64, 0x23, 0xFB, 0xFD, 0x4F, 0x9B,
];

/// Interaction trace and environment snapshot of a PC browser runtime.
const INTERACTION_TRACE: &str = "0002000200004700000000000001ffd9ffa8005c23fff1ffff001501ff90fff9000022fff2ffff001501ffb800770061a5ffe3ffff002a01fffcffe70000f0ffe4ffff002a01ffd30000003e01ffd40000003e01ffc8ffff005301ffbdfffa006901ffbefffb006901ffb1fff4007d01ffa6ffee009301ffa8ffef009301ff9effe900a801ff96ffe600be01ff97ffe600be01ff93ffe500d301ff92ffe500ea01ff92ffe4010501ff92ffe3011b01ff92ffe402d101ff93ffe502f201ff94ffe6040501";
const ENVIRONMENT_SNAPSHOT: &str = "000000010000000000000000fffeffff00000000000000390001ffff00bc00000000006e00020001013400000000012f00000002011a00000000016100000000019f0000000002470000000002790000000002b1";

/// x-rap-param for `api` (`//edith.xiaohongshu.com/<path>`) and the request
/// data as compact JSON (the POST body, or the GET params as an object).
pub fn x_rap_param(api: &str, data_json: &str, now_ms: u64) -> String {
  let raw = body_structure(api, data_json, now_ms);
  let compressed = gzip(&raw, now_ms / 1000);
  pack_envelope(&compressed)
}

// ── block cipher ────────────────────────────────────────────────────────

fn gf_double(x: u8) -> u8 {
  let x = u16::from(x) << 1;
  (if x & 0x100 != 0 { x ^ 0x11B } else { x }) as u8
}

/// The four T-tables derived from the S-box (`_build_lookup_tables`).
fn lookup_tables() -> [[u32; 256]; 4] {
  let mut t = [[0u32; 256]; 4];
  for (i, &s) in SBOX.iter().enumerate() {
    let (s, a) = (u32::from(s), u32::from(gf_double(s)));
    let d = a ^ s;
    t[0][i] = (a << 24) | (s << 16) | (s << 8) | d;
    t[1][i] = (d << 24) | (a << 16) | (s << 8) | s;
    t[2][i] = (s << 24) | (d << 16) | (a << 8) | s;
    t[3][i] = (s << 24) | (s << 16) | (d << 8) | a;
  }
  t
}

/// Encrypt one block; shorter input is zero-padded.
fn encrypt_block(block: &[u8], lut: &[[u32; 256]; 4]) -> [u8; 16] {
  let mut padded = [0u8; 16];
  padded[..block.len()].copy_from_slice(block);
  let mut s: [u32; 4] = std::array::from_fn(|i| {
    u32::from_be_bytes(padded[i * 4..i * 4 + 4].try_into().expect("4 bytes")) ^ ROUND_KEYS[0][i]
  });
  for rk in &ROUND_KEYS[1..] {
    s = std::array::from_fn(|i| {
      lut[0][(s[i] >> 24) as usize]
        ^ lut[1][((s[(i + 1) % 4] >> 16) & 0xFF) as usize]
        ^ lut[2][((s[(i + 2) % 4] >> 8) & 0xFF) as usize]
        ^ lut[3][(s[(i + 3) % 4] & 0xFF) as usize]
        ^ rk[i]
    });
  }
  let last: Vec<u8> = LAST_ROUND_KEY
    .iter()
    .flat_map(|w| w.to_be_bytes())
    .collect();
  let mut out = [0u8; 16];
  for row in 0..4 {
    for col in 0..4 {
      let idx = (s[(row + col) & 3] >> (24 - 8 * col)) & 0xFF;
      out[4 * row + col] = SBOX[idx as usize] ^ last[4 * row + col];
    }
  }
  out
}

fn encrypt_blocks(src: &[u8]) -> Vec<u8> {
  let lut = lookup_tables();
  src
    .chunks(16)
    .flat_map(|b| encrypt_block(b, &lut))
    .collect()
}

// ── body ────────────────────────────────────────────────────────────────

fn random_ascii(len: usize) -> Vec<u8> {
  let charset = b"abcdefghijklmnopqrstuvwxyz0123456789";
  let mut rng = rand::rng();
  (0..len)
    .map(|_| *charset.choose(&mut rng).expect("charset"))
    .collect()
}

fn hex(s: &str) -> Vec<u8> {
  (0..s.len())
    .step_by(2)
    .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
    .collect()
}

/// TLV writer: big-endian `u16` tags.
struct Tlv(Vec<u8>);

impl Tlv {
  fn byte(&mut self, tag: u16) {
    self.0.extend_from_slice(&tag.to_be_bytes());
    self.0.push(0);
  }

  fn u32(&mut self, tag: u16, v: u32) {
    self.0.extend_from_slice(&tag.to_be_bytes());
    self.0.extend_from_slice(&v.to_be_bytes());
  }

  fn u64(&mut self, tag: u16, v: u64) {
    self.0.extend_from_slice(&tag.to_be_bytes());
    self.0.extend_from_slice(&v.to_be_bytes());
  }

  fn blob(&mut self, tag: u16, data: &[u8]) {
    self.u32(tag, data.len() as u32);
    self.0.extend_from_slice(data);
  }
}

/// The pre-compression payload (`_build_body_structure`).
pub fn body_structure(api: &str, data_json: &str, ts: u64) -> Vec<u8> {
  let mut rng = rand::rng();
  let xor_byte: u8 = rng.random_range(1..=255);
  let mut t = Tlv(Vec::with_capacity(512));
  t.u64(0x03E8, ts);
  t.u32(0x03E9, rng.random());
  t.blob(0x03EA, &random_ascii(16));
  t.u32(0x03EB, xxh32(format!("{api}{data_json}").as_bytes(), 0));
  // Capability flags.
  for tag in (1051..1066).chain([1070]).chain(1066..1070) {
    t.byte(tag);
  }
  t.u32(1100, 0);
  for tag in 1071..1074 {
    t.byte(tag);
  }
  // Runtime timing and environment.
  t.u32(1075, 0x564);
  t.u32(1076, 0x2C);
  t.u64(1077, ts.saturating_sub(0x434));
  t.blob(1078, &hex(INTERACTION_TRACE));
  t.u32(1082, 0);
  t.u32(1084, 0);
  t.u32(1085, 0);
  t.u32(1086, 100);
  t.u64(1087, ts.saturating_sub(0x2D7));
  t.blob(1088, &hex(ENVIRONMENT_SNAPSHOT));
  t.u32(1090, 0);
  t.u32(1097, 0);
  t.u32(1092, 0x566);
  t.u32(1094, 0x519);
  t.u64(1095, ts.saturating_sub(0x218C));
  t.u32(1093, 0);
  t.byte(1096);
  t.blob(1091, &[0x00, 0x00, 0xff, 0xff]);
  for tag in 1151..1157 {
    t.byte(tag);
  }
  // The first 16 bytes stay clear, the rest is XOR-masked.
  let mut buf = t.0;
  for b in &mut buf[16..] {
    *b ^= xor_byte;
  }
  buf
}

/// Gzip with the browser runtime's OS byte (0x03).
fn gzip(raw: &[u8], mtime: u64) -> Vec<u8> {
  let mut enc = GzBuilder::new()
    .mtime(mtime as u32)
    .operating_system(3)
    .write(Vec::new(), Compression::new(6));
  enc.write_all(raw).expect("in-memory write");
  enc.finish().expect("in-memory gzip")
}

/// Encrypt and wrap the compressed body (`_pack_envelope`).
fn pack_envelope(gz: &[u8]) -> String {
  let mut rng = rand::rng();
  let key = random_ascii(16);
  let salt = random_ascii(*[4, 5, 6].choose(&mut rng).expect("choice"));
  let xored: Vec<u8> = gz
    .iter()
    .enumerate()
    .map(|(i, b)| b ^ key[i % key.len()])
    .collect();
  let mut cipher_body = encrypt_blocks(&xored);
  cipher_body.extend_from_slice(&(gz.len() as u32).to_be_bytes());
  let mut content = salt.clone();
  content.extend_from_slice(&encrypt_blocks(&key));
  content.extend_from_slice(&16u32.to_be_bytes());
  content.extend_from_slice(&cipher_body);
  let mut out = vec![0x07, 0x24, 0x01, salt.len() as u8];
  for v in [
    1,
    20,
    cipher_body.len() as u32,
    xxh32(&content, 0),
    SDK_VERSION,
    rng.random_range(60..=240),
  ] {
    out.extend_from_slice(&v.to_be_bytes());
  }
  out.extend_from_slice(&[0; 8]);
  out.extend_from_slice(&content);
  STANDARD.encode(out)
}
