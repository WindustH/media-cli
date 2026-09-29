//! What the player loads: streams (playurl), subtitles, danmaku and the AI summary.

use media_core::{
  Ctx, Cue, Error, Media, MediaKind, Post, Result, Transcript, Value, ValueExt, json,
};

use crate::proto::{self, Wire};
use crate::refs::Video;
use crate::{api, dynamic, parse, video};

const PLAYURL: &str = "https://api.bilibili.com/x/player/wbi/playurl";
const PLAYER: &str = "https://api.bilibili.com/x/player/wbi/v2";
const DANMAKU: &str = "https://api.bilibili.com/x/v2/dm/web/seg.so";
const SUMMARY: &str = "https://api.bilibili.com/x/web-interface/view/conclusion/get";

/// One part of a video: its post (titled with the part), `cid` and `view` data.
struct Part {
  post: Post,
  cid: u64,
  view: Value,
}

async fn part(ctx: &Ctx, v: &Video) -> Result<Part> {
  let view = video::view(ctx, v).await?;
  let cid = video::cid(&view, v.page)?;
  let mut post = parse::video(&view);
  if view.list("pages").len() > 1 {
    let name = view
      .str(&format!("pages.{}.part", v.page - 1))
      .unwrap_or_default();
    post.title = Some(format!(
      "{} P{} {name}",
      post.title.unwrap_or_default(),
      v.page
    ));
  }
  post.extra.insert("cid".into(), json!(cid));
  Ok(Part { post, cid, view })
}

/// Best DASH video + audio as one [`Media`] (or audio alone); `durl` MP4/FLV as fallback.
pub async fn media(ctx: &Ctx, v: &Video, audio_only: bool) -> Result<(Post, Vec<Media>)> {
  let Part { post, cid, .. } = part(ctx, v).await?;
  let data = api::get(ctx, PLAYURL)
    .arg("avid", v.aid)
    .arg("bvid", &v.bvid)
    .arg("cid", cid)
    .arg("qn", 127)
    .arg("fnval", 4048)
    .arg("fnver", 0)
    .arg("fourk", 1)
    .arg("from_client", "BROWSER")
    .arg("gaia_source", "pre-load")
    .arg("isGaiaAvoided", "true")
    .arg("web_location", 1315873)
    .wbi()
    .send()
    .await?;
  Ok((post, streams(&data, audio_only)?))
}

/// A dynamic's media: the video it announces, else its images.
pub async fn dynamic_media(ctx: &Ctx, id: &str, audio_only: bool) -> Result<(Post, Vec<Media>)> {
  let post = dynamic::read(ctx, id).await?;
  let video = post
    .extra
    .get("bvid")
    .and_then(|b| b.as_str())
    .and_then(Video::from_bvid);
  match video {
    Some(v) => media(ctx, &v, audio_only).await,
    None => {
      let images = post.media.clone();
      Ok((post, images))
    }
  }
}

fn stream_url(s: &Value) -> Option<String> {
  s.first_str(&["baseUrl", "base_url", "url"])
}

fn streams(data: &Value, audio_only: bool) -> Result<Vec<Media>> {
  let dash = data.at("dash");
  let duration = dash
    .f64("duration")
    .or_else(|| data.f64("timelength").map(|ms| ms / 1000.0));
  let audio = dash
    .list("audio")
    .iter()
    .max_by_key(|a| a.u64("bandwidth").unwrap_or(0))
    .and_then(stream_url);
  if audio_only && let Some(url) = &audio {
    return Ok(vec![Media {
      duration,
      ..Media::new(MediaKind::Audio, url)
    }]);
  }
  // Highest quality first; at equal quality prefer AVC > HEVC > AV1 (widest support).
  let codec = |s: &Value| match s.u64("codecid") {
    Some(7) => 3,
    Some(12) => 2,
    _ => 1,
  };
  let best = dash
    .list("video")
    .iter()
    .max_by_key(|s| (s.u64("id").unwrap_or(0), codec(s)));
  if let Some((s, url)) = best.and_then(|s| Some((s, stream_url(s)?))) {
    return Ok(vec![Media {
      audio_url: audio,
      width: s.u64("width").map(|w| w as u32),
      height: s.u64("height").map(|h| h as u32),
      duration,
      ..Media::video(url)
    }]);
  }
  match data.list("durl").first().and_then(stream_url) {
    Some(url) => Ok(vec![Media {
      duration,
      ..Media::video(url)
    }]),
    None => Err(
      Error::not_found("no playable stream for this video")
        .with_hint("it may be paid, region-locked or need a login"),
    ),
  }
}

/// A subtitle track; `lang` picks by code (`zh-CN`, `ai-zh`, `en` ...), Chinese first by default.
pub async fn subtitle(ctx: &Ctx, v: &Video, lang: Option<&str>) -> Result<Transcript> {
  let cid = part(ctx, v).await?.cid;
  let data = api::get(ctx, PLAYER)
    .arg("aid", v.aid)
    .arg("cid", cid)
    .arg("isGaiaAvoided", "false")
    .arg("web_location", 1315873)
    .wbi()
    .send()
    .await?;
  let tracks = data.list("subtitle.subtitles");
  let lan = |t: &Value| t.str("lan").unwrap_or_default();
  let track = match lang {
    Some(want) => tracks
      .iter()
      .find(|t| lan(t) == want)
      .or_else(|| tracks.iter().find(|t| lan(t).contains(want))),
    None => tracks
      .iter()
      .find(|t| lan(t).contains("zh"))
      .or(tracks.first()),
  };
  let Some(track) = track else {
    let available: Vec<String> = tracks.iter().map(lan).collect();
    let mut err = Error::not_found(if available.is_empty() {
      "no subtitles for this video".to_owned()
    } else {
      format!(
        "no `{}` subtitle; available: {}",
        lang.unwrap_or("zh"),
        available.join(", ")
      )
    });
    if !ctx.http.has_cookie("SESSDATA") {
      err =
        err.with_hint("most subtitles are only served to logged-in accounts: `media bili login`");
    }
    return Err(err);
  };
  let url = parse::https(track.str("subtitle_url").unwrap_or_default());
  let body = ctx.http.get(url).no_cookies().value().await?;
  let cues = body
    .list("body")
    .iter()
    .map(|c| Cue {
      from: c.f64("from").unwrap_or(0.0),
      to: c.f64("to").unwrap_or(0.0),
      text: c.str("content").unwrap_or_default(),
    })
    .collect();
  Ok(Transcript {
    lang: lan(track),
    cues,
  })
}

/// Danmaku (bullet comments) of the video part, ordered by time. They come
/// as protobuf in 6-minute segments (`DmSegMobileReply.elems`): every segment
/// up to the part's duration (whole seconds, so one more for the rest), or
/// until an empty one when the duration is unknown.
pub async fn danmaku(ctx: &Ctx, v: &Video) -> Result<Transcript> {
  let Part { cid, view, .. } = part(ctx, v).await?;
  let secs = view
    .u64(&format!("pages.{}.duration", v.page - 1))
    .or_else(|| view.u64("duration"))
    .unwrap_or(0);
  let segments = if secs > 0 { secs / 360 + 1 } else { 200 };
  let mut cues = Vec::new();
  for segment in 1..=segments {
    let before = cues.len();
    let resp = ctx
      .http
      .get(format!(
        "{DANMAKU}?type=1&oid={cid}&pid={}&segment_index={segment}",
        v.aid
      ))
      .send()
      .await?
      .check()?;
    cues.extend(
      proto::fields(&resp.body)
        .into_iter()
        .filter_map(|field| match field {
          (1, Wire::Bytes(elem)) => Some(cue(elem)),
          _ => None,
        }),
    );
    if secs == 0 && cues.len() == before {
      break;
    }
  }
  cues.sort_by(|a, b| a.from.total_cmp(&b.from));
  Ok(Transcript {
    lang: "danmaku".into(),
    cues,
  })
}

/// One `DanmakuElem`: field 2 is the time in milliseconds, field 7 the text.
fn cue(elem: &[u8]) -> Cue {
  let (mut at, mut text) = (0.0, String::new());
  for field in proto::fields(elem) {
    match field {
      (2, Wire::Int(ms)) => at = ms as f64 / 1000.0,
      (7, Wire::Bytes(b)) => text = String::from_utf8_lossy(b).into_owned(),
      _ => {}
    }
  }
  Cue {
    from: at,
    to: at,
    text,
  }
}

/// The AI summary and outline, when Bilibili has generated one.
pub async fn summary(ctx: &Ctx, v: &Video) -> Result<Value> {
  let Part { post, cid, view } = part(ctx, v).await?;
  let data = api::get(ctx, SUMMARY)
    .arg("aid", v.aid)
    .arg("bvid", &v.bvid)
    .arg("cid", cid)
    .arg("up_mid", view.str("owner.mid").unwrap_or_default())
    .arg("web_location", "333.788")
    .wbi()
    .send()
    .await?;
  let result = data.at("model_result");
  let Some(summary) = result.str("summary") else {
    return Err(Error::not_found("no AI summary for this video"));
  };
  let outline: Vec<Value> = result
    .list("outline")
    .iter()
    .map(|o| {
      json!({
        "title": o.str("title"),
        "at": o.u64("timestamp"),
        "points": o.list("part_outline").iter().map(|p| json!({
          "at": p.u64("timestamp"),
          "text": p.str("content"),
        })).collect::<Vec<_>>(),
      })
    })
    .collect();
  Ok(json!({
    "video": post.id,
    "title": post.title,
    "summary": summary,
    "outline": outline,
  }))
}
