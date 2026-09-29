//! One error type for every platform, carrying a stable machine-readable code.

use std::fmt;

use serde::Serialize;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Stable error codes shared by all platforms (`error.code` in structured output).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
  NotAuthenticated,
  VerificationRequired,
  IpBlocked,
  RateLimited,
  SignatureError,
  InvalidInput,
  NotFound,
  PermissionDenied,
  UnsupportedOperation,
  NetworkError,
  UpstreamError,
  InternalError,
}

impl ErrorCode {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::NotAuthenticated => "not_authenticated",
      Self::VerificationRequired => "verification_required",
      Self::IpBlocked => "ip_blocked",
      Self::RateLimited => "rate_limited",
      Self::SignatureError => "signature_error",
      Self::InvalidInput => "invalid_input",
      Self::NotFound => "not_found",
      Self::PermissionDenied => "permission_denied",
      Self::UnsupportedOperation => "unsupported_operation",
      Self::NetworkError => "network_error",
      Self::UpstreamError => "upstream_error",
      Self::InternalError => "internal_error",
    }
  }

  /// Process exit code: 2 for bad input (like clap), 3 for auth problems, 1 otherwise.
  pub fn exit_code(self) -> u8 {
    match self {
      Self::InvalidInput => 2,
      Self::NotAuthenticated => 3,
      _ => 1,
    }
  }
}

#[derive(Debug, Clone, Serialize)]
pub struct Error {
  pub code: ErrorCode,
  pub message: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub hint: Option<String>,
}

impl Error {
  pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
    Self {
      code,
      message: message.into(),
      hint: None,
    }
  }

  pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
    self.hint = Some(hint.into());
    self
  }

  pub fn auth(message: impl Into<String>) -> Self {
    Self::new(ErrorCode::NotAuthenticated, message)
  }

  pub fn input(message: impl Into<String>) -> Self {
    Self::new(ErrorCode::InvalidInput, message)
  }

  pub fn not_found(message: impl Into<String>) -> Self {
    Self::new(ErrorCode::NotFound, message)
  }

  pub fn upstream(message: impl Into<String>) -> Self {
    Self::new(ErrorCode::UpstreamError, message)
  }

  pub fn network(message: impl Into<String>) -> Self {
    Self::new(ErrorCode::NetworkError, message)
  }

  pub fn internal(message: impl Into<String>) -> Self {
    Self::new(ErrorCode::InternalError, message)
  }

  pub fn unsupported(operation: &str) -> Self {
    Self::new(
      ErrorCode::UnsupportedOperation,
      format!("`{operation}` is not supported on this platform"),
    )
  }
}

impl fmt::Display for Error {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.message)
  }
}

impl std::error::Error for Error {}

impl From<wreq::Error> for Error {
  fn from(e: wreq::Error) -> Self {
    Self::network(e.to_string())
  }
}

impl From<serde_json::Error> for Error {
  fn from(e: serde_json::Error) -> Self {
    Self::upstream(format!("unexpected response: {e}"))
  }
}

impl From<std::io::Error> for Error {
  fn from(e: std::io::Error) -> Self {
    Self::internal(e.to_string())
  }
}
