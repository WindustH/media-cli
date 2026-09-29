//! Transcripts from caption tracks. The tracks come from the ANDROID_VR
//! player (the web player's caption URLs need a proof-of-origin token and
//! answer empty); `fmt=json3` returns timed events, as youtube-transcript-api
//! fetches them.

use media_core::{Cue, Error, ErrorCode, Result, Transcript, Value, ValueExt};

use crate::api::{self, Api};
use crate::{refs, video};

/// Caption track to fetch: `lang` exactly, then by prefix (`en` → `en-US`),
/// human captions before automatic ones; otherwise a translation into `lang`.
fn pick<'a>(tracks: &'a [Value], lang: Option<&str>) -> Option<(&'a Value, Option<String>)> {
  let human = |t: &&Value| t.str("kind").as_deref() != Some("asr");
  let code = |t: &Value| t.str("languageCode").unwrap_or_default().to_lowercase();
  let Some(lang) = lang.map(str::to_lowercase) else {
    return tracks
      .iter()
      .find(human)
      .or(tracks.first())
      .map(|t| (t, None));
  };
  let base = lang.split('-').next().unwrap_or(&lang).to_owned();
  let matching = |pred: &dyn Fn(&Value) -> bool| {
    let found: Vec<&Value> = tracks.iter().filter(|t| pred(t)).collect();
    found.iter().copied().find(human).or(found.first().copied())
  };
  matching(&|t| code(t) == lang)
    .or_else(|| matching(&|t| code(t).split('-').next() == Some(&base)))
    .map(|t| (t, None))
    .or_else(|| {
      let translatable = |t: &&Value| t.bool("isTranslatable") == Some(true);
      let t = tracks
        .iter()
        .filter(translatable)
        .find(human)
        .or_else(|| tracks.iter().find(translatable))?;
      Some((t, Some(lang.clone())))
    })
}

/// `json3` events → cues; rolling automatic captions overlap, so each cue
/// ends where the next begins.
fn cues(v: &Value) -> Vec<Cue> {
  let mut out: Vec<Cue> = Vec::new();
  for e in v.list("events") {
    let text: String = e
      .list("segs")
      .iter()
      .filter_map(|s| s.str("utf8"))
      .collect();
    let text = text.trim();
    if text.is_empty() {
      continue;
    }
    let from = e.f64("tStartMs").unwrap_or(0.0) / 1000.0;
    let to = from + e.f64("dDurationMs").unwrap_or(0.0) / 1000.0;
    if let Some(prev) = out.last_mut() {
      prev.to = prev.to.min(from).max(prev.from);
    }
    out.push(Cue {
      from,
      to,
      text: media_core::text::one_line(text),
    });
  }
  out
}

/// The track's URL asking for `json3`, translated when `tlang` is given.
fn track_url(base: &str, tlang: Option<&str>) -> Result<String> {
  let mut url =
    url::Url::parse(base).map_err(|e| Error::upstream(format!("bad caption URL: {e}")))?;
  let pairs: Vec<(String, String)> = url
    .query_pairs()
    .filter(|(k, _)| k != "fmt")
    .map(|(k, v)| (k.into_owned(), v.into_owned()))
    .collect();
  {
    let mut q = url.query_pairs_mut();
    q.clear().extend_pairs(pairs).append_pair("fmt", "json3");
    if let Some(t) = tlang {
      q.append_pair("tlang", t);
    }
  }
  Ok(url.into())
}

pub async fn transcript(api: &Api, arg: &str, lang: Option<&str>) -> Result<Transcript> {
  let id = refs::video(arg)?;
  let v = video::mobile_player(api, &id).await?;
  api::playable(&v)?;
  let tracks = v.list("captions.playerCaptionsTracklistRenderer.captionTracks");
  if tracks.is_empty() {
    return Err(Error::not_found(format!("video {id} has no captions")));
  }
  let Some((track, translate)) = pick(tracks, lang) else {
    let have: Vec<String> = tracks
      .iter()
      .filter_map(|t| t.str("languageCode"))
      .collect();
    return Err(Error::not_found(format!(
      "no {} captions; available: {}",
      lang.unwrap_or_default(),
      have.join(", ")
    )));
  };
  let base = track
    .str("baseUrl")
    .ok_or_else(|| Error::upstream("caption track without a URL"))?;
  let url = track_url(&base, translate.as_deref())?;
  let resp = api.ctx.http.get(url).no_cookies().send().await?;
  if resp.status.as_u16() == 429 && translate.is_some() {
    return Err(
      Error::new(
        ErrorCode::RateLimited,
        "YouTube throttles machine-translated captions",
      )
      .with_hint("retry later, or omit --lang for the original track"),
    );
  }
  let body = resp.check()?.value()?;
  let mut lang = translate.unwrap_or_else(|| track.str("languageCode").unwrap_or_default());
  if track.str("kind").as_deref() == Some("asr") {
    lang.push_str(" (auto)");
  }
  Ok(Transcript {
    lang,
    cues: cues(&body),
  })
}
