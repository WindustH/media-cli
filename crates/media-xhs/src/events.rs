//! The creator center's activity center (活动中心): official campaigns that
//! creators join by publishing a note for them, and the activities they keep.
//!
//! Endpoints from the creator center bundle: `EVENTS_LIST` and
//! `COLLECT_MODIFY` in the index API list, called by chunk 6808. There is no
//! detail endpoint: the page shows the list entries. Joining is publishing a
//! note "associated" with the activity, as the publish page's 关联活动 panel
//! does (see [`association`]).

use media_core::text::{from_millis, html_to_text};
use media_core::{Action, Collection, Draft, Error, Page, Result, Value, ValueExt, json};

use crate::api::Client;
use crate::creator;

const LIST: &str = "/api/galaxy/v2/creator/activity_center/list";
const KEEP: &str = "/api/galaxy/v2/creator/activity_favor";

/// The activity list: all (`type=1`) or the kept ones (`type=2`), in the
/// center's order (`sort=1`) or latest first (`sort=2`).
pub async fn list(c: &Client, kept: bool, latest: bool) -> Result<Vec<Value>> {
  c.require_login()?;
  let kind = if kept { "2" } else { "1" };
  let sort = if kept || latest { "2" } else { "1" };
  let data = c
    .creator_get(
      LIST,
      &[
        ("sort", sort),
        ("type", kind),
        ("source", "3"),
        ("topic_activity", "0"),
      ],
    )
    .await?;
  Ok(data.list("activity_list").to_vec())
}

/// Activities whose name, reward or topics contain `keyword` (case-insensitive).
pub fn matching(rows: Vec<Value>, keyword: Option<&str>) -> Vec<Value> {
  let Some(k) = keyword.map(str::to_lowercase).filter(|k| !k.is_empty()) else {
    return rows;
  };
  rows
    .into_iter()
    .filter(|a| {
      let topics = a.list("topic_infos").iter().filter_map(|t| t.str("name"));
      [a.str("activity_name"), a.str("activity_reward")]
        .into_iter()
        .flatten()
        .chain(topics)
        .any(|s| s.to_lowercase().contains(&k))
    })
    .collect()
}

/// One activity by `#N` of the last list, its numeric activity id, its page
/// id, a detail link, or its exact name.
pub async fn find(c: &Client, arg: &str) -> Result<Value> {
  let arg = c.ctx.collection_ref(arg)?;
  let arg = arg.as_str();
  let page_id = arg
    .split(['?', '#'])
    .next()
    .and_then(|p| p.rsplit('/').next())
    .unwrap_or(arg);
  let hit = |a: &Value| {
    a.str("activity_id").as_deref() == Some(arg)
      || a.str("page_id").as_deref() == Some(page_id)
      || a.str("activity_name").as_deref() == Some(arg)
  };
  for kept in [false, true] {
    if let Some(a) = list(c, kept, false).await?.into_iter().find(hit) {
      return Ok(a);
    }
  }
  Err(Error::not_found(format!(
    "no activity `{arg}` in the activity center; `media xhs events` lists them"
  )))
}

/// An activity as a collection: its topics are what notes join it with.
pub fn collection(a: &Value) -> Collection {
  let mut c = Collection {
    id: a.str("activity_id").unwrap_or_default(),
    kind: "event".into(),
    name: a.str("activity_name").unwrap_or_default(),
    description: a.str("activity_reward").map(|r| html_to_text(&r)),
    url: a.str("activity_link"),
    raw: Some(a.clone()),
    ..Collection::default()
  };
  let topics: Vec<String> = a
    .list("topic_infos")
    .iter()
    .filter_map(|t| t.str("name"))
    .collect();
  let time = |k: &str| a.i64(k).and_then(from_millis).map(|t| json!(t.to_string()));
  for (key, value) in [
    ("page_id", a.str("page_id").map(Value::from)),
    ("start", time("start_time")),
    ("end", time("end_time")),
    ("topics", Some(json!(topics))),
    ("status", a.i64("activity_status").map(Value::from)),
    ("kept", Some(json!(a.i64("focus_status") == Some(1)))),
    ("post_link", a.str("pc_post_link").map(Value::from)),
  ] {
    if let Some(v) = value {
      c.extra.insert(key.into(), v);
    }
  }
  c
}

/// One activity with every field shown (a table would hide the period).
pub fn detail(a: &Value) -> Value {
  let mut c = collection(a);
  c.raw = None;
  json!(c)
}

pub fn page(rows: &[Value]) -> Page<Collection> {
  Page::last(rows.iter().map(collection).collect())
}

/// Keep (收藏) an activity, or drop it with `undo`.
pub async fn keep(c: &Client, arg: &str, undo: bool) -> Result<Action> {
  let a = find(c, arg).await?;
  let page_id = a
    .str("page_id")
    .ok_or_else(|| Error::upstream("the activity has no page id"))?;
  let body = json!({ "type": if undo { "2" } else { "1" }, "target_id": page_id });
  c.creator_post(KEEP, &body).await?;
  let name = a.str("activity_name").unwrap_or_default();
  Ok(Action::done(if undo { "unkeep" } else { "keep" }, name).with_id(page_id))
}

/// Publish a note that joins an activity.
pub async fn join(c: &Client, arg: &str, draft: &Draft) -> Result<Action> {
  let a = find(c, arg).await?;
  let event = association(&a)?;
  let action = creator::publish_note(c, draft, Some(&event)).await?;
  Ok(action.with_message(format!("joined {}", event.name)))
}

/// What a note joining an activity carries, as the publish page builds it
/// from the activity's `pc_post_link`: the activity's topics, the activity
/// center as the note's source, and an `ACTIVITY_COMPONENT` relation for the
/// business binds (one activity per note).
pub struct Association {
  pub topics: Vec<Value>,
  pub relation: Value,
  pub name: String,
}

impl Association {
  /// The note's `source` field.
  pub fn source(&self) -> String {
    let extra = json!({"subType": "web_activity_center", "ad_channel": "ditto", "systemId": "web"});
    json!({"type": "web", "ids": "", "extraInfo": extra.to_string()}).to_string()
  }
}

fn association(a: &Value) -> Result<Association> {
  let id = a
    .str("activity_id")
    .ok_or_else(|| Error::upstream("the activity has no id"))?;
  let name = a.str("activity_name").unwrap_or_default();
  let info = json!({
    "id": id,
    "name": name,
    "start_time": a.i64("start_time"),
    "end_time": a.i64("end_time"),
  });
  let relation = json!({
    "type": "ACTIVITY_COMPONENT",
    "relationList": [{
      "bizType": "ACTIVITY_COMPONENT",
      "bizId": id,
      "extraInfo": info.to_string(),
    }],
  });
  let topics = a
    .list("topic_infos")
    .iter()
    .filter(|t| t.str("id").is_some() && t.str("name").is_some())
    .map(|t| json!({ "id": t.str("id"), "name": t.str("name"), "link": t.str("link"), "type": "topic" }))
    .collect();
  Ok(Association {
    topics,
    relation,
    name,
  })
}
