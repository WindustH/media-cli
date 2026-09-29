//! Streams to download, from the ANDROID_VR player: its formats carry plain
//! `url`s (no signature cipher, no `n` parameter to solve). What googlevideo
//! then serves was measured (2026-09):
//!
//! - without a range, a stream comes at about playback speed, and large ones
//!   are refused (403); `range=a-b` URL parameters come at full speed (a
//!   `Range` header gets an HLS playlist instead), so streams carry their size
//!   and `range_param`, and media-core fetches them in chunks;
//! - for some videos, clients without a proof-of-origin token get only the
//!   first megabyte (403 after it), so the end of each stream is probed.
//!
//! A video the Android client cannot play (age-restricted, members-only, a
//! bot check) or whose streams are refused falls back to `yt-dlp` when that is
//! on PATH, which runs YouTube's player code for signatures and tokens.

use std::process::Stdio;

use media_core::{Error, ErrorCode, Media, MediaKind, Post, Result, Value, ValueExt};

use crate::api::{self, Api};
use crate::{parse, refs, video};

/// googlevideo takes byte ranges as this URL parameter.
const RANGE_PARAM: &str = "range";

fn size(f: &Value) -> Option<u64> {
  f.u64("contentLength")
}

fn url(f: &Value) -> Option<String> {
  f.str("url")
}

fn mime(f: &Value) -> String {
  f.str("mimeType").unwrap_or_default()
}

/// AVC > VP9 > AV1: at equal resolution the most widely playable codec.
fn codec(f: &Value) -> u8 {
  let m = mime(f);
  if m.contains("avc1") {
    3
  } else if m.contains("vp9") || m.contains("vp09") {
    2
  } else {
    1
  }
}

/// Best video (highest resolution, then codec) with the best audio (original
/// language track, AAC before Opus) as a DASH pair, or the audio alone.
fn streams(v: &Value, audio_only: bool) -> Result<Vec<Media>> {
  let duration = v.f64("videoDetails.lengthSeconds");
  let adaptive: Vec<&Value> = v
    .list("streamingData.adaptiveFormats")
    .iter()
    .filter(|f| f.str("url").is_some())
    .collect();
  let mut audio: Vec<&Value> = adaptive
    .iter()
    .copied()
    .filter(|f| mime(f).starts_with("audio/"))
    .collect();
  audio.sort_by_key(|f| {
    std::cmp::Reverse((
      f.bool("audioTrack.audioIsDefault").unwrap_or(true),
      mime(f).starts_with("audio/mp4"),
      f.u64("bitrate").unwrap_or(0),
    ))
  });
  let mut videos: Vec<&Value> = adaptive
    .iter()
    .copied()
    .filter(|f| mime(f).starts_with("video/"))
    .collect();
  videos.sort_by_key(|f| {
    std::cmp::Reverse((
      f.u64("height").unwrap_or(0),
      codec(f),
      f.u64("bitrate").unwrap_or(0),
    ))
  });
  let none = || Error::new(ErrorCode::UpstreamError, "YouTube listed no direct streams");
  let a = audio.first().copied();
  if audio_only {
    let a = a.ok_or_else(none)?;
    return Ok(vec![Media {
      duration,
      size: size(a),
      range_param: Some(RANGE_PARAM.into()),
      ..Media::new(MediaKind::Audio, url(a).unwrap_or_default())
    }]);
  }
  let (f, a) = match (videos.first().copied(), a) {
    (Some(f), Some(a)) => (f, Some(a)),
    // No split streams: the best muxed (audio + video) format.
    _ => {
      let mut muxed: Vec<&Value> = v
        .list("streamingData.formats")
        .iter()
        .filter(|f| f.str("url").is_some())
        .collect();
      muxed.sort_by_key(|f| std::cmp::Reverse(f.u64("height").unwrap_or(0)));
      (muxed.first().copied().ok_or_else(none)?, None)
    }
  };
  Ok(vec![Media {
    audio_url: a.and_then(url),
    size: size(f),
    audio_size: a.and_then(size),
    range_param: Some(RANGE_PARAM.into()),
    width: f.u64("width").map(|w| w as u32),
    height: f.u64("height").map(|h| h as u32),
    duration,
    ..Media::video(url(f).unwrap_or_default())
  }])
}

pub async fn media(api: &Api, post: &str, audio_only: bool) -> Result<(Post, Vec<Media>)> {
  let id = refs::video(post)?;
  let v = video::mobile_player(api, &id).await?;
  let mut p = parse::full_video(&v, &Value::Null).unwrap_or_else(|| Post {
    id: id.clone(),
    kind: "video".into(),
    url: Some(refs::video_url(&id)),
    ..Post::default()
  });
  p.raw = None;
  if v.bool("videoDetails.isLive") == Some(true) {
    return Err(Error::unsupported(
      "download of a live stream (wait for the recording)",
    ));
  }
  let reason = match api::playable(&v).and_then(|_| streams(&v, audio_only)) {
    Err(e) if e.code == ErrorCode::UnsupportedOperation => return Err(e),
    Err(e) => e,
    Ok(media) if media.is_empty() => {
      Error::new(ErrorCode::UpstreamError, "YouTube listed no direct streams")
    }
    Ok(media) => {
      if !refused(api, &v, &media).await {
        return Ok((p, media));
      }
      Error::new(
        ErrorCode::VerificationRequired,
        "YouTube serves only the first megabyte of this video's streams to clients \
         without a proof-of-origin token",
      )
    }
  };
  tracing::debug!("android_vr streams: {}; trying yt-dlp", reason.message);
  match yt_dlp(&id, audio_only).await? {
    Some(media) => Ok((p, media)),
    None => Err(reason.with_hint(
      "install yt-dlp (with a JavaScript runtime): it produces the tokens and signatures \
       YouTube's web player uses",
    )),
  }
}

/// Whether googlevideo refuses the end of a stream. Without a proof-of-origin
/// token it serves some videos' streams only up to the first megabyte (403
/// after that), so the last kilobyte of each stream is asked for first.
async fn refused(api: &Api, v: &Value, media: &[Media]) -> bool {
  let formats = v
    .list("streamingData.adaptiveFormats")
    .iter()
    .chain(v.list("streamingData.formats"));
  let chosen: Vec<&str> = media
    .iter()
    .flat_map(|m| std::iter::once(m.url.as_str()).chain(m.audio_url.as_deref()))
    .collect();
  for f in formats {
    let (Some(u), Some(last)) = (url(f), size(f).map(|n| n.saturating_sub(1))) else {
      continue;
    };
    if last < 1 << 20 || !chosen.contains(&u.as_str()) {
      continue;
    }
    let resp = api
      .ctx
      .http
      .get(format!("{u}&{RANGE_PARAM}={}-{last}", last - 1023))
      .no_cookies()
      .no_throttle()
      .send()
      .await;
    if resp.is_ok_and(|r| r.status.as_u16() == 403) {
      return true;
    }
  }
  false
}

/// Stream URLs from `yt-dlp -J`; `None` when yt-dlp is not installed.
async fn yt_dlp(id: &str, audio_only: bool) -> Result<Option<Vec<Media>>> {
  // Plain https files only: the best formats are often HLS playlists.
  let format = if audio_only {
    "bestaudio[ext=m4a][protocol=https]/bestaudio[protocol=https]"
  } else {
    "bestvideo[protocol=https]+bestaudio[ext=m4a][protocol=https]/best[protocol=https]"
  };
  let run = tokio::process::Command::new("yt-dlp")
    .args(["--no-warnings", "--no-playlist", "-J", "-f", format])
    .arg(refs::video_url(id))
    .stdin(Stdio::null())
    .output()
    .await;
  let out = match run {
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    other => other?,
  };
  if !out.status.success() {
    let err = String::from_utf8_lossy(&out.stderr);
    return Err(Error::upstream(format!("yt-dlp failed: {}", err.trim())));
  }
  let v: Value = serde_json::from_slice(&out.stdout)?;
  let duration = v.f64("duration");
  let url = |f: &Value| f.str("url").unwrap_or_default();
  let google = |f: &Value| url(f).contains(".googlevideo.com/") && f.u64("filesize").is_some();
  let media = match v.list("requested_formats") {
    [video, audio] => Media {
      audio_url: Some(url(audio)).filter(|u| !u.is_empty()),
      width: video.u64("width").map(|w| w as u32),
      height: video.u64("height").map(|h| h as u32),
      duration,
      size: video.u64("filesize"),
      audio_size: audio.u64("filesize"),
      range_param: (google(video) && google(audio)).then(|| RANGE_PARAM.into()),
      ..Media::video(url(video))
    },
    _ => {
      let kind = if audio_only {
        MediaKind::Audio
      } else {
        MediaKind::Video
      };
      Media {
        duration,
        size: v.u64("filesize"),
        range_param: google(&v).then(|| RANGE_PARAM.into()),
        ..Media::new(kind, url(&v))
      }
    }
  };
  Ok((!media.url.is_empty()).then(|| vec![media]))
}
