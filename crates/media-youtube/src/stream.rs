//! Streams to download, from the ANDROID_VR player: its formats carry plain
//! `url`s (no signature cipher, no `n` parameter to solve). What googlevideo
//! then serves was measured (2026-09):
//!
//! - without a range, a stream comes at about playback speed, and large ones
//!   are refused (403); a `range=` of up to 20 MB comes at full speed. The
//!   shared downloader fetches each file in one request, so streams get a
//!   `range` covering the whole file and, when the best one is larger, the
//!   best one within that limit is taken, with a note (chunked downloads need
//!   media-core support);
//! - for some videos, clients without a proof-of-origin token get only the
//!   first megabyte (403 after it), so the end of each stream is probed.
//!
//! A video the Android client cannot play (age-restricted, members-only, a
//! bot check) or whose streams are refused falls back to `yt-dlp` when that is
//! on PATH, which runs YouTube's player code for signatures and tokens.

use std::process::Stdio;

use media_core::output::note;
use media_core::text::fmt_count;
use media_core::{Error, ErrorCode, Media, MediaKind, Post, Result, Value, ValueExt};

use crate::api::{self, Api};
use crate::{parse, refs, video};

/// Largest stream fetched in one request.
const ONE_REQUEST: u64 = 20 << 20;

fn size(f: &Value) -> Option<u64> {
  f.u64("contentLength")
}

fn fits(f: &Value) -> bool {
  size(f).is_some_and(|n| n <= ONE_REQUEST)
}

/// The stream URL asking for the whole file as one range.
fn ranged(f: &Value) -> Option<String> {
  let url = f.str("url")?;
  Some(match size(f).filter(|n| *n > 0) {
    Some(n) => format!("{url}&range=0-{}", n - 1),
    None => url,
  })
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

/// The first format that fits one request, noting when better ones did not.
fn first_fitting<'a>(ranked: &[&'a Value], what: &str) -> Option<&'a Value> {
  let best = *ranked.first()?;
  let pick = ranked.iter().copied().find(|f| fits(f))?;
  if !std::ptr::eq(pick, best) {
    let label = |f: &Value| {
      let size = fmt_count(size(f).unwrap_or(0));
      match f.u64("height") {
        Some(h) => format!("{h}p ({size}B)"),
        None => format!("{} kbps ({size}B)", f.u64("bitrate").unwrap_or(0) / 1000),
      }
    };
    note(&format!(
      "{what}: took {} instead of {}; larger YouTube streams need chunked downloads",
      label(pick),
      label(best)
    ));
  }
  Some(pick)
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
  let too_big = || {
    Error::new(
      ErrorCode::UnsupportedOperation,
      "every stream of this video is larger than YouTube sends in one request (20 MB)",
    )
    .with_hint("media-cli's downloader does not fetch in chunks yet; yt-dlp does")
  };
  let a = first_fitting(&audio, "audio");
  if audio_only {
    let a = a.ok_or_else(too_big)?;
    return Ok(vec![Media {
      duration,
      ..Media::new(MediaKind::Audio, ranged(a).unwrap_or_default())
    }]);
  }
  let (f, a) = match (first_fitting(&videos, "video"), a) {
    (Some(f), Some(a)) => (f, Some(a)),
    // No split streams: the best muxed (audio + video) format.
    _ => {
      let mut muxed: Vec<&Value> = v
        .list("streamingData.formats")
        .iter()
        .filter(|f| f.str("url").is_some())
        .collect();
      muxed.sort_by_key(|f| std::cmp::Reverse(f.u64("height").unwrap_or(0)));
      (first_fitting(&muxed, "video").ok_or_else(too_big)?, None)
    }
  };
  Ok(vec![Media {
    audio_url: a.and_then(ranged),
    width: f.u64("width").map(|w| w as u32),
    height: f.u64("height").map(|h| h as u32),
    duration,
    ..Media::video(ranged(f).unwrap_or_default())
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
      if !refused(api, &media).await {
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
async fn refused(api: &Api, media: &[Media]) -> bool {
  let urls = media
    .iter()
    .flat_map(|m| std::iter::once(&m.url).chain(m.audio_url.as_ref()));
  for url in urls {
    // `…&range=0-<last byte>` as [`ranged`] wrote it.
    let Some((base, last)) = url
      .rsplit_once("&range=0-")
      .and_then(|(b, l)| Some((b, l.parse::<u64>().ok()?)))
    else {
      continue;
    };
    if last < 1 << 20 {
      continue;
    }
    let probe = format!("{base}&range={}-{last}", last - 1023);
    let resp = api
      .ctx
      .http
      .get(probe)
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
  let format = if audio_only {
    "bestaudio[ext=m4a][filesize<20M]/bestaudio[filesize<20M]/bestaudio"
  } else {
    "bestvideo[filesize<20M]+bestaudio[ext=m4a][filesize<20M]/best[filesize<20M]/best"
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
  // googlevideo URLs get the same whole-file range as ours.
  let url = |f: &Value| {
    let u = f.str("url").unwrap_or_default();
    match f.u64("filesize").filter(|n| (1..=ONE_REQUEST).contains(n)) {
      Some(n) if u.contains(".googlevideo.com/") => format!("{u}&range=0-{}", n - 1),
      _ => u,
    }
  };
  let media = match v.list("requested_formats") {
    [video, audio] => Media {
      audio_url: Some(url(audio)).filter(|u| !u.is_empty()),
      width: video.u64("width").map(|w| w as u32),
      height: video.u64("height").map(|h| h as u32),
      duration,
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
        ..Media::new(kind, url(&v))
      }
    }
  };
  Ok((!media.url.is_empty()).then(|| vec![media]))
}
