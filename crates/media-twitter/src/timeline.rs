//! GraphQL timelines: entries and cursors of their `instructions`, and
//! paging (one upstream page per call, the bottom cursor as `next_cursor`).

use std::collections::HashSet;

use media_core::{Page, PageReq, Post, Result, User, Value, ValueExt, json};

use crate::api::Api;
use crate::graphql::Op;
use crate::parse;

/// Upstream page size for timelines.
pub const PAGE_MAX: usize = 40;

/// Common variables of a timeline request.
pub fn vars(req: &PageReq) -> Value {
  json!({ "count": req.size_within(PAGE_MAX) })
}

/// One page of `op`: `paths` lead to its `instructions`, `extract` picks the items.
/// A page with a cursor but no items (it happens) is skipped once.
pub async fn page<T>(
  api: &Api,
  op: &Op,
  mut variables: Value,
  paths: &[&str],
  req: &PageReq,
  extract: fn(&Timeline) -> Vec<T>,
) -> Result<Page<T>> {
  let mut cursor = req.cursor.clone();
  for _ in 0..2 {
    if let Some(c) = &cursor {
      variables["cursor"] = c.as_str().into();
    }
    let data = api.graphql(op, variables.clone()).await?;
    let tl = Timeline::at(&data, paths);
    let items = extract(&tl);
    let next = tl.bottom.filter(|c| Some(c) != cursor.as_ref());
    match next {
      Some(next) if items.is_empty() => cursor = Some(next),
      Some(next) => return Ok(Page::new(items, Some(next))),
      None => return Ok(Page::last(items)),
    }
  }
  Ok(Page::last(Vec::new()))
}

/// Merge `extra` into the object `base`.
pub fn with(mut base: Value, extra: Value) -> Value {
  if let (Some(b), Value::Object(e)) = (base.as_object_mut(), extra) {
    b.extend(e);
  }
  base
}

/// One timeline entry: its id and the `itemContent`s it holds (one for a
/// plain item, several for a module such as a conversation thread).
pub struct Entry<'a> {
  pub id: &'a str,
  pub items: Vec<(&'a str, &'a Value)>,
}

/// Entries and cursors of a timeline's `instructions`.
#[derive(Default)]
pub struct Timeline<'a> {
  pub entries: Vec<Entry<'a>>,
  /// Cursor of the next page.
  pub bottom: Option<String>,
  /// "Show more replies" cursor of a conversation (hidden / low-ranked threads).
  pub more: Option<String>,
}

impl<'a> Timeline<'a> {
  /// Read the instructions at the first of `paths` that holds a list.
  pub fn at(data: &'a Value, paths: &[&str]) -> Self {
    let instructions = paths
      .iter()
      .map(|p| data.list(p))
      .find(|l| !l.is_empty())
      .unwrap_or_default();
    let mut tl = Timeline::default();
    for ins in instructions {
      // Pinned tweets come back on every page; like the reference, skip them.
      let single = ins.at("entry");
      let single = (ins.str("type").as_deref() != Some("TimelinePinEntry") && single.is_object())
        .then_some(single);
      let entries = ins
        .list("entries")
        .iter()
        .chain(single)
        .chain(ins.list("moduleItems"));
      for e in entries {
        tl.add(e);
      }
    }
    tl
  }

  fn add(&mut self, e: &'a Value) {
    let id = e.at("entryId").as_str().unwrap_or_default();
    let content = e.at("content");
    if self.cursor(content) || self.cursor(content.at("itemContent")) {
      return;
    }
    let mut items = Vec::new();
    if content.at("itemContent").is_object() {
      items.push((id, content.at("itemContent")));
    }
    if e.at("item.itemContent").is_object() {
      items.push((id, e.at("item.itemContent")));
    }
    for it in content.list("items") {
      let item_id = it.at("entryId").as_str().unwrap_or(id);
      let item = it.at("item.itemContent");
      if !self.cursor(item) && item.is_object() {
        items.push((item_id, item));
      }
    }
    if !items.is_empty() {
      self.entries.push(Entry { id, items });
    }
  }

  /// Record a cursor; true when `c` is one.
  fn cursor(&mut self, c: &Value) -> bool {
    let Some(kind) = c.str("cursorType") else {
      return false;
    };
    let value = c.str("value");
    match kind.as_str() {
      "Bottom" => self.bottom = value.or(self.bottom.take()),
      "ShowMoreThreads" | "ShowMoreThreadsPrompt" => self.more = value.or(self.more.take()),
      _ => {}
    }
    true
  }

  /// All `itemContent`s with their entry ids.
  pub fn items(&self) -> impl Iterator<Item = &(&'a str, &'a Value)> {
    self.entries.iter().flat_map(|e| &e.items)
  }
}

/// Tweets of a timeline, promoted ones skipped, deduplicated.
pub fn posts(tl: &Timeline) -> Vec<Post> {
  let mut seen = HashSet::new();
  tl.items()
    .filter(|(id, item)| !is_promoted(id, item))
    .filter_map(|(_, item)| parse::post(item.at("tweet_results.result")))
    .filter(|p| seen.insert(p.id.clone()))
    .collect()
}

/// Users of a timeline (followers, search results ...).
pub fn users(tl: &Timeline) -> Vec<User> {
  tl.items()
    .filter_map(|(_, item)| parse::user(item.at("user_results.result")))
    .collect()
}

fn is_promoted(entry_id: &str, item: &Value) -> bool {
  entry_id.starts_with("promoted") || item.at("promotedMetadata").is_object()
}
