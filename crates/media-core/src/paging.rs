//! Collect items across pages up to a limit.
//!
//! Cursors handed to the user may carry a `!N` suffix: "the page at this
//! platform cursor, minus its first N items". That lets a listing stop in the
//! middle of an upstream page and continue later without skipping or repeating
//! anything, even on platforms whose page size cannot be chosen.

use jiff::Timestamp;

use crate::error::Result;
use crate::model::{Dated, Page};
use crate::platform::PageReq;

/// Upper bound on upstream calls for one listing, as a safety net against cursors that never end.
const MAX_PAGES: usize = 200;
/// Give up after this many consecutive pages without items (but keep the cursor).
const MAX_EMPTY_PAGES: usize = 3;
const SKIP_MARK: char = '!';

fn split_cursor(cursor: Option<String>) -> (Option<String>, usize) {
  let Some(c) = cursor else { return (None, 0) };
  if let Some((base, n)) = c.rsplit_once(SKIP_MARK)
    && let Ok(n) = n.parse()
  {
    return ((!base.is_empty()).then(|| base.to_owned()), n);
  }
  (Some(c), 0)
}

fn join_cursor(cursor: Option<&str>, skip: usize) -> String {
  format!("{}{SKIP_MARK}{skip}", cursor.unwrap_or_default())
}

/// Call `fetch` page after page, starting at `cursor`, until `limit` items are
/// collected or the listing ends.
pub async fn collect<T>(
  limit: usize,
  cursor: Option<String>,
  mut fetch: impl AsyncFnMut(PageReq) -> Result<Page<T>>,
) -> Result<Page<T>> {
  let (mut cursor, mut skip) = split_cursor(cursor);
  let mut items = Vec::new();
  let mut empty_run = 0;
  for _ in 0..MAX_PAGES {
    let want = limit - items.len();
    let page = fetch(PageReq {
      cursor: cursor.clone(),
      size: want.saturating_add(skip),
    })
    .await?;
    let more = page.has_more && page.next_cursor.is_some() && page.next_cursor != cursor;
    let fresh: Vec<T> = page.items.into_iter().skip(skip).collect();
    if fresh.len() > want {
      // More than needed: the next call resumes inside this page.
      let next = join_cursor(cursor.as_deref(), skip + want);
      items.extend(fresh.into_iter().take(want));
      return Ok(Page::new(items, Some(next)));
    }
    empty_run = if fresh.is_empty() { empty_run + 1 } else { 0 };
    items.extend(fresh);
    if !more {
      return Ok(Page::last(items));
    }
    cursor = page.next_cursor;
    skip = 0;
    if items.len() >= limit || empty_run >= MAX_EMPTY_PAGES {
      break;
    }
  }
  Ok(Page::new(items, cursor))
}

/// `--since` / `--until` bounds of a listing (inclusive).
#[derive(Debug, Clone, Copy, Default)]
pub struct Window {
  pub since: Option<Timestamp>,
  pub until: Option<Timestamp>,
}

impl Window {
  pub fn is_open(&self) -> bool {
    self.since.is_none() && self.until.is_none()
  }

  /// Undated items are kept: the window cannot judge them.
  fn contains(&self, at: Option<Timestamp>) -> bool {
    let Some(at) = at else { return true };
    self.since.is_none_or(|s| at >= s) && self.until.is_none_or(|u| at <= u)
  }

  fn before(&self, at: Option<Timestamp>) -> bool {
    matches!((at, self.since), (Some(at), Some(s)) if at < s)
  }
}

/// [`collect`] keeping only items inside `window`. A page whose items are all
/// older than `since` ends the listing, which suits newest-first listings.
pub async fn collect_window<T: Dated>(
  limit: usize,
  cursor: Option<String>,
  window: Window,
  mut fetch: impl AsyncFnMut(PageReq) -> Result<Page<T>>,
) -> Result<Page<T>> {
  if window.is_open() {
    return collect(limit, cursor, fetch).await;
  }
  collect(limit, cursor, async |req| {
    let mut page = fetch(req).await?;
    let past = !page.items.is_empty() && page.items.iter().all(|i| window.before(i.date()));
    page.items.retain(|i| window.contains(i.date()));
    if past {
      page.has_more = false;
      page.next_cursor = None;
    }
    Ok(page)
  })
  .await
}
