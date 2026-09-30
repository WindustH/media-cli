//! Platform-agnostic kernel of media-cli.
//!
//! - [`platform`]: the [`Platform`] trait a platform crate implements, plus its static [`PlatformInfo`].
//! - [`model`]: normalized posts, users, comments ... printed the same way everywhere.
//! - [`http`]: Chrome-fingerprinted HTTP with cookie jar, pacing and retries.
//! - [`json`] / [`text`]: helpers for picking apart upstream payloads.
//! - [`app`] / [`cli`]: the shared command set and dispatch; [`output`]: tables and the JSON/YAML envelope.

mod account;
pub mod app;
pub mod browser;
pub mod cli;
pub mod ctx;
pub mod download;
pub mod error;
pub mod file;
pub mod guide;
pub mod http;
pub mod json;
pub mod model;
pub mod output;
pub mod paging;
pub mod platform;
mod qr;
pub mod store;
pub mod text;

pub use app::App;
pub use ctx::Ctx;
pub use error::{Error, ErrorCode, Result};
pub use http::Http;
pub use json::ValueExt;
pub use model::*;
pub use platform::{
  Cap, Choices, Draft, NoExtra, PageReq, Platform, PlatformInfo, QrStatus, QrTicket, Query,
};
pub use serde_json::{Value, json};
