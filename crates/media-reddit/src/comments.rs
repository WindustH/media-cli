//! Comment threads, complete. The first page is `/comments/<post>`; later
//! pages load the top-level comments Reddit left in its `more` stub through
//! `/api/morechildren`. `replies` returns the whole subtree of a comment,
//! loading every stub below it (`/api/morechildren`, or the comment's own
//! thread for "continue this thread"). Trees are kept per post for the run,
//! so `comments --replies` only asks for what is missing.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use media_core::{Comment, Error, Page, PageReq, Result, Value, ValueExt};

use crate::api::Api;
use crate::refs;
use crate::thread::Tree;

pub const SORTS: &[&str] = &["confidence", "top", "new", "controversial", "old", "qa"];
/// Upstream cap of `limit` on `/comments/<post>` (it counts nested replies).
const MAX_LIMIT: usize = 500;
/// Ids per `/api/morechildren` call.
const BATCH: usize = 100;
/// Safety net against stubs that never resolve.
const MAX_LOADS: usize = 2000;

/// Comment trees of this run, by post id.
#[derive(Default)]
pub struct Threads(RefCell<HashMap<String, Tree>>);

impl Threads {
  fn take(&self, post: &str) -> Option<Tree> {
    self.0.borrow_mut().remove(post)
  }

  fn put(&self, post: &str, tree: Tree) {
    self.0.borrow_mut().insert(post.to_owned(), tree);
  }
}

/// Levels of replies per request: few for a short listing, which then holds
/// more threads; the most Reddit allows when everything is wanted.
fn depth(limit: usize) -> usize {
  if limit >= MAX_LIMIT { 10 } else { 4 }
}

/// Add `/comments/<post>` (or one comment's thread with `focus`) to `tree`.
async fn load(
  api: &Api,
  tree: &mut Tree,
  post: &str,
  focus: Option<&str>,
  sort: Option<&str>,
  limit: usize,
) -> Result<()> {
  let path = match focus {
    Some(c) => format!("/comments/{post}/_/{c}"),
    None => format!("/comments/{post}"),
  };
  let mut query = vec![
    ("limit", limit.to_string()),
    ("depth", depth(limit).to_string()),
  ];
  if let Some(sort) = sort {
    query.push(("sort", sort.to_owned()));
  }
  let v = api.get(&path, &query).await?;
  tree.add_listing(v.at("1"));
  Ok(())
}

/// `/api/morechildren`: the comments `ids` (at most [`BATCH`]) with replies.
async fn more_children(
  api: &Api,
  post: &str,
  ids: &[String],
  sort: Option<&str>,
) -> Result<Vec<Value>> {
  let mut query = vec![
    ("api_type", "json".into()),
    ("link_id", format!("t3_{post}")),
    ("children", ids.join(",")),
    ("limit_children", "false".into()),
    ("depth", depth(MAX_LIMIT).to_string()),
  ];
  if let Some(sort) = sort {
    query.push(("sort", sort.to_owned()));
  }
  let v = api.get("/api/morechildren", &query).await?;
  if let Some(e) = v.list("json.errors").first() {
    return Err(Error::upstream(format!(
      "Reddit refused to load comments: {e}"
    )));
  }
  Ok(v.list("json.data.things").to_vec())
}

/// `LIMIT:OFFSET`: the first page's size, and how many ids of its
/// top-level stub were loaded.
fn cursor(c: &str) -> Result<(usize, usize)> {
  c.split_once(':')
    .and_then(|(l, o)| Some((l.parse().ok()?, o.parse().ok()?)))
    .ok_or_else(|| Error::input(format!("not a Reddit comments cursor: {c}")))
}

pub async fn list(
  api: &Api,
  threads: &Threads,
  post: &str,
  sort: Option<&str>,
  req: &PageReq,
) -> Result<Page<Comment>> {
  let id = refs::post(&api.ctx, post).await?;
  let Some(c) = &req.cursor else {
    let limit = req.size.saturating_mul(5).clamp(50, MAX_LIMIT);
    let mut tree = Tree::default();
    load(api, &mut tree, &id, None, sort, limit).await?;
    let items = tree.top.iter().filter_map(|c| tree.build(c)).collect();
    let next = (!tree.more_top().is_empty()).then(|| format!("{limit}:0"));
    threads.put(&id, tree);
    return Ok(Page::new(items, next));
  };
  let (limit, offset) = cursor(c)?;
  let mut tree = match threads.take(&id) {
    Some(tree) => tree,
    None => {
      let mut tree = Tree::default();
      load(api, &mut tree, &id, None, sort, limit).await?;
      tree
    }
  };
  let all = tree.more_top();
  let batch: Vec<String> = all
    .iter()
    .skip(offset)
    .take(req.size_within(BATCH))
    .cloned()
    .collect();
  let loaded = load_top(api, &mut tree, &id, &batch, sort).await;
  let items = batch.iter().filter_map(|c| tree.build(c)).collect();
  threads.put(&id, tree);
  loaded?;
  let end = offset + batch.len();
  let next = (end < all.len()).then(|| format!("{limit}:{end}"));
  Ok(Page::new(items, next))
}

/// Load top-level comments `ids`. Reddit may answer part of a batch (and
/// stub the rest), so ask again for what is still missing.
async fn load_top(
  api: &Api,
  tree: &mut Tree,
  post: &str,
  ids: &[String],
  sort: Option<&str>,
) -> Result<()> {
  let mut missing = ids.to_vec();
  for _ in 0..3 {
    if missing.is_empty() {
      break;
    }
    let before = missing.len();
    tree.add_flat(&more_children(api, post, &missing, sort).await?);
    missing.retain(|c| !tree.has(c));
    if missing.len() == before {
      break;
    }
  }
  Ok(())
}

/// Every reply under one comment (`/comments/<post>/_/<comment>`), nested,
/// with nothing left out.
pub async fn replies(
  api: &Api,
  threads: &Threads,
  post: &str,
  comment: &str,
) -> Result<Page<Comment>> {
  let id = refs::post(&api.ctx, post).await?;
  let cid = refs::comment_id(comment)?;
  let mut tree = threads.take(&id).unwrap_or_default();
  let result = complete(api, &mut tree, &id, &cid).await;
  let root = tree.build(&cid);
  threads.put(&id, tree);
  result?;
  let root =
    root.ok_or_else(|| Error::not_found(format!("comment {cid} not found under post {id}")))?;
  Ok(Page::last(root.replies))
}

/// Load comment `cid` (unless known) and every stub below it into `tree`.
async fn complete(api: &Api, tree: &mut Tree, post: &str, cid: &str) -> Result<()> {
  if !tree.has(cid) {
    load(api, tree, post, Some(cid), None, MAX_LIMIT).await?;
  }
  let mut continued = HashSet::new();
  for _ in 0..MAX_LOADS {
    let Some(stub) = tree.take_stub_under(cid) else {
      return Ok(());
    };
    if stub.children.is_empty() {
      // "Continue this thread": the parent's own thread goes deeper.
      let parent = stub.parent.trim_start_matches("t1_").to_owned();
      if continued.insert(parent.clone()) {
        load(api, tree, post, Some(&parent), None, MAX_LIMIT).await?;
      }
      continue;
    }
    let ids: Vec<String> = stub.children.into_iter().filter(|c| !tree.has(c)).collect();
    for ids in ids.chunks(BATCH) {
      tree.add_flat(&more_children(api, post, ids, None).await?);
    }
  }
  tracing::warn!("comment {cid}: gave up after {MAX_LOADS} loads");
  Ok(())
}
