//! Collect items across pages up to a limit.

use crate::error::Result;
use crate::model::Page;
use crate::platform::PageReq;

/// Upper bound on upstream calls for one listing, as a safety net against cursors that never end.
const MAX_PAGES: usize = 200;

/// Call `fetch` page after page, starting at `cursor`, until `limit` items are
/// collected or the listing ends. The returned `next_cursor` continues after
/// the last fetched page.
pub async fn collect<T>(
  limit: usize,
  cursor: Option<String>,
  mut fetch: impl AsyncFnMut(PageReq) -> Result<Page<T>>,
) -> Result<Page<T>> {
  let mut items = Vec::new();
  let mut cursor = cursor;
  let mut has_more = true;
  for _ in 0..MAX_PAGES {
    if items.len() >= limit {
      break;
    }
    let page = fetch(PageReq {
      cursor: cursor.clone(),
      size: limit - items.len(),
    })
    .await?;
    let got = page.items.len();
    items.extend(page.items);
    has_more = page.has_more && page.next_cursor.is_some() && page.next_cursor != cursor;
    cursor = page.next_cursor;
    if !has_more || got == 0 {
      has_more = has_more && got > 0;
      break;
    }
  }
  items.truncate(limit);
  Ok(Page {
    items,
    next_cursor: if has_more { cursor } else { None },
    has_more,
  })
}
