//! Session simulation (xhshow `session.py`): a fixed page-load time and
//! counters that grow between requests, like one open browser tab.

use rand::Rng;

use super::config::{
  SESSION_SEQUENCE_INIT, SESSION_SEQUENCE_STEP, SESSION_WINDOW_PROPS_INIT,
  SESSION_WINDOW_PROPS_STEP,
};

/// State that goes into one signature.
#[derive(Debug, Clone, Copy)]
pub struct SignState {
  pub page_load_timestamp: u64,
  pub sequence_value: u32,
  pub window_props_length: u32,
  pub uri_length: u32,
}

#[derive(Debug)]
pub struct SignSession {
  page_load_timestamp: u64,
  sequence_value: u32,
  window_props_length: u32,
}

impl SignSession {
  pub fn new(now_ms: u64) -> Self {
    let mut rng = rand::rng();
    Self {
      page_load_timestamp: now_ms,
      sequence_value: rng.random_range(SESSION_SEQUENCE_INIT.0..=SESSION_SEQUENCE_INIT.1),
      window_props_length: rng
        .random_range(SESSION_WINDOW_PROPS_INIT.0..=SESSION_WINDOW_PROPS_INIT.1),
    }
  }

  /// Advance the counters and return the state for signing `content`
  /// (the signed URI with query or body; its length counts characters, like Python's `len`).
  pub fn next(&mut self, content: &str) -> SignState {
    let mut rng = rand::rng();
    self.sequence_value += rng.random_range(SESSION_SEQUENCE_STEP.0..=SESSION_SEQUENCE_STEP.1);
    self.window_props_length +=
      rng.random_range(SESSION_WINDOW_PROPS_STEP.0..=SESSION_WINDOW_PROPS_STEP.1);
    SignState {
      page_load_timestamp: self.page_load_timestamp,
      sequence_value: self.sequence_value,
      window_props_length: self.window_props_length,
      uri_length: content.chars().count() as u32,
    }
  }
}
