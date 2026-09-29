//! Image posts. Each file gets an upload lease (`/api/media/asset.json`), is
//! posted to Reddit's media bucket, and is then submitted as an image post
//! (one file) or a gallery (several).

use std::path::PathBuf;

use media_core::file::Image;
use media_core::http::Part;
use media_core::{Action, Error, Result, Value, ValueExt, json};

use crate::api::Api;
use crate::write::submitted;

/// Most items a gallery takes.
const MAX_IMAGES: usize = 20;

struct Asset {
  id: String,
  /// Where the uploaded file lives, for single-image posts.
  url: String,
}

pub async fn submit(api: &Api, sr: &str, title: &str, images: &[PathBuf]) -> Result<Action> {
  if images.len() > MAX_IMAGES {
    return Err(Error::input(format!(
      "a Reddit gallery takes at most {MAX_IMAGES} images"
    )));
  }
  let mut assets = Vec::new();
  for path in images {
    assets.push(upload(api, path).await?);
  }
  if let [asset] = assets.as_slice() {
    let form = vec![
      ("sr", sr.to_owned()),
      ("title", title.to_owned()),
      ("kind", "image".into()),
      ("url", asset.url.clone()),
      ("resubmit", "true".into()),
      ("sendreplies", "true".into()),
    ];
    let v = api.post("/api/submit", form).await?;
    // Image posts are created asynchronously (the web app waits on a
    // websocket); the answer only points at the profile.
    let mut action = Action::done("publish", format!("r/{sr}"))
      .with_message("Reddit is processing the image; the post appears shortly");
    if let Some(page) = v.str("json.data.user_submitted_page") {
      action = action.with_url(page);
    }
    return Ok(action);
  }
  let items: Vec<Value> = assets
    .iter()
    .map(|a| json!({ "media_id": a.id, "caption": "", "outbound_url": "" }))
    .collect();
  let body = json!({
    "api_type": "json",
    "sr": sr,
    "title": title,
    "items": items,
    "sendreplies": true,
    "nsfw": false,
    "spoiler": false,
    "show_error_list": true,
  });
  let v = api.post_json("/api/submit_gallery_post.json", body).await?;
  Ok(submitted(&v, sr))
}

async fn upload(api: &Api, path: &std::path::Path) -> Result<Asset> {
  let image = Image::read(path).await?;
  let form = vec![
    ("filepath", image.name.clone()),
    ("mimetype", image.mime.to_owned()),
  ];
  let lease = api.post("/api/media/asset.json", form).await?;
  let fields = lease.list("args.fields");
  let (Some(action), Some(id)) = (lease.str("args.action"), lease.str("asset.asset_id")) else {
    return Err(Error::upstream("Reddit did not grant an upload slot"));
  };
  let action = match action.strip_prefix("//") {
    Some(rest) => format!("https://{rest}"),
    None => action,
  };
  let mut parts: Vec<Part> = fields
    .iter()
    .filter_map(|f| {
      Some(Part::text(
        &f.str("name")?,
        f.str("value").unwrap_or_default(),
      ))
    })
    .collect();
  let key = fields
    .iter()
    .find(|f| f.str("name").as_deref() == Some("key"))
    .and_then(|f| f.str("value"))
    .unwrap_or_default();
  // The bucket wants the file after all policy fields.
  parts.push(Part::file("file", image.data, &image.name, image.mime));
  let resp = api
    .ctx
    .http
    .post(&action)
    .no_cookies()
    .multipart(parts)
    .send()
    .await?;
  if !resp.status.is_success() {
    return Err(Error::upstream(format!(
      "uploading {} failed (HTTP {})",
      path.display(),
      resp.status
    )));
  }
  Ok(Asset {
    id,
    url: format!("{action}/{key}"),
  })
}
