//! Comment sections: listing (main replies / one thread) and writing.
//!
//! Every commentable resource has an `(oid, type)` pair: a video is
//! `(aid, 1)`, a dynamic tells its own pair in `basic.comment_id_str/_type`.

use media_core::{Action, Comment, Ctx, Error, Page, PageReq, Result, Value, ValueExt, json};

use crate::page::Pn;
use crate::refs::{self, PostRef};
use crate::{api, dynamic, parse};

const LIST: &str = "https://api.bilibili.com/x/v2/reply";
const MAIN: &str = "https://api.bilibili.com/x/v2/reply/wbi/main";
const THREAD: &str = "https://api.bilibili.com/x/v2/reply/reply";
const ADD: &str = "https://api.bilibili.com/x/v2/reply/add";
const DEL: &str = "https://api.bilibili.com/x/v2/reply/del";

/// The comment section of a post.
pub struct Section {
  pub oid: String,
  pub kind: u64,
  /// The post it belongs to (BV id or dynamic id), for action results.
  pub post: String,
}

pub async fn section(ctx: &Ctx, post: &PostRef) -> Result<Section> {
  match post {
    PostRef::Video(v) => Ok(Section {
      oid: v.aid.to_string(),
      kind: 1,
      post: v.bvid.clone(),
    }),
    PostRef::Dynamic(id) => {
      let item = dynamic::detail(ctx, id).await?;
      match (
        item.str("basic.comment_id_str"),
        item.u64("basic.comment_type"),
      ) {
        (Some(oid), Some(kind)) => Ok(Section {
          oid,
          kind,
          post: id.clone(),
        }),
        _ => Err(Error::not_found(format!(
          "dynamic {id} has no comment section"
        ))),
      }
    }
  }
}

/// Main replies: `hot` (default, by likes, paged by number) or `time` (newest
/// first, offset cursor; anonymous visitors only get the first few).
pub async fn list(
  ctx: &Ctx,
  s: &Section,
  sort: Option<&str>,
  page: &PageReq,
) -> Result<Page<Comment>> {
  if sort == Some("time") {
    return latest(ctx, s, page).await;
  }
  let pn = Pn::of(page, 20);
  let data = api::get(ctx, LIST)
    .arg("oid", &s.oid)
    .arg("type", s.kind)
    .arg("pn", pn.pn)
    .arg("ps", pn.ps)
    .arg("sort", 1)
    .send()
    .await?;
  let pinned = (pn.pn == 1).then(|| data.at("upper.top"));
  let comments = with_pinned(pinned, data.list("replies"));
  Ok(pn.page(comments, pn.before(data.u64("page.count"))))
}

async fn latest(ctx: &Ctx, s: &Section, page: &PageReq) -> Result<Page<Comment>> {
  let offset = page.cursor.as_deref().unwrap_or_default();
  let data = api::get(ctx, MAIN)
    .arg("oid", &s.oid)
    .arg("type", s.kind)
    .arg("mode", 2)
    .arg("pagination_str", json!({ "offset": offset }).to_string())
    .arg("plat", 1)
    .arg("web_location", 1315875)
    .wbi()
    .send()
    .await?;
  let pinned = offset.is_empty().then(|| data.at("top_replies.0"));
  let comments = with_pinned(pinned, data.list("replies"));
  let next = data
    .str("cursor.pagination_reply.next_offset")
    .filter(|_| data.bool("cursor.is_end") == Some(false));
  Ok(Page::new(comments, next))
}

/// Replies, preceded by the pinned one (marked `extra.pinned`) when there is one.
fn with_pinned(pinned: Option<&Value>, replies: &[Value]) -> Vec<Comment> {
  let mut out: Vec<Comment> = Vec::new();
  if let Some(top) = pinned.filter(|t| t.is_object()) {
    let mut c = parse::comment(top);
    c.extra.insert("pinned".into(), json!(true));
    out.push(c);
  }
  for c in replies.iter().map(parse::comment) {
    if !out.iter().any(|p| p.id == c.id) {
      out.push(c);
    }
  }
  out
}

/// Replies inside the thread of root comment `root`.
pub async fn thread(ctx: &Ctx, s: &Section, root: &str, page: &PageReq) -> Result<Page<Comment>> {
  let pn = Pn::of(page, 20);
  let data = api::get(ctx, THREAD)
    .arg("oid", &s.oid)
    .arg("type", s.kind)
    .arg("root", root)
    .arg("pn", pn.pn)
    .arg("ps", pn.ps)
    .send()
    .await?;
  let replies = data.list("replies").iter().map(parse::comment).collect();
  Ok(pn.page(replies, pn.before(data.u64("page.count"))))
}

/// Comment on the post, or reply to `ROOT` / `ROOT:PARENT`.
pub async fn add(ctx: &Ctx, s: &Section, text: &str, reply_to: Option<&str>) -> Result<Action> {
  let mut call = api::post(ctx, ADD)
    .arg("oid", &s.oid)
    .arg("type", s.kind)
    .arg("message", text)
    .arg("plat", 1)
    .arg("statistics", r#"{"appId":100,"platform":5}"#)
    .arg("gaia_source", "main_web");
  if let Some(target) = reply_to {
    let (root, parent) = refs::reply_target(target)?;
    call = call.arg("root", root).arg("parent", parent);
  }
  let data = call.dm().wbi().send().await?;
  let verb = if reply_to.is_some() {
    "reply"
  } else {
    "comment"
  };
  let action = Action::done(verb, &s.post);
  Ok(match data.first_str(&["rpid_str", "rpid"]) {
    Some(id) => action.with_id(id),
    None => action,
  })
}

pub async fn delete(ctx: &Ctx, s: &Section, rpid: &str) -> Result<Action> {
  api::post(ctx, DEL)
    .arg("oid", &s.oid)
    .arg("type", s.kind)
    .arg("rpid", rpid)
    .send()
    .await?;
  Ok(Action::done("delete-comment", &s.post).with_id(rpid))
}
