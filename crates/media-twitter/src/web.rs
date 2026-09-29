//! Metadata taken from the live web client, cached in the store: the
//! transaction-id material (page + `ondemand.s`) and fresh GraphQL query ids
//! (scanned from `main.js`, completed by the community `placeholder.json`).

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use media_core::{Ctx, Error, Result, ValueExt};
use regex::Regex;

use crate::graphql::Op;
use crate::sign::{self, Material};

/// Serves the legacy web app (with the `ondemand.s` chunk map) with or without login;
/// the logged-out home page moved to a new app without it.
const APP_PAGE: &str = "https://x.com/i/jf/";
const PLACEHOLDER: &str = "https://raw.githubusercontent.com/fa0311/twitter-openapi/refs/heads/main/src/config/placeholder.json";

const MATERIAL_KEY: &str = "transaction";
const MATERIAL_TTL: Duration = Duration::from_secs(3 * 3600);
const IDS_KEY: &str = "query-ids";
const IDS_TTL: Duration = Duration::from_secs(24 * 3600);
/// Do not refetch the ids more often than this, even on repeated 404s.
const IDS_MIN_AGE: Duration = Duration::from_secs(3600);

#[derive(Default)]
pub struct Web {
  page: RefCell<Option<Rc<str>>>,
  /// `None` until first needed; `Some(None)` when it could not be derived.
  material: RefCell<Option<Option<Rc<Material>>>>,
  ids: RefCell<Option<Rc<BTreeMap<String, String>>>>,
  refreshed: Cell<bool>,
}

impl Web {
  /// Transaction-id material; `None` when the web client could not be read
  /// (requests then go out without the header, as the reference does).
  pub async fn material(&self, ctx: &Ctx) -> Option<Rc<Material>> {
    if let Some(m) = self.material.borrow().clone() {
      return m;
    }
    let m = match ctx.store.cache_get::<Material>(MATERIAL_KEY, MATERIAL_TTL) {
      Some(m) => Some(Rc::new(m)),
      None => self.fresh_material(ctx).await,
    };
    *self.material.borrow_mut() = Some(m.clone());
    m
  }

  async fn fresh_material(&self, ctx: &Ctx) -> Option<Rc<Material>> {
    let derived = async {
      let page = self.page(ctx).await?;
      let url =
        sign::ondemand_url(&page).ok_or_else(|| Error::upstream("no ondemand.s in the web app"))?;
      let script = fetch_text(ctx, &url).await?;
      Material::derive(&page, &script)
    };
    match derived.await {
      Ok(m) => {
        ctx.store.cache_put(MATERIAL_KEY, &m);
        Some(Rc::new(m))
      }
      Err(e) => {
        tracing::warn!("no x-client-transaction-id: {e}");
        None
      }
    }
  }

  /// Query id of an operation: a refreshed one when known, else the fallback.
  pub fn query_id(&self, ctx: &Ctx, op: &Op) -> String {
    let ids = self
      .ids
      .borrow_mut()
      .get_or_insert_with(|| Rc::new(ctx.store.cache_get(IDS_KEY, IDS_TTL).unwrap_or_default()))
      .clone();
    ids
      .get(op.name)
      .cloned()
      .unwrap_or_else(|| op.id.to_owned())
  }

  /// After a 404: refetch the query ids (unless they are recent) and the
  /// transaction material, once per run. Returns whether anything changed.
  pub async fn refresh(&self, ctx: &Ctx) -> bool {
    if self.refreshed.replace(true) {
      return false;
    }
    let mut changed = false;
    let recent = ctx
      .store
      .cache_get::<BTreeMap<String, String>>(IDS_KEY, IDS_MIN_AGE)
      .is_some();
    if !recent {
      let ids = self.fresh_ids(ctx).await;
      if !ids.is_empty() {
        ctx.store.cache_put(IDS_KEY, &ids);
        *self.ids.borrow_mut() = Some(Rc::new(ids));
        changed = true;
      }
    }
    if let Some(m) = self.fresh_material(ctx).await {
      *self.material.borrow_mut() = Some(Some(m));
      changed = true;
    }
    changed
  }

  /// Live ids: `main.js` of the web app first, `placeholder.json` for the rest.
  async fn fresh_ids(&self, ctx: &Ctx) -> BTreeMap<String, String> {
    let mut ids = BTreeMap::new();
    match self.scan_main(ctx).await {
      Ok(found) => ids.extend(found),
      Err(e) => tracing::debug!("scanning main.js failed: {e}"),
    }
    match fetch_text(ctx, PLACEHOLDER)
      .await
      .and_then(|t| Ok(serde_json::from_str::<serde_json::Value>(&t)?))
    {
      Ok(v) => {
        for (name, op) in v.as_object().into_iter().flatten() {
          if let Some(id) = op.str("queryId") {
            ids.entry(name.clone()).or_insert(id);
          }
        }
      }
      Err(e) => tracing::debug!("placeholder.json failed: {e}"),
    }
    tracing::debug!("refreshed {} query ids", ids.len());
    ids
  }

  async fn scan_main(&self, ctx: &Ctx) -> Result<BTreeMap<String, String>> {
    let page = self.page(ctx).await?;
    let script =
      Regex::new(r#"https://abs\.twimg\.com/responsive-web/client-web[^"']*/main\.[0-9a-f]+\.js"#)
        .expect("valid regex")
        .find(&page)
        .ok_or_else(|| Error::upstream("no main.js in the web app"))?
        .as_str()
        .to_owned();
    let js = fetch_text(ctx, &script).await?;
    let op = Regex::new(r#"queryId:\s*"([A-Za-z0-9_-]+)"[^}]{0,200}?operationName:\s*"([^"]+)""#)
      .expect("valid regex");
    let mut ids = BTreeMap::new();
    for c in op.captures_iter(&js) {
      ids
        .entry(c[2].to_owned())
        .or_insert_with(|| c[1].to_owned());
    }
    Ok(ids)
  }

  /// The web app page, fetched at most once per run.
  async fn page(&self, ctx: &Ctx) -> Result<Rc<str>> {
    if let Some(p) = self.page.borrow().clone() {
      return Ok(p);
    }
    let page: Rc<str> = fetch_text(ctx, APP_PAGE).await?.into();
    *self.page.borrow_mut() = Some(page.clone());
    Ok(page)
  }
}

async fn fetch_text(ctx: &Ctx, url: &str) -> Result<String> {
  let resp = ctx
    .http
    .get(url)
    .header("accept", "text/html,application/xhtml+xml,*/*;q=0.8")
    .no_cookies()
    .no_throttle()
    .send()
    .await?
    .check()?;
  Ok(resp.text())
}
