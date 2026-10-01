## References

Most note reads need the note's `xsec_token`, which Xiaohongshu puts in the
links it shows. Use:

- links: `https://www.xiaohongshu.com/explore/<id>?xsec_token=…`,
  `/discovery/item/<id>?…`, `https://xhslink.com/…` short links, or shared
  text containing one;
- `#N` and the `url` of earlier results (they carry the token);
- a bare note id only after it appeared in a list (tokens are cached a day).

Users: the id, a `/user/profile/<id>` link, or a short link.

## Logging in

`media xhs login --browser` is the reliable way. The QR code login works,
but Xiaohongshu often asks the new device for a captcha after the scan.
Without a login only `read` and `download` work; everything else answers
captchas to visitors.

Xiaohongshu is quick to flag automation: requests are spaced at least a
second apart, and a captcha (`verification_required`) means stop, open the
site in the browser, pass the check, and wait before retrying.

## Analytics

`media xhs insights [--days N]`: the creator data center, with 7-day and
30-day windows only (`--days` up to 7 uses the first).

- totals and daily series: `impressions`, `views`, `ctr`,
  `avg_watch_seconds`, `completion_rate`, `likes`, `comments`, `favorites`,
  `shares`, follower gains and losses, `profile_views`, posts published.
- breakdowns: `traffic_source`, `hour_of_day`, `vs_similar_creators`; fan
  portraits (gender, age, city, interest, follow source) from 50 fans.

`media xhs insights <your note>`: lifetime totals, daily series, audience,
traffic sources and, for videos, `retention`. Others' notes get public
counters. The data center has to be enabled once (it is requested on first
use and shows data from the next day).

`note-stats` lists your notes with impressions, views, CTR, watch time and
follows; `active-fans` your most engaged fans; `my-notes` your notes.

## Interactions

- `post` needs `--title` and at least one image (`-i`); `--topic` and
  `#tags` in the text become topics.
- `comment NOTE "text" [--reply-to COMMENT]`, `delete-comment`, `like`,
  `favorite` (collect), `follow`, `delete` (your note). Comments are text
  only: images in comments are an app feature the web API ignores. Read
  comments carry their images in `extra.pictures`.
- `likers` and `reposts` do not exist on Xiaohongshu; who liked your notes
  shows in `notifications --type likes`.

## Activity center

The creator center's activities (活动中心): official campaigns with rewards
for notes about their topic.

- `events [KEYWORD] [--kept] [--latest]` lists the running activities, or
  those whose name, reward or topic contains KEYWORD; `--kept` only the ones
  you kept.
- `event EVENT` shows one: rewards, start and end, topics, links.
- `keep-event EVENT [--undo]` keeps (收藏) an activity or drops it.
- `join-event EVENT [TEXT] --title T -i IMAGE [--topic T]` publishes an
  image note for the activity, as the publish page's 关联活动 does: the
  activity's topics are added to the note and the note is linked to the
  activity. One activity per note.

EVENT is `#N` of the last list, the activity id, its link, or its exact name.

## Downloads

Images at full resolution (JPEG transcodes of the originals) and the best
video stream, including live photos.
