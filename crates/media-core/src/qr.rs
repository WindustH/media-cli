//! Show a login QR code in the terminal and save it as an SVG next to the cache.

use std::path::{Path, PathBuf};

use qrcode::QrCode;
use qrcode::render::{svg, unicode};

use crate::error::{Error, Result};

/// Print `data` as a QR code on stderr and write `login-qr.svg` into `dir`.
pub fn show(data: &str, dir: &Path) -> Result<PathBuf> {
  let code = QrCode::new(data.as_bytes()).map_err(|e| Error::internal(format!("qr code: {e}")))?;
  let text = code
    .render::<unicode::Dense1x2>()
    .dark_color(unicode::Dense1x2::Light)
    .light_color(unicode::Dense1x2::Dark)
    .quiet_zone(true)
    .build();
  eprintln!("{text}");
  std::fs::create_dir_all(dir)?;
  let path = dir.join("login-qr.svg");
  let image = code.render::<svg::Color>().min_dimensions(256, 256).build();
  std::fs::write(&path, image)?;
  Ok(path)
}
