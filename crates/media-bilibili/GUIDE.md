## References

- Videos: `BV1xx411c7mD`, `av170001`, `https://www.bilibili.com/video/BV…`
  (with `?p=N` for a part), `https://b23.tv/…` short links.
- Dynamics: numeric ids (17+ digits), `https://t.bilibili.com/<id>`,
  `https://www.bilibili.com/opus/<id>`.
- Users: the numeric mid (`946974`), `https://space.bilibili.com/<mid>`, or a
  name (looked up through user search).
- Reply to a comment with `--reply-to ROOT` or `--reply-to ROOT:PARENT`.

## Logging in

`media bili login` shows a QR code for the Bilibili app. Most reads work
without an account; subtitles, history, the dynamic feed, favorites of
others, notifications and every write need one.

## Analytics

`media bili insights [--days N]`: the creator data center. `--days` is
rounded up to Bilibili's windows (yesterday, 7, 30, 90 days, or all).

- totals and daily series: `views`, `profile_visitors`, `likes`,
  `favorites`, `coins`, `comments`, `danmaku`, `shares`, `followers`,
  `new_followers`, `lost_followers`, `net_followers`, `active_followers`;
  `extra.previous_period` holds the same numbers for the period before.
- breakdowns: `device`, `viewer_type` (followers vs others), `follow_source`,
  `video_last_day` (plays per video), and a follower portrait (gender, age,
  region, interest, active hours) once Bilibili builds one.

`media bili insights <your video>`: lifetime counters, average watch
seconds and share watched, 3-second bounce, interaction, follow-conversion
and non-follower rates, rank among similar videos (`extra.beats_similar`),
daily trends, audience breakdowns and a `retention` curve. Other videos get
their public counters plus the number watching now.

`media bili video-stats` lists your videos with those numbers side by side
(one row per video with `-f csv`).

## Interactions

- `likers` / `reposts` work on dynamics, and on videos through the dynamic
  that announced them.
- `coin VIDEO [-n 1|2] [--like]` and `triple VIDEO` spend coins: they cannot
  be undone.
- `subtitle VIDEO [--lang L]` and `danmaku VIDEO` return timed text;
  `summary VIDEO` Bilibili's AI summary.
- `post` publishes a dynamic (text and images, one topic id with `--topic`,
  `--quote` to repost).

## Downloads

Video and audio come as separate DASH streams (best quality your account
may see) and are merged with ffmpeg; `--audio-only` keeps the audio.

## Limits

- The data center no longer reports traffic sources (recommend, search ...).
- Per-video trends cover the first and the last 30 days after publishing.
- Reposts carry only a display date, so `--since` cannot filter them.
