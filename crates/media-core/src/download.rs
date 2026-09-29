//! Download the media of a post: images, videos (merging split audio/video
//! with ffmpeg), audio-only extraction and ASR-ready WAV segments.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use indicatif::{ProgressBar, ProgressStyle};
use tokio::io::AsyncReadExt;

use crate::error::{Error, Result};
use crate::model::{Downloaded, Media, MediaKind, Post};
use crate::output::note;
use crate::platform::Ctx;
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
  let Some(ext) = sniff(&head[..n]) else {
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

fn sniff(head: &[u8]) -> Option<&'static str> {
  Some(match head {
    [0xFF, 0xD8, 0xFF, ..] => "jpg",
    [0x89, b'P', b'N', b'G', ..] => "png",
    [b'G', b'I', b'F', b'8', ..] => "gif",
    [
      b'R',
      b'I',
      b'F',
      b'F',
      _,
      _,
      _,
      _,
      b'W',
      b'E',
      b'B',
      b'P',
      ..,
    ] => "webp",
    [0x1A, 0x45, 0xDF, 0xA3, ..] => "webm",
    [b'F', b'L', b'V', ..] => "flv",
    [_, _, _, _, b'f', b't', b'y', b'p', b0, b1, b2, b3, ..] => match &[*b0, *b1, *b2, *b3] {
      b"heic" | b"heix" | b"heim" | b"heis" | b"mif1" | b"msf1" => "heic",
      b"avif" | b"avis" => "avif",
      b"M4A " => "m4a",
      b"qt  " => "mov",
      _ => "mp4",
    },
    _ => return None,
  })
}

async fn fetch(ctx: &Ctx, referer: &str, url: &str, path: &Path) -> Result<u64> {
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
  let result = ctx
    .http
    .get(url)
    .header("referer", referer)
    .header("accept", "*/*")
    .no_cookies()
    .no_throttle()
    .retries(2)
    .save_to(path, |done, total| {
      if let Some(t) = total {
        bar.set_length(t);
      }
      bar.set_position(done);
    })
    .await;
  bar.finish_and_clear();
  result
}

async fn full(ctx: &Ctx, referer: &str, m: &Media, dir: &Path, name: &str) -> Result<PathBuf> {
  let out = dir.join(format!("{name}.{}", extension(&m.url, m.kind)));
  let Some(audio_url) = &m.audio_url else {
    fetch(ctx, referer, &m.url, &out).await?;
    return Ok(fix_extension(out).await);
  };
  let out = dir.join(format!("{name}.mp4"));
  let video = dir.join(format!("{name}.video.m4s"));
  let audio = dir.join(format!("{name}.audio.m4s"));
  fetch(ctx, referer, &m.url, &video).await?;
  fetch(ctx, referer, audio_url, &audio).await?;
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
  if let Some(url) = m
    .audio_url
    .as_deref()
    .or((m.kind == MediaKind::Audio).then_some(m.url.as_str()))
  {
    fetch(ctx, referer, url, &out).await?;
    return Ok(out);
  }
  let video = dir.join(format!("{name}.source.{}", extension(&m.url, m.kind)));
  fetch(ctx, referer, &m.url, &video).await?;
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
