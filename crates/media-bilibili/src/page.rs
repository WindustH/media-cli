//! Page-number paging (`pn` / `ps`) behind opaque `PN:PS` cursors.
//!
//! The page size is fixed by the first request, so later pages line up even
//! when the caller asks for fewer items.

use media_core::{Page, PageReq};

#[derive(Debug, Clone, Copy)]
pub struct Pn {
  pub pn: u64,
  pub ps: usize,
}

impl Pn {
  pub fn of(req: &PageReq, max: usize) -> Self {
    let cursor = req.cursor.as_deref().unwrap_or_default();
    let (pn, ps) = cursor.split_once(':').unwrap_or((cursor, ""));
    Self {
      pn: pn.parse().unwrap_or(1).max(1),
      ps: ps.parse().unwrap_or_else(|_| req.size_within(max)),
    }
  }

  /// This page's items; `more` says whether another page follows.
  pub fn page<T>(self, items: Vec<T>, more: bool) -> Page<T> {
    let more = more && !items.is_empty();
    Page::new(items, more.then(|| format!("{}:{}", self.pn + 1, self.ps)))
  }

  /// Whether `total` items reach past this page.
  pub fn before(self, total: Option<u64>) -> bool {
    total.is_some_and(|t| self.pn * (self.ps as u64) < t)
  }
}
