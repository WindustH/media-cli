//! `browse`: every page with a `browseId` (channels, playlists, feeds,
//! library pages) and its continuations. The first request names the page,
//! the following ones pass the previous page's token as the cursor.

use media_core::{PageReq, Result, Value, json};

use crate::api::Api;
use crate::parse::{self, Listing};

/// One page of `id` (with tab `params`), or the page after `cursor`.
pub async fn page(api: &Api, id: &str, params: Option<&str>, req: &PageReq) -> Result<Value> {
  let body = match &req.cursor {
    Some(token) => json!({ "continuation": token }),
    None => {
      let mut b = json!({ "browseId": id });
      if let Some(p) = params {
        b["params"] = Value::from(p);
      }
      b
    }
  };
  api.call("browse", body).await
}

/// The items of one page.
pub async fn listing(api: &Api, id: &str, params: Option<&str>, req: &PageReq) -> Result<Listing> {
  Ok(parse::listing(&page(api, id, params, req).await?))
}
