//! XYW signature of the creator APIs (`xhs_cli/creator_signing.py`):
//! MD5 of `url=<uri>[json]`, wrapped in an AES-128-CBC envelope.

use aes::Aes128;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockEncryptMut, KeyIvInit};
use md5::{Digest, Md5};
use serde_json::json;

use super::config::{XYW_AES_IV, XYW_AES_KEY, XYW_CREATOR_ENV_FLAGS, XYW_PREFIX};

/// x-s for `content` (`url=/path?query` plus the JSON body for POST).
pub fn xyw(content: &str, a1: &str, ts_ms: u64) -> String {
  let x1 = hex(&Md5::digest(content.as_bytes()));
  let plain = format!("x1={x1};x2={XYW_CREATOR_ENV_FLAGS};x3={a1};x4={ts_ms};");
  let payload = hex(&aes_cbc(STANDARD.encode(plain).as_bytes()));
  let envelope = json!({
    "signSvn": "56",
    "signType": "x2",
    "appId": "ugc",
    "signVersion": "1",
    "payload": payload,
  });
  format!("{XYW_PREFIX}{}", STANDARD.encode(envelope.to_string()))
}

fn aes_cbc(data: &[u8]) -> Vec<u8> {
  let mut buf = data.to_vec();
  buf.resize(data.len() + 16, 0);
  let len = cbc::Encryptor::<Aes128>::new(XYW_AES_KEY.into(), XYW_AES_IV.into())
    .encrypt_padded_mut::<Pkcs7>(&mut buf, data.len())
    .expect("buffer has room for padding")
    .len();
  buf.truncate(len);
  buf
}

pub fn hex(bytes: &[u8]) -> String {
  bytes.iter().map(|b| format!("{b:02x}")).collect()
}
