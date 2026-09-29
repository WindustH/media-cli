//! Reddit-hosted videos (`v.redd.it`) are split DASH streams: the post
//! carries a video-only `fallback_url`, and the audio track is only listed in
//! the DASH manifest (`CMAF_AUDIO_128.mp4` today, `DASH_AUDIO_*.mp4` or
//! `DASH_audio.mp4` on older videos). This reads the manifest and adds the
//! best audio (and video) rendition to the post's media.

use std::sync::LazyLock;

use media_core::{Error, Post, Result, ValueExt};
use regex::Regex;
use url::Url;

use crate::api::Api;
use crate::parse;

static REPRESENTATION: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r#"(?s)<Representation\b([^>]*)>.*?<BaseURL>([^<]+)</BaseURL>"#).expect("valid regex")
});
static BANDWIDTH: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r#"\bbandwidth="(\d+)""#).expect("valid regex"));

/// Fill `audio_url` of the Reddit video of a post and of its crosspost parent.
pub async fn add_audio(api: &Api, post: &mut Post) -> Result<()> {
  attach(api, post).await?;
  if let Some(parent) = post.quoted.as_deref_mut() {
    attach(api, parent).await?;
  }
  Ok(())
}

async fn attach(api: &Api, post: &mut Post) -> Result<()> {
  let Some(v) = post.raw.as_ref().and_then(parse::reddit_video) else {
    return Ok(());
  };
  if v.bool("has_audio") == Some(false) || v.bool("is_gif") == Some(true) {
    return Ok(());
  }
  let Some(dash) = v.str("dash_url") else {
    return Ok(());
  };
  let (video, audio) = tracks(api, &dash).await?;
  if let Some(m) = post.media.first_mut() {
    if let Some(video) = video {
      m.url = video;
    }
    m.audio_url = audio;
  }
  Ok(())
}

/// Best video and audio renditions listed in a DASH manifest.
async fn tracks(api: &Api, dash: &str) -> Result<(Option<String>, Option<String>)> {
  let resp = api.ctx.http.get(dash).no_cookies().send().await?.check()?;
  let base = Url::parse(dash).map_err(|e| Error::upstream(format!("bad DASH url: {e}")))?;
  let (mut video, mut audio) = (None, None);
  // Each adaptation set holds one kind of stream.
  for set in resp.text().split("<AdaptationSet").skip(1) {
    let is_audio = set.contains(r#"contentType="audio""#) || set.contains(r#"mimeType="audio/"#);
    let best = REPRESENTATION
      .captures_iter(set)
      .filter_map(|c| {
        let bandwidth: u64 = BANDWIDTH.captures(&c[1])?[1].parse().ok()?;
        Some((bandwidth, base.join(c[2].trim()).ok()?.to_string()))
      })
      .max_by_key(|(bandwidth, _)| *bandwidth)
      .map(|(_, url)| url);
    let slot = if is_audio { &mut audio } else { &mut video };
    if slot.is_none() {
      *slot = best;
    }
  }
  Ok((video, audio))
}
