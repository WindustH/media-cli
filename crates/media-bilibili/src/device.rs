//! Anonymous device cookies (`buvid3`, `buvid4`, `b_nut`).
//!
//! Browsers get them on the first visit; without them many anonymous reads
//! fail with -352 / -412. They are fetched once from `/x/frontend/finger/spi`
//! and then live in the session's cookie jar.

use media_core::{Ctx, ValueExt};

const SPI: &str = "https://api.bilibili.com/x/frontend/finger/spi";

/// Make sure the jar holds device cookies. Best effort: without them the
/// request is still sent and may just be refused.
pub async fn ensure(ctx: &Ctx) {
  let http = &ctx.http;
  if http.has_cookie("buvid3") && http.has_cookie("buvid4") {
    return;
  }
  match http.get(SPI).value().await {
    Ok(spi) => {
      if let (Some(b3), Some(b4)) = (spi.str("data.b_3"), spi.str("data.b_4")) {
        http.set_cookie("buvid3", &b3);
        http.set_cookie("buvid4", &b4);
      }
    }
    Err(e) => tracing::debug!("buvid (spi) unavailable: {e}"),
  }
  if !http.has_cookie("b_nut") {
    http.set_cookie("b_nut", &jiff::Timestamp::now().as_second().to_string());
  }
}
