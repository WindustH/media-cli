//! Local files: format detection by magic bytes and images to upload.

use std::path::Path;

use crate::error::{Error, Result};

/// File extension for the format the first bytes of a file reveal.
pub fn sniff(head: &[u8]) -> Option<&'static str> {
  let at = |range: std::ops::Range<usize>| head.get(range).unwrap_or_default();
  if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
    return Some("jpg");
  }
  if head.starts_with(b"\x89PNG") {
    return Some("png");
  }
  if head.starts_with(b"GIF8") {
    return Some("gif");
  }
  if head.starts_with(b"RIFF") && at(8..12) == b"WEBP" {
    return Some("webp");
  }
  if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
    return Some("webm");
  }
  if head.starts_with(b"FLV") {
    return Some("flv");
  }
  if at(4..8) == b"ftyp" {
    return Some(match at(8..12) {
      b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1" => "heic",
      b"avif" | b"avis" => "avif",
      b"M4A " => "m4a",
      b"qt  " => "mov",
      _ => "mp4",
    });
  }
  None
}

/// An image read from disk, ready to upload.
#[derive(Debug, Clone)]
pub struct Image {
  pub data: Vec<u8>,
  /// `image/jpeg`, `image/png`, `image/gif`, `image/webp`, `image/heic` or `image/avif`.
  pub mime: &'static str,
  /// File name to send along, with an extension matching `mime`.
  pub name: String,
}

impl Image {
  /// Read `path` and detect its format from its content, not its extension.
  pub async fn read(path: &Path) -> Result<Self> {
    let data = tokio::fs::read(path)
      .await
      .map_err(|e| Error::input(format!("cannot read {}: {e}", path.display())))?;
    let ext = sniff(&data).unwrap_or_default();
    let mime = match ext {
      "jpg" => "image/jpeg",
      "png" => "image/png",
      "gif" => "image/gif",
      "webp" => "image/webp",
      "heic" => "image/heic",
      "avif" => "image/avif",
      _ => {
        return Err(Error::input(format!(
          "not a supported image: {}",
          path.display()
        )));
      }
    };
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("image");
    Ok(Self {
      data,
      mime,
      name: format!("{stem}.{ext}"),
    })
  }

  /// Extension matching the detected format.
  pub fn ext(&self) -> &str {
    self.name.rsplit_once('.').map(|(_, e)| e).unwrap_or("jpg")
  }
}
