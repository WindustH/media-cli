//! Image upload for new tweets: the chunked `media/upload` API
//! (INIT → APPEND → FINALIZE) with one base64 segment, as the reference does.

use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use media_core::{Error, Result, ValueExt};

use crate::api::Api;

const UPLOAD: &str = "https://upload.twitter.com/i/media/upload.json";
const MAX_BYTES: u64 = 5 * 1024 * 1024;

fn mime(path: &Path) -> Option<&'static str> {
  let ext = path.extension()?.to_str()?.to_ascii_lowercase();
  Some(match ext.as_str() {
    "jpg" | "jpeg" => "image/jpeg",
    "png" => "image/png",
    "gif" => "image/gif",
    "webp" => "image/webp",
    _ => return None,
  })
}

/// Upload one image; returns its media id.
pub async fn image(api: &Api, path: &Path) -> Result<String> {
  let media_type = mime(path).ok_or_else(|| {
    Error::input(format!(
      "unsupported image {} (jpeg, png, gif or webp)",
      path.display()
    ))
  })?;
  let data = std::fs::read(path)?;
  let size = data.len() as u64;
  if size > MAX_BYTES {
    return Err(Error::input(format!(
      "{} is {:.1} MB; images may be at most 5 MB",
      path.display(),
      size as f64 / 1048576.0
    )));
  }
  let size = size.to_string();
  let init = api
    .post_form(
      UPLOAD,
      &[
        ("command", "INIT"),
        ("total_bytes", &size),
        ("media_type", media_type),
      ],
    )
    .await?;
  let id = init
    .str("media_id_string")
    .ok_or_else(|| Error::upstream("media upload INIT returned no media id"))?;
  let encoded = STANDARD.encode(&data);
  api
    .post_form(
      UPLOAD,
      &[
        ("command", "APPEND"),
        ("media_id", &id),
        ("segment_index", "0"),
        ("media_data", &encoded),
      ],
    )
    .await?;
  api
    .post_form(UPLOAD, &[("command", "FINALIZE"), ("media_id", &id)])
    .await?;
  Ok(id)
}
