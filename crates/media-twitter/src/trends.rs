//! Trends of the Explore tabs (`GenericTimelineById`), shown as posts of
//! kind `trend` whose link is the matching search.

use media_core::{Page, PageReq, Post, Result, Value, ValueExt, json};

use crate::api::Api;
use crate::graphql::EXPLORE_TIMELINE;
use crate::timeline::{self, Timeline, vars, with};

/// Explore tab timeline ids (base64 of `Timeline:` + a thrift tab name).
fn timeline_id(category: Option<&str>) -> &'static str {
  match category {
    Some("for-you") => "VGltZWxpbmU6DAC2CwABAAAAB2Zvcl95b3UAAA",
    Some("news") => "VGltZWxpbmU6DAC2CwABAAAABG5ld3MAAA",
    Some("sports") => "VGltZWxpbmU6DAC2CwABAAAABnNwb3J0cwAA",
    Some("entertainment") => "VGltZWxpbmU6DAC2CwABAAAADWVudGVydGFpbm1lbnQAAA",
    _ => "VGltZWxpbmU6DAC2CwABAAAACHRyZW5kaW5nAAA",
  }
}

pub async fn hot(api: &Api, category: Option<&str>, req: &PageReq) -> Result<Page<Post>> {
  api.require_login()?;
  let variables = with(
    vars(req),
    json!({
      "timelineId": timeline_id(category),
      "withQuickPromoteEligibilityTweetFields": true,
    }),
  );
  let paths = &["data.timeline.timeline.instructions"];
  timeline::page(
    api,
    &EXPLORE_TIMELINE,
    variables,
    paths,
    req,
    |tl: &Timeline| tl.items().filter_map(|(_, item)| trend(item)).collect(),
  )
  .await
}

fn trend(item: &Value) -> Option<Post> {
  if item.str("__typename").as_deref() != Some("TimelineTrend") {
    return None;
  }
  let name = item.str("name")?;
  let context: Vec<String> = [
    "trend_metadata.domain_context",
    "trend_metadata.meta_description",
  ]
  .iter()
  .filter_map(|p| item.str(p))
  .collect();
  let search = url::Url::parse_with_params("https://x.com/search", &[("q", name.as_str())])
    .map(String::from)
    .ok();
  let mut post = Post {
    kind: "trend".into(),
    title: Some(name.clone()),
    text: (!context.is_empty()).then(|| context.join(" · ")),
    url: search,
    tags: item
      .list("grouped_trends")
      .iter()
      .filter_map(|g| g.str("name"))
      .collect(),
    raw: Some(item.clone()),
    id: name,
    ..Post::default()
  };
  if let Some(rank) = item.u64("rank") {
    post.extra.insert("rank".into(), rank.into());
  }
  Some(post)
}
