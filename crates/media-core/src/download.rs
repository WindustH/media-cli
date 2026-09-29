//! Download the media of a post: images, videos (merging split audio/video
//! with ffmpeg), audio-only extraction and ASR-ready WAV segments.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::ctx::Ctx;
use crate::error::{Error, Result};
use crate::model::{Downloaded, Media, MediaKind, Post};
use crate::output::note;
use crate::text::{file_stem, one_line};

#[derive(Debug, Clone)]
pub struct DownloadOpts {
  pub dir: PathBuf,
  /// Keep only the audio track of videos.
  pub audio_only: bool,
  /// Split audio into WAV segments of this many seconds (16 kHz mono).
  pub split: Option<u32>,
}

pub async fn download(
  ctx: &Ctx,
  referer: &str,
  post: &Post,
  media: &[Media],
  opts: &DownloadOpts,
) -> Result<Vec<Downloaded>> {
  let wanted: Vec<&Media> = media
    .iter()
    .filter(|m| !opts.audio_only || matches!(m.kind, MediaKind::Video | MediaKind::Audio))
    .collect();
  if wanted.is_empty() {
    return Err(Error::not_found("this post has no downloadable media"));
  }
  tokio::fs::create_dir_all(&opts.dir).await?;
  let stem = base_name(post);
  let mut saved = Vec::new();
  for (i, m) in wanted.iter().enumerate() {
    let name = if wanted.len() > 1 {
      format!("{stem}-{}", i + 1)
    } else {
      stem.clone()
    };
    let path = if opts.audio_only {
      audio(ctx, referer, m, &opts.dir, &name).await?
    } else {
      full(ctx, referer, m, &opts.dir, &name).await?
    };
    let kind = if opts.audio_only {
      MediaKind::Audio
    } else {
      m.kind
    };
    if let (Some(secs), true) = (
      opts.split,
      matches!(kind, MediaKind::Audio | MediaKind::Video),
    ) {
      for seg in split(&path, &opts.dir.join(&name), secs).await? {
        saved.push(downloaded(MediaKind::Audio, &seg).await);
      }
    }
    saved.push(downloaded(kind, &path).await);
  }
  Ok(saved)
}

fn base_name(post: &Post) -> String {
  let label = post
    .title
    .as_deref()
    .or(post.text.as_deref())
    .map(one_line)
    .unwrap_or_default();
  let label = file_stem(&label);
  if label.is_empty() {
    file_stem(&post.id)
  } else {
    format!("{label} [{}]", file_stem(&post.id))
  }
}

async fn downloaded(kind: MediaKind, path: &Path) -> Downloaded {
  let bytes = tokio::fs::metadata(path)
    .await
    .map(|m| m.len())
    .unwrap_or(0);
  Downloaded {
    kind,
    path: path.display().to_string(),
    bytes,
  }
}

fn extension(url: &str, kind: MediaKind) -> &'static str {
  let path = url.split(['?', '#']).next().unwrap_or(url);
  let last = path.rsplit('/').next().unwrap_or("");
  let ext = last
    .rsplit_once('.')
    .map(|(_, e)| e.to_ascii_lowercase())
    .unwrap_or_default();
  const KNOWN: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "heic", "avif", "mp4", "m4a", "mp3", "flv", "webm", "mov",
  ];
  match KNOWN.iter().find(|k| **k == ext) {
    Some(k) => k,
    None => match kind {
      MediaKind::Image => "jpg",
      MediaKind::Video | MediaKind::Gif => "mp4",
      MediaKind::Audio => "m4a",
    },
  }
}

/// Rename `path` when its magic bytes show another format than the extension
/// guessed from the URL (e.g. HEIC originals behind extension-less URLs).
async fn fix_extension(path: PathBuf) -> PathBuf {
  let mut head = [0u8; 16];
  let read = async {
    let mut f = tokio::fs::File::open(&path).await?;
    f.read(&mut head).await
  };
  let Ok(n) = read.await else { return path };
  let Some(ext) = crate::file::sniff(&head[..n]) else {
    return path;
  };
  let current = path.extension().and_then(|e| e.to_str()).unwrap_or("");
  if current.eq_ignore_ascii_case(ext) || (ext == "jpg" && current.eq_ignore_ascii_case("jpeg")) {
    return path;
  }
  let renamed = path.with_extension(ext);
  match tokio::fs::rename(&path, &renamed).await {
    Ok(()) => renamed,
    Err(_) => path,
  }
}

/// Bytes per ranged request: large files come as a series of these, each with
/// its own timeout and retries (some CDNs also throttle or refuse unranged
/// downloads of big files).
const CHUNK: u64 = 8 << 20;
const CHUNK_TIMEOUT: Duration = Duration::from_secs(120);

/// How to fetch one file: its size and how its host takes byte ranges.
#[derive(Clone, Copy)]
struct Source<'a> {
  url: &'a str,
  size: Option<u64>,
  range_param: Option<&'a str>,
}

impl<'a> Source<'a> {
  fn video(m: &'a Media) -> Self {
    Self {
      url: &m.url,
      size: m.size,
      range_param: m.range_param.as_deref(),
    }
  }

  fn audio(m: &'a Media) -> Option<Self> {
    let url = m.audio_url.as_deref()?;
    Some(Self {
      url,
      size: m.audio_size,
      range_param: m.range_param.as_deref(),
    })
  }
}

async fn fetch(ctx: &Ctx, referer: &str, src: Source<'_>, path: &Path) -> Result<u64> {
  let label = path
    .file_name()
    .map(|f| f.to_string_lossy().into_owned())
    .unwrap_or_default();
  let bar = ProgressBar::new(0);
  bar.set_style(
    ProgressStyle::with_template(
      "{msg:40!} {bytes:>10}/{total_bytes:10} {bar:24} {bytes_per_sec:>10}",
    )
    .unwrap_or_else(|_| ProgressStyle::default_bar()),
  );
  bar.set_message(label);
  let result = match (src.range_param, src.size) {
    (Some(param), Some(size)) => {
      fetch_by_param(ctx, referer, src.url, param, size, path, &bar).await
    }
    _ => fetch_ranged(ctx, referer, src.url, path, &bar).await,
  };
  bar.finish_and_clear();
  result
}

/// Chunks as `url&param=a-b`, for hosts (googlevideo) that take ranges in the
/// URL and answer a `Range` header with something else.
async fn fetch_by_param(
  ctx: &Ctx,
  referer: &str,
  url: &str,
  param: &str,
  size: u64,
  path: &Path,
  bar: &ProgressBar,
) -> Result<u64> {
  bar.set_length(size);
  let sep = if url.contains('?') { '&' } else { '?' };
  let mut file = tokio::fs::File::create(path).await?;
  let mut done = 0;
  while done < size {
    let to = (done + CHUNK).min(size) - 1;
    let got = ctx
      .http
      .get(format!("{url}{sep}{param}={done}-{to}"))
      .header("referer", referer)
      .header("accept", "*/*")
      .no_cookies()
      .no_throttle()
      .retries(2)
      .timeout(CHUNK_TIMEOUT)
      .append_to(&mut file, |n| bar.inc(n))
      .await?;
    if got.bytes == 0 {
      return Err(Error::network(format!(
        "the server sent nothing for bytes {done}-{to}"
      )));
    }
    done += got.bytes;
  }
  file.flush().await?;
  Ok(done)
}

/// Ask for the first chunk; a `206` with the file size continues chunk by
/// chunk, a `200` (ranges ignored) already carried the whole file.
async fn fetch_ranged(
  ctx: &Ctx,
  referer: &str,
  url: &str,
  path: &Path,
  bar: &ProgressBar,
) -> Result<u64> {
  let chunk = |from: u64, to: u64| {
    ctx
      .http
      .get(url)
      .header("referer", referer)
      .header("accept", "*/*")
      .header("range", format!("bytes={from}-{to}"))
      .no_cookies()
      .no_throttle()
      .retries(2)
      .timeout(CHUNK_TIMEOUT)
  };
  let mut file = tokio::fs::File::create(path).await?;
  let first = chunk(0, CHUNK - 1)
    .append_to(&mut file, |n| bar.inc(n))
    .await?;
  let mut done = first.bytes;
  if first.status == wreq::StatusCode::PARTIAL_CONTENT
    && let Some(total) = first.range_total()
  {
    bar.set_length(total);
    while done < total {
      let to = (done + CHUNK).min(total) - 1;
      let got = chunk(done, to).append_to(&mut file, |n| bar.inc(n)).await?;
      if got.bytes == 0 {
        return Err(Error::network(format!(
          "the server sent nothing for bytes {done}-{to}"
        )));
      }
      done += got.bytes;
    }
  }
  file.flush().await?;
  Ok(done)
}

async fn full(ctx: &Ctx, referer: &str, m: &Media, dir: &Path, name: &str) -> Result<PathBuf> {
  let out = dir.join(format!("{name}.{}", extension(&m.url, m.kind)));
  let Some(audio_src) = Source::audio(m) else {
    fetch(ctx, referer, Source::video(m), &out).await?;
    return Ok(fix_extension(out).await);
  };
  let out = dir.join(format!("{name}.mp4"));
  let video = dir.join(format!("{name}.video.m4s"));
  let audio = dir.join(format!("{name}.audio.m4s"));
  fetch(ctx, referer, Source::video(m), &video).await?;
  fetch(ctx, referer, audio_src, &audio).await?;
  if !has_ffmpeg().await {
    note("ffmpeg not found: kept the separate video and audio tracks");
    return Ok(video);
  }
  ffmpeg(&[
    "-i".as_ref(),
    video.as_os_str(),
    "-i".as_ref(),
    audio.as_os_str(),
    "-c".as_ref(),
    "copy".as_ref(),
    out.as_os_str(),
  ])
  .await?;
  let _ = tokio::fs::remove_file(&video).await;
  let _ = tokio::fs::remove_file(&audio).await;
  Ok(out)
}

async fn audio(ctx: &Ctx, referer: &str, m: &Media, dir: &Path, name: &str) -> Result<PathBuf> {
  let out = dir.join(format!("{name}.m4a"));
  if let Some(src) = Source::audio(m).or((m.kind == MediaKind::Audio).then(|| Source::video(m))) {
    fetch(ctx, referer, src, &out).await?;
    return Ok(out);
  }
  let video = dir.join(format!("{name}.source.{}", extension(&m.url, m.kind)));
  fetch(ctx, referer, Source::video(m), &video).await?;
  require_ffmpeg().await?;
  ffmpeg(&[
    "-i".as_ref(),
    video.as_os_str(),
    "-vn".as_ref(),
    "-c:a".as_ref(),
    "aac".as_ref(),
    out.as_os_str(),
  ])
  .await?;
  let _ = tokio::fs::remove_file(&video).await;
  Ok(out)
}

/// `input` -> `dir/seg_000.wav ...` (16 kHz mono PCM).
async fn split(input: &Path, dir: &Path, secs: u32) -> Result<Vec<PathBuf>> {
  require_ffmpeg().await?;
  tokio::fs::create_dir_all(dir).await?;
  let pattern = dir.join("seg_%03d.wav");
  let secs = secs.max(1).to_string();
  ffmpeg(&[
    "-i".as_ref(),
    input.as_os_str(),
    "-vn".as_ref(),
    "-ac".as_ref(),
    "1".as_ref(),
    "-ar".as_ref(),
    "16000".as_ref(),
    "-f".as_ref(),
    "segment".as_ref(),
    "-segment_time".as_ref(),
    secs.as_ref(),
    pattern.as_os_str(),
  ])
  .await?;
  let mut segments = Vec::new();
  let mut entries = tokio::fs::read_dir(dir).await?;
  while let Some(e) = entries.next_entry().await? {
    if e.file_name().to_string_lossy().starts_with("seg_") {
      segments.push(e.path());
    }
  }
  segments.sort();
  Ok(segments)
}

async fn has_ffmpeg() -> bool {
  tokio::process::Command::new("ffmpeg")
    .arg("-version")
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .await
    .is_ok_and(|s| s.success())
}

async fn require_ffmpeg() -> Result<()> {
  if has_ffmpeg().await {
    Ok(())
  } else {
    Err(
      Error::internal("ffmpeg is required for this operation")
        .with_hint("install ffmpeg and make sure it is on PATH"),
    )
  }
}

async fn ffmpeg(args: &[&std::ffi::OsStr]) -> Result<()> {
  let output = tokio::process::Command::new("ffmpeg")
    .args(["-hide_banner", "-loglevel", "error", "-y"])
    .args(args)
    .stdin(Stdio::null())
    .output()
    .await?;
  if output.status.success() {
    Ok(())
  } else {
    Err(Error::internal(format!(
      "ffmpeg failed: {}",
      String::from_utf8_lossy(&output.stderr).trim()
    )))
  }
}
