//! A minimal protobuf writer for the opaque `params` / continuation tokens
//! InnerTube takes (search filters, comment sections, comment creation).
//! Field layouts follow YouTube.js `protos/misc/params.proto` and tokens the
//! web app itself sends.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE;

#[derive(Default)]
pub struct Msg(Vec<u8>);

impl Msg {
  pub fn new() -> Self {
    Self::default()
  }

  fn varint(&mut self, mut n: u64) {
    while n >= 0x80 {
      self.0.push((n as u8 & 0x7f) | 0x80);
      n >>= 7;
    }
    self.0.push(n as u8);
  }

  fn tag(&mut self, field: u32, wire: u64) {
    self.varint(u64::from(field) << 3 | wire);
  }

  /// A varint field.
  pub fn int(mut self, field: u32, n: u64) -> Self {
    self.tag(field, 0);
    self.varint(n);
    self
  }

  /// A length-delimited field (string, bytes or nested message).
  pub fn bytes(mut self, field: u32, data: &[u8]) -> Self {
    self.tag(field, 2);
    self.varint(data.len() as u64);
    self.0.extend_from_slice(data);
    self
  }

  pub fn str(self, field: u32, s: &str) -> Self {
    self.bytes(field, s.as_bytes())
  }

  pub fn msg(self, field: u32, m: Msg) -> Self {
    self.bytes(field, &m.0)
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  /// URL-safe base64 with padding, the form continuation tokens travel in.
  pub fn encode(&self) -> String {
    URL_SAFE.encode(&self.0)
  }
}
