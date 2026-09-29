//! Community posts (`backstagePostRenderer`): text, images, a shared video or a poll.

use media_core::{Media, Post, Value, ValueExt};

use super::{byline, count, image, put, text, when};
use crate::refs::post_url;

pub fn backstage(r: &Value) -> Option<Post> {
  let id = r.str("postId")?;
  let published = text(r.at("publishedTimeText"));
  let mut p = Post {
    kind: "post".into(),
    text: text(r.at("contentText")),
    url: Some(post_url(&id)),
    author: byline(&serde_json::json!({ "runs": [{
      "text": text(r.at("authorText")),
      "navigationEndpoint": r.at("authorEndpoint"),
    }]})),
    created_at: published.as_deref().and_then(when),
    id,
    ..Post::default()
  };
  p.metrics.likes = text(r.at("voteCount")).and_then(|t| count(&t)).or(Some(0));
  p.metrics.comments =
    text(r.at("actionButtons.commentActionButtonsRenderer.replyButton.buttonRenderer.text"))
      .and_then(|t| count(&t));
  let att = r.at("backstageAttachment");
  let images: Vec<&Value> = match att.list("postMultiImageRenderer.images") {
    [] => vec![att.at("backstageImageRenderer.image")],
    many => many
      .iter()
      .map(|i| i.at("backstageImageRenderer.image"))
      .collect(),
  };
  p.media = images
    .into_iter()
    .filter_map(image)
    .map(Media::image)
    .collect();
  if let Some(v) = att.get("videoRenderer") {
    p.quoted = super::items::video_renderer(v).map(Box::new);
  }
  let choices: Vec<String> = att
    .list("pollRenderer.choices")
    .iter()
    .filter_map(|c| text(c.at("text")))
    .collect();
  if !choices.is_empty() {
    put(&mut p.extra, "poll", choices);
    put(
      &mut p.extra,
      "poll_votes",
      text(att.at("pollRenderer.totalVotes")).and_then(|t| count(&t)),
    );
  }
  put(&mut p.extra, "published", published);
  p.raw = Some(r.clone());
  Some(p)
}
