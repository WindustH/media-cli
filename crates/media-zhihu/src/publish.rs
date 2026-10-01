//! Publishing: image upload (register → OSS → poll), pins (想法), questions and articles.

use std::path::{Path, PathBuf};
use std::time::Duration;

use md5::{Digest, Md5};
use media_core::http::Method;
use media_core::{Action, Ctx, Error, Result, Value, ValueExt, json};

use crate::api::{self, MOBILE, V4, WWW, ZHUANLAN};
use crate::markup;
use crate::refs::{answer_url, article_url, question_url};
use crate::sign;
use crate::write::prepare;

const OSS: &str = "https://zhihu-pics-upload.zhimg.com";

/// An uploaded image, as the editors embed it.
struct Image {
  src: String,
  original: String,
  watermark: String,
  watermark_src: String,
  width: usize,
  height: usize,
}

impl Image {
  fn html(&self) -> String {
    format!(
      r#"<img src="{}" data-caption="" data-size="normal" data-rawwidth="{}" data-rawheight="{}" data-watermark="{}" data-original-src="{}" data-watermark-src="{}" data-private-watermark-src=""/>"#,
      self.src, self.width, self.height, self.watermark, self.original, self.watermark_src
    )
  }
}

async fn upload(ctx: &Ctx, path: &Path, source: &str) -> Result<Image> {
  let file = media_core::file::Image::read(path).await?;
  let (width, height) = file.size();
  let data = file.data;
  let hash = format!("{:x}", Md5::digest(&data));
  let mut v = register(ctx, &hash, source).await?;
  match v.i64("upload_file.state") {
    Some(1) => {} // already known to Zhihu
    Some(2) => {
      put_object(ctx, &v, data, file.mime).await?;
      // The id handed out before the upload stays `init` forever; registering
      // again yields a new id (state 1) whose processing does complete.
      v = register(ctx, &hash, source).await?;
    }
    other => return Err(Error::upstream(format!("unexpected image state {other:?}"))),
  }
  let image_id = v
    .str("upload_file.image_id")
    .ok_or_else(|| Error::upstream("image registration returned no image_id"))?;
  let info = poll_image(ctx, &image_id).await?;
  let src = info.str("src").unwrap_or_default();
  Ok(Image {
    original: info.str("original_src").unwrap_or_else(|| src.clone()),
    src,
    watermark: info.str("watermark").unwrap_or_else(|| "watermark".into()),
    watermark_src: info.str("watermark_src").unwrap_or_default(),
    width,
    height,
  })
}

/// Register an image by content hash: its id, and an upload token when Zhihu lacks it.
async fn register(ctx: &Ctx, hash: &str, source: &str) -> Result<Value> {
  api::call(
    ctx,
    api::post(ctx, &format!("{MOBILE}/images"))
      .json(&json!({ "image_hash": hash, "source": source })),
  )
  .await
}

/// Upload the bytes to Aliyun OSS with the STS token from the registration.
async fn put_object(ctx: &Ctx, reg: &Value, data: Vec<u8>, content_type: &str) -> Result<()> {
  let key = reg
    .str("upload_file.object_key")
    .ok_or_else(|| Error::upstream("image registration returned no object_key"))?;
  let token = reg.at("upload_token");
  let field = |name: &str| {
    token
      .str(name)
      .ok_or_else(|| Error::upstream(format!("upload token lacks {name}")))
  };
  let (id, secret, sts) = (
    field("access_id")?,
    field("access_key")?,
    field("access_token")?,
  );
  let date = jiff::Timestamp::now()
    .strftime("%a, %d %b %Y %H:%M:%S GMT")
    .to_string();
  let auth = sign::oss_authorization(&id, &secret, &sts, content_type, &date, &key);
  let resp = ctx
    .http
    .request(Method::PUT, format!("{OSS}/{key}"))
    .header("date", &date)
    .header("x-oss-security-token", &sts)
    .header("authorization", auth)
    .bytes(data, content_type)
    .no_cookies()
    .retries(1)
    .send()
    .await?;
  resp.check().map(drop)
}

async fn poll_image(ctx: &Ctx, image_id: &str) -> Result<Value> {
  // Processing can take a minute or more; poll with a growing interval (~2 min in all).
  let mut status = String::new();
  for attempt in 0..30u64 {
    let v = api::call(ctx, api::get(ctx, &format!("{MOBILE}/images/{image_id}"))).await?;
    status = v.str("status").unwrap_or_default();
    match status.as_str() {
      "success" => return Ok(v),
      "fail" | "failed" | "error" => {
        return Err(Error::upstream(format!(
          "Zhihu could not process the image ({status})"
        )));
      }
      _ => tokio::time::sleep(Duration::from_secs((2 + attempt / 4).min(6))).await,
    }
  }
  Err(Error::upstream(format!(
    "image processing timed out (last status `{status}`)"
  )))
}

async fn upload_all(ctx: &Ctx, paths: &[PathBuf], source: &str) -> Result<Vec<Image>> {
  let mut images = Vec::with_capacity(paths.len());
  for path in paths {
    images.push(upload(ctx, path, source).await?);
  }
  Ok(images)
}

/// Plain text as HTML paragraphs, one per line.
fn paragraphs(text: &str) -> String {
  text
    .lines()
    .map(str::trim)
    .filter(|l| !l.is_empty())
    .map(|l| format!("<p>{l}</p>"))
    .collect()
}

fn images_html(images: &[Image]) -> String {
  images.iter().map(Image::html).collect()
}

/// A body as editor HTML with its length in characters: plain text as one
/// paragraph per line, or `markdown` with its images uploaded in place.
/// The flag says whether the HTML holds uploaded images.
async fn body_html(
  ctx: &Ctx,
  body: &str,
  markdown: bool,
  source: &str,
) -> Result<(String, usize, bool)> {
  if !markdown {
    return Ok((paragraphs(body), body.chars().count(), false));
  }
  let m = markup::render(body);
  if let Some(missing) = m.images.iter().find(|p| !p.is_file()) {
    return Err(Error::input(format!(
      "image not found: {}",
      missing.display()
    )));
  }
  let images = upload_all(ctx, &m.images, source).await?;
  let html: Vec<String> = images.iter().map(Image::html).collect();
  Ok((m.fill(&html), m.text_len, !images.is_empty()))
}

async fn content_draft(ctx: &Ctx, action: &str) -> Result<String> {
  let v = api::call(
    ctx,
    api::post(ctx, &format!("{V4}/content/drafts")).json(&json!({ "action": action })),
  )
  .await?;
  v.str("data.content_id")
    .ok_or_else(|| Error::upstream("draft created but no content_id returned"))
}

/// The unified editor endpoint; returns the id of the published content.
async fn content_publish(ctx: &Ctx, payload: &Value) -> Result<String> {
  let v = api::call(
    ctx,
    api::post(ctx, &format!("{V4}/content/publish")).json(payload),
  )
  .await?;
  if let Some(code) = v.i64("code").filter(|c| *c != 0) {
    let message = v
      .first_str(&["message", "toast_message"])
      .unwrap_or_else(|| format!("code {code}"));
    return Err(Error::upstream(format!("publish failed: {message}")));
  }
  let result: Value = v
    .str("data.result")
    .and_then(|r| serde_json::from_str(&r).ok())
    .unwrap_or(Value::Null);
  result
    .first_str(&["id", "data.id"])
    .or_else(|| v.first_str(&["data.id", "id"]))
    .ok_or_else(|| Error::upstream("published, but Zhihu returned no id"))
}

fn trace_id() -> String {
  let ms = jiff::Timestamp::now().as_millisecond();
  let b: [u8; 16] = rand::random();
  let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
  // A version-4 UUID shape, like the web editor sends.
  format!(
    "{ms},{}-{}-4{}-a{}-{}",
    &hex[..8],
    &hex[8..12],
    &hex[13..16],
    &hex[17..20],
    &hex[20..32]
  )
}

/// Publish a pin (想法) with an optional title and images.
pub async fn pin(ctx: &Ctx, title: &str, text: &str, images: &[PathBuf]) -> Result<Action> {
  prepare(ctx).await?;
  let images = upload_all(ctx, images, "pin").await?;
  let draft = content_draft(ctx, "pin").await?;
  let chars = text.chars().count();
  let mut data = json!({
    "publish": { "traceId": trace_id() },
    "commentsPermission": { "comment_permission": "all" },
    "extra_info": { "view_permission": "all", "publisher": "pc" },
    "draft": { "disabled": 1, "id": draft },
    "title": { "title": title },
    "hybrid": { "html": paragraphs(text) + &images_html(&images), "textLength": chars },
  });
  if !images.is_empty() {
    data["hybrid"]["textLength"] = (title.chars().count() + chars).into();
    let medias: Vec<Value> = images
      .iter()
      .map(|i| {
        json!({ "image": {
          "width": i.width, "height": i.height, "url": i.src, "originalUrl": i.original,
          "watermark": i.watermark, "watermarkUrl": i.watermark_src,
        }})
      })
      .collect();
    data["media"] = json!({ "medias": medias });
  }
  let id = content_publish(ctx, &json!({ "action": "pin", "data": data })).await?;
  Ok(
    Action::done("publish", "pin")
      .with_id(&id)
      .with_url(format!("{WWW}/pin/{id}")),
  )
}

/// Ask a question; `topics` are topic ids.
pub async fn question(
  ctx: &Ctx,
  title: &str,
  detail: &str,
  topics: &[String],
  images: &[PathBuf],
) -> Result<Action> {
  prepare(ctx).await?;
  let detail_html = paragraphs(detail);
  let id = if images.is_empty() {
    let v = api::call(
      ctx,
      api::post(ctx, &format!("{V4}/questions")).json(&json!({
        "title": title, "detail": detail_html, "topic_url_tokens": topics,
      })),
    )
    .await?;
    v.str("id")
      .ok_or_else(|| Error::upstream("question created, but Zhihu returned no id"))?
  } else {
    let images = upload_all(ctx, images, "question").await?;
    let data = json!({
      "title": { "title": title },
      "topic": { "topics": topics },
      "hybrid": { "html": detail_html + &images_html(&images), "textLength": detail.chars().count() },
      "extra_info": { "publisher": "pc" },
      "questionConfig": { "type": "0" },
      "draft": { "disabled": 1 },
    });
    content_publish(ctx, &json!({ "action": "question", "data": data })).await?
  };
  Ok(
    Action::done("ask", "question")
      .with_url(question_url(&id))
      .with_id(id),
  )
}

/// Publish a column article; `topics` are topic ids.
pub async fn article(
  ctx: &Ctx,
  title: &str,
  body: &str,
  markdown: bool,
  topics: &[String],
  images: &[PathBuf],
) -> Result<Action> {
  prepare(ctx).await?;
  let (html, text_len, inline_images) = body_html(ctx, body, markdown, "article").await?;
  let id = if images.is_empty() && !inline_images {
    let draft = api::call(
      ctx,
      api::post(ctx, &format!("{ZHUANLAN}/articles/drafts")).json(&json!({})),
    )
    .await?;
    let id = draft
      .str("id")
      .ok_or_else(|| Error::upstream("article draft created, but Zhihu returned no id"))?;
    let mut patch = json!({ "title": title, "content": html });
    if !topics.is_empty() {
      patch["topics"] = json!(topics);
    }
    let url = format!("{ZHUANLAN}/articles/{id}/draft");
    api::call(ctx, api::request(ctx, Method::PATCH, &url).json(&patch)).await?;
    let url = format!("{ZHUANLAN}/articles/{id}/publish");
    let body = json!({ "column": null, "commentPermission": "anyone" });
    let v = api::call(ctx, api::request(ctx, Method::PUT, &url).json(&body)).await?;
    v.str("id").unwrap_or(id)
  } else {
    let images = upload_all(ctx, images, "article").await?;
    let draft = content_draft(ctx, "article").await?;
    let data = json!({
      "title": { "title": title },
      "hybrid": { "html": html + &images_html(&images), "textLength": text_len },
      "extra_info": { "publisher": "pc" },
      "draft": { "disabled": 1, "id": draft },
      "commentsPermission": { "comment_permission": "anyone" },
    });
    content_publish(ctx, &json!({ "action": "article", "data": data })).await?
  };
  Ok(
    Action::done("publish", "article")
      .with_url(article_url(&id))
      .with_id(id),
  )
}

/// Answer a question through the editor's publish endpoint.
pub async fn answer(
  ctx: &Ctx,
  question: &str,
  body: &str,
  markdown: bool,
  images: &[PathBuf],
) -> Result<Action> {
  prepare(ctx).await?;
  let (html, text_len, _) = body_html(ctx, body, markdown, "answer").await?;
  let images = upload_all(ctx, images, "answer").await?;
  let business = json!({
    "reshipment_settings": "allowed", "comment_permission": "all",
    "reward_setting": { "can_reward": false }, "disclaimer_status": "close",
    "disclaimer_type": "none", "commercial_report_info": { "is_report": false },
    "commercial_zhitask_bind_info": null, "is_report": false,
    "table_of_contents_enabled": false, "thank_inviter_status": "close", "thank_inviter": "",
  });
  let data = json!({
    "publish": { "traceId": trace_id() },
    "hybridInfo": {},
    "draft": { "isPublished": false, "disabled": 1 },
    "extra_info": {
      "question_id": question, "publisher": "pc",
      "pc_business_params": business.to_string(),
    },
    "hybrid": { "html": html + &images_html(&images), "textLength": text_len },
    "reprint": { "reshipment_settings": "allowed" },
    "commentsPermission": { "comment_permission": "all" },
    "appreciate": { "can_reward": false },
    "publishSwitch": { "draft_type": "normal" },
    "creationStatement": { "disclaimer_status": "close", "disclaimer_type": "none" },
    "commercialReportInfo": { "isReport": 0 },
    "toFollower": {},
    "contentsTables": { "table_of_contents_enabled": false },
    "thanksInvitation": { "thank_inviter_status": "close", "thank_inviter": "" },
  });
  let id = content_publish(ctx, &json!({ "action": "answer", "data": data })).await?;
  Ok(
    Action::done("publish", "answer")
      .with_url(answer_url(&id, Some(question)))
      .with_id(id),
  )
}
