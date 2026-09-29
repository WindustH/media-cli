//! Reading content: hot list, recommendations, search, posts, answers and topics.

use media_core::{
  Collection, Ctx, Error, Page, PageReq, Post, Query, Result, User, Value, ValueExt,
};

use crate::api::{self, MOBILE, V3, V4, WWW, ZHUANLAN};
use crate::parse;
use crate::refs::Target;

const ANSWER_INCLUDE: &str = "content,excerpt,voteup_count,comment_count,thanks_count,favlists_count,created_time,updated_time,author,question";

/// The hot list (热榜). The web endpoint needs a login; the mobile one serves the same list anonymously.
pub async fn hot(ctx: &Ctx) -> Result<Page<Post>> {
  let req = if ctx.http.has_cookie("z_c0") {
    api::get(ctx, &format!("{V3}/feed/topstory/hot-lists/total")).query("desktop", "true")
  } else {
    api::get(ctx, &format!("{MOBILE}/topstory/hot-lists/total"))
  };
  let v = api::call_public(ctx, req.query("limit", 50)).await?;
  Ok(Page::last(
    v.list("data").iter().filter_map(parse::hot_item).collect(),
  ))
}

/// The recommendation feed (推荐); the cursor is the query of `paging.next`.
pub async fn feed(ctx: &Ctx, page: &PageReq) -> Result<Page<Post>> {
  let base = format!("{V3}/feed/topstory/recommend");
  let req = match &page.cursor {
    Some(query) => api::get(ctx, &format!("{base}?{query}")),
    None => api::get(ctx, &base).queries([
      ("desktop", "true".to_owned()),
      ("action", "down".to_owned()),
      ("page_number", "1".to_owned()),
      ("limit", page.size_within(10).to_string()),
    ]),
  };
  let v = api::call(ctx, req).await?;
  let items = v
    .list("data")
    .iter()
    .filter_map(|item| parse::post(item.at("target")))
    .collect();
  Ok(Page::new(items, api::next_query(&v)))
}

pub async fn read(ctx: &Ctx, target: &Target) -> Result<Post> {
  let (url, include, parse): (String, Option<&str>, fn(&Value) -> Post) = match target {
    Target::Question(id) => return question(ctx, id).await,
    Target::Answer(id) => (
      format!("{V4}/answers/{id}"),
      Some(ANSWER_INCLUDE),
      parse::answer,
    ),
    Target::Article(id) => (format!("{ZHUANLAN}/articles/{id}"), None, parse::article),
    Target::Pin(id) => (format!("{V4}/pins/{id}"), None, parse::pin),
  };
  let mut req = api::get(ctx, &url);
  if let Some(include) = include {
    req = req.query("include", include);
  }
  Ok(parse(&api::call(ctx, req).await?))
}

/// A question, from its page: the v4 endpoint demands `x-zse-96`.
async fn question(ctx: &Ctx, id: &str) -> Result<Post> {
  let state = api::page_state(ctx, &format!("{WWW}/question/{id}")).await?;
  let question = state.at(&format!("entities.questions.{id}"));
  if question.is_null() {
    return Err(Error::not_found(format!("no question {id}")));
  }
  Ok(parse::question(&parse::snake_keys(question)))
}

/// Answers of a question, sorted by `default` (votes) or `created`.
pub async fn answers(ctx: &Ctx, question: &str, sort: &str, page: &PageReq) -> Result<Page<Post>> {
  let offset = page.number_or(0);
  let v = api::call(
    ctx,
    api::get(ctx, &format!("{V4}/questions/{question}/answers")).queries([
      ("include", format!("data[*].{ANSWER_INCLUDE}")),
      ("offset", offset.to_string()),
      ("limit", page.size_within(20).to_string()),
      ("sort_by", sort.to_owned()),
    ]),
  )
  .await?;
  Ok(Page::new(
    v.list("data").iter().map(parse::answer).collect(),
    api::next_param(&v, "offset"),
  ))
}

// ── search ────────────────────────────────────────────────────────────

/// `search_v3` for one result type (`general`, `people`, `topic`).
async fn search_v3(
  ctx: &Ctx,
  kind: &str,
  q: &Query,
  page: &PageReq,
) -> Result<(Vec<Value>, Option<String>)> {
  let offset = page.number_or(0);
  let mut req = api::get(ctx, &format!("{V4}/search_v3")).queries([
    ("gk_version", "gz-gaokao".to_owned()),
    ("t", kind.to_owned()),
    ("q", q.keyword.clone()),
    ("correction", "1".to_owned()),
    ("offset", offset.to_string()),
    ("limit", page.size_within(20).to_string()),
    ("filter_fields", String::new()),
    ("lc_idx", offset.to_string()),
    ("show_all_topics", "0".to_owned()),
    ("search_source", "Normal".to_owned()),
  ]);
  match q.sort.as_deref() {
    Some("upvoted") => req = req.query("sort", "upvoted_count"),
    Some("newest") => req = req.query("sort", "created_time"),
    _ => {}
  }
  if let Some(vertical) = &q.filter {
    req = req.query("vertical", vertical);
  }
  let v = api::call(ctx, req).await?;
  let objects = v
    .list("data")
    .iter()
    .filter(|item| item.str("type").as_deref() == Some("search_result"))
    .map(|item| item.at("object").clone())
    .collect();
  Ok((objects, api::next_param(&v, "offset")))
}

pub async fn search(ctx: &Ctx, q: &Query, page: &PageReq) -> Result<Page<Post>> {
  let (objects, next) = search_v3(ctx, "general", q, page).await?;
  Ok(Page::new(
    objects.iter().filter_map(parse::post).collect(),
    next,
  ))
}

pub async fn search_users(ctx: &Ctx, q: &Query, page: &PageReq) -> Result<Page<User>> {
  let (objects, next) = search_v3(ctx, "people", q, page).await?;
  let users = objects
    .iter()
    .filter(|o| o.str("type").as_deref() == Some("people"))
    .map(parse::user)
    .collect();
  Ok(Page::new(users, next))
}

pub async fn search_topics(ctx: &Ctx, q: &Query, page: &PageReq) -> Result<Page<Collection>> {
  let (objects, next) = search_v3(ctx, "topic", q, page).await?;
  let topics = objects
    .iter()
    .filter(|o| o.str("type").as_deref() == Some("topic"))
    .map(parse::topic)
    .collect();
  Ok(Page::new(topics, next))
}

// ── topics ────────────────────────────────────────────────────────────

pub async fn topic(ctx: &Ctx, id: &str) -> Result<Collection> {
  let state = api::page_state(ctx, &format!("{WWW}/topic/{id}")).await?;
  let topic = state.at(&format!("entities.topics.{id}"));
  if topic.is_null() {
    return Err(Error::not_found(format!("no topic {id}")));
  }
  Ok(parse::topic(&parse::snake_keys(topic)))
}

/// The topic's essence feed (精华).
pub async fn topic_essence(ctx: &Ctx, id: &str, page: &PageReq) -> Result<Page<Post>> {
  let v = api::call(
    ctx,
    api::get(ctx, &format!("{V4}/topics/{id}/feeds/essence")).queries([
      ("offset", page.number_or(0)),
      ("limit", page.size_within(10) as u64),
    ]),
  )
  .await?;
  let items = v
    .list("data")
    .iter()
    .filter_map(|item| parse::post(item.at("target")).or_else(|| parse::post(item)))
    .collect();
  Ok(Page::new(items, api::next_param(&v, "offset")))
}
