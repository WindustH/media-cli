//! A post's comment tree, which Reddit hands out in pieces: the nested
//! listing of `/comments/<post>`, the flat answers of `/api/morechildren`,
//! and `more` stubs for whatever is still missing. Pieces are merged by id
//! and the nested [`Comment`]s are rebuilt on demand.

use std::collections::{HashMap, HashSet};

use media_core::{Comment, Value, ValueExt};

use crate::parse;

/// Replies Reddit left out under `parent` (`t1_…`, or `t3_…` for top-level
/// comments): `children` to load with `/api/morechildren`, or none for
/// "continue this thread", which needs the parent's own thread.
#[derive(Debug)]
pub struct Stub {
  pub parent: String,
  pub children: Vec<String>,
  /// Comments behind the stub, replies of replies included (Reddit's `count`).
  count: u64,
}

impl Stub {
  /// Hidden comments; a "continue this thread" link hides at least one.
  fn hidden(&self) -> u64 {
    self.count.max(self.children.len() as u64).max(1)
  }
}

struct Node {
  /// Without `replies`, `reply_count` and `more`; [`Tree::build`] adds them.
  comment: Comment,
  children: Vec<String>,
}

#[derive(Default)]
pub struct Tree {
  nodes: HashMap<String, Node>,
  /// Top-level comment ids in order.
  pub top: Vec<String>,
  stubs: Vec<Stub>,
  /// Children of comments not loaded yet, by parent id.
  orphans: HashMap<String, Vec<String>>,
}

impl Tree {
  pub fn has(&self, id: &str) -> bool {
    self.nodes.contains_key(id)
  }

  /// Add the comments of a nested listing (`{data: {children}}`).
  pub fn add_listing(&mut self, listing: &Value) {
    for thing in listing.list("data.children") {
      self.add(thing);
    }
  }

  /// Add the flat things of `/api/morechildren`; each names its parent.
  pub fn add_flat(&mut self, things: &[Value]) {
    for thing in things {
      self.add(thing);
    }
  }

  fn add(&mut self, thing: &Value) {
    let d = thing.at("data");
    let Some(parent) = d.str("parent_id") else {
      return;
    };
    match thing.str("kind").as_deref() {
      Some("t1") => {
        let reply_to = parent
          .strip_prefix("t1_")
          .and_then(|p| self.nodes.get(p))
          .and_then(|n| n.comment.author.as_ref())
          .map(|a| a.name.clone());
        if let Some(c) = parse::comment_node(d, reply_to.as_deref()) {
          self.insert(c, &parent);
        }
        self.add_listing(d.at("replies"));
      }
      Some("more") => self.stubs.push(Stub {
        parent,
        children: d
          .list("children")
          .iter()
          .filter_map(|c| c.as_str().map(str::to_owned))
          .collect(),
        count: d.u64("count").unwrap_or(0),
      }),
      _ => {}
    }
  }

  fn insert(&mut self, comment: Comment, parent: &str) {
    let id = comment.id.clone();
    let siblings = match parent.strip_prefix("t1_") {
      Some(p) => match self.nodes.get_mut(p) {
        Some(n) => &mut n.children,
        // Its parent may come later.
        None => self.orphans.entry(p.to_owned()).or_default(),
      },
      None => &mut self.top,
    };
    if !siblings.contains(&id) {
      siblings.push(id.clone());
    }
    // A comment loaded again keeps the replies found so far.
    match self.nodes.get_mut(&id) {
      Some(node) => node.comment = comment,
      None => {
        let children = self.orphans.remove(&id).unwrap_or_default();
        self.nodes.insert(id, Node { comment, children });
      }
    }
  }

  /// Ids behind the top-level stubs, in order: the pages after the first.
  pub fn more_top(&self) -> Vec<String> {
    let mut seen = HashSet::new();
    self
      .stubs
      .iter()
      .filter(|s| s.parent.starts_with("t3_"))
      .flat_map(|s| s.children.iter())
      .filter(|id| seen.insert(id.as_str()))
      .cloned()
      .collect()
  }

  /// Remove and return one stub somewhere below comment `root`.
  pub fn take_stub_under(&mut self, root: &str) -> Option<Stub> {
    let mut inside = HashSet::from([root.to_owned()]);
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
      for c in self
        .nodes
        .get(id)
        .map(|n| n.children.as_slice())
        .unwrap_or_default()
      {
        inside.insert(c.clone());
        stack.push(c);
      }
    }
    let i = self.stubs.iter().position(|s| {
      s.parent
        .strip_prefix("t1_")
        .is_some_and(|p| inside.contains(p))
    })?;
    Some(self.stubs.remove(i))
  }

  /// Comment `id` with every loaded reply nested under it. `reply_count`
  /// counts all replies below it, loaded or not; `extra.more` the replies
  /// directly under it that are not loaded.
  pub fn build(&self, id: &str) -> Option<Comment> {
    let mut hidden: HashMap<&str, u64> = HashMap::new();
    for s in &self.stubs {
      if let Some(p) = s.parent.strip_prefix("t1_") {
        *hidden.entry(p).or_default() += s.hidden();
      }
    }
    self.build_with(id, &hidden)
  }

  fn build_with(&self, id: &str, hidden: &HashMap<&str, u64>) -> Option<Comment> {
    let node = self.nodes.get(id)?;
    let mut c = node.comment.clone();
    c.replies = node
      .children
      .iter()
      .filter_map(|k| self.build_with(k, hidden))
      .collect();
    let more = hidden.get(id).copied().unwrap_or(0);
    let below: u64 = c
      .replies
      .iter()
      .map(|r| 1 + r.reply_count.unwrap_or(0))
      .sum();
    c.reply_count = Some(below + more).filter(|n| *n > 0);
    if more > 0 {
      c.extra.insert("more".into(), more.into());
    }
    Some(c)
  }
}
