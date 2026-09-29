//! Subreddits: search, your subscriptions, community listings and details,
//! and (un)subscribing, which is also how Reddit follows a user's profile.

use media_core::{Action, Collection, Error, Page, PageReq, Query, Result};

use crate::api::Api;
use crate::refs::{self, Target, sub_url, user_url};
use crate::{listing, parse};

async fn communities(
  api: &Api,
  path: &str,
  query: Vec<(&str, String)>,
  req: &PageReq,
) -> Result<Page<Collection>> {
  listing::page(api, path, query, req, |t| {
    parse::data(t, "t5").and_then(parse::subreddit)
  })
  .await
}

pub async fn search(api: &Api, q: &Query, req: &PageReq) -> Result<Page<Collection>> {
  communities(
    api,
    "/subreddits/search",
    vec![("q", q.keyword.clone())],
    req,
  )
  .await
}

/// `popular`, `new` or `default` communities.
pub async fn listed(api: &Api, which: &str, req: &PageReq) -> Result<Page<Collection>> {
  communities(api, &format!("/subreddits/{which}"), Vec::new(), req).await
}

/// Communities (and followed profiles) of the logged-in account.
pub async fn subscriptions(
  api: &Api,
  user: Option<&str>,
  req: &PageReq,
) -> Result<Page<Collection>> {
  api.require_login()?;
  if let Some(user) = user
    && !refs::user(user)?.eq_ignore_ascii_case(&api.username().await?)
  {
    return Err(Error::input(
      "Reddit lists the subscriptions of the logged-in account only",
    ));
  }
  communities(api, "/subreddits/mine/subscriber", Vec::new(), req).await
}

pub async fn about(api: &Api, arg: &str) -> Result<Collection> {
  let name = refs::subreddit(arg)?;
  let v = api.get(&format!("/r/{name}/about"), &[]).await?;
  parse::data(&v, "t5")
    .and_then(parse::subreddit)
    .ok_or_else(|| Error::not_found(format!("no such subreddit: r/{name}")))
}

/// Subscribe to `r/name`, or follow `u/name` (its profile subreddit `u_name`).
pub async fn subscribe(api: &Api, arg: &str, undo: bool) -> Result<Action> {
  let (sr_name, url) = match refs::target(arg)? {
    Target::Subreddit(name) => (name.clone(), sub_url(&name)),
    Target::User(name) => (format!("u_{name}"), user_url(&name)),
  };
  let mut form = vec![
    ("action", if undo { "unsub" } else { "sub" }.into()),
    ("sr_name", sr_name.clone()),
  ];
  if !undo {
    form.push(("skip_initial_defaults", "true".into()));
  }
  api.post("/api/subscribe", form).await?;
  let name = if undo { "unfollow" } else { "follow" };
  Ok(Action::done(name, sr_name).with_url(url))
}
