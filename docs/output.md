# Output for scripts and agents

Every command prints a human view (tables and cards), a structured envelope,
or flat rows. Choose with `--json`, `--yaml` or `-f/--format
table|json|yaml|jsonl|csv`, or `MEDIA_OUTPUT`. When stdout is not a terminal,
YAML is the default, because it is the most compact for language models.

## Rows for analysis (`jsonl`, `csv`)

`-f jsonl` prints one JSON object per item and `-f csv` one row per item
with nested fields as dotted columns (`author.name`, `metrics.views`); lists
of plain values are joined with `|`. There is no envelope: errors go to
stderr with a non-zero exit code. Every row carries `platform` and
`fetched_at`, so repeated runs (cron) build a time series directly.

- Comment threads are flattened: each comment is a row with `depth` and
  `parent_id`. `comments --all --replies` fetches every comment and every reply.
- Insights give one row per headline number (`section=total`), trend point
  (`section=series`, with `date`), distribution slice (`section=breakdown`)
  and warning (`section=warning`); `period` says what a number covers when it
  is not the series window.
- Listings of posts, comments and notifications take `--since` / `--until`
  (`7d`, `12h`, `2026-09-01` or an RFC 3339 time).

```sh
media bili user-posts 946974 -n 500 --since 90d -f csv > videos.csv
media bili comments BV1xx411c7mD --all --replies -f jsonl > comments.jsonl
media x insights -f csv > account.csv
```

## Envelope

Success:

```yaml
ok: true
schema_version: "1"
platform: bili
fetched_at: 2026-09-30T02:00:00Z
data: ...
```

Failure (the exit code is non-zero as well):

```yaml
ok: false
schema_version: "1"
platform: bili
error:
  code: not_authenticated
  message: not logged in (missing cookie `SESSDATA`)
  hint: run `media bili login` (QR code) or `media bili login --cookie '...'`
```

| Exit code | Meaning |
| --- | --- |
| 0 | success |
| 1 | any failure not listed below; also `status` when not logged in |
| 2 | invalid input (bad arguments, unknown id format ...) |
| 3 | not authenticated |

### Error codes

| Code | What to do |
| --- | --- |
| `not_authenticated` | log in (`media <platform> login`) |
| `verification_required` | the platform wants a captcha or check: pass it in a browser, retry later |
| `ip_blocked` | switch network |
| `rate_limited` | wait before retrying |
| `signature_error` | the request signature was rejected; update media-cli |
| `invalid_input` | fix the arguments |
| `not_found` | the post / user does not exist or is not visible |
| `permission_denied` | the account may not do this |
| `unsupported_operation` | the platform has no such feature |
| `network_error` | connection problem; retry |
| `upstream_error` | the platform answered something unexpected |
| `internal_error` | local problem (files, ffmpeg ...) |

## Data shapes

Every platform maps its content onto the same shapes. Fields without a value
are left out.

**Listings** are pages: `items`, `has_more`, and `next_cursor` to pass back
with `--cursor`. `--limit/-n` follows pages automatically; a cursor ending in
`!N` resumes in the middle of an upstream page.

**Post**: `id`, `kind` (`video`, `note`, `tweet`, `answer`, `question`,
`article`, `pin`, `dynamic` ...), `title`, `text`, `url`, `author` (a User),
`created_at` / `updated_at` (RFC 3339), `metrics`, `media`, `tags`, `quoted`
(the quoted / reposted / parent post), `extra` (platform fields).

`metrics`: `views`, `likes`, `comments`, `shares`, `favorites`, plus platform
counters such as `coins`, `danmaku`, `quotes`.

`media`: `kind` (`image`, `video`, `audio`, `gif`), `url`, `audio_url` (separate
audio track), `width`, `height`, `duration` (seconds), `alt`.

**User**: `id`, `name`, `handle`, `url`, `avatar`, `bio`, `verified`,
`location`, `stats` (`followers`, `following`, `posts`, `likes`, ...),
`followed`, `created_at`, `extra`.

**Comment**: `id`, `author`, `text`, `created_at`, `likes`, `reply_count`,
`reply_to`, `location`, `replies` (nested comments).

**Collection** (topics, favorites folders, lists): `id`, `kind`, `name`,
`description`, `url`, `items`, `followers`, `views`, `owner`.

**Notification**: `id`, `kind`, `text`, `actor`, `target`, `url`,
`created_at`, `unread`.

**Action** (every write command): `action`, `target`, `ok`, and `id` / `url`
of anything created.

**Auth** (`login`, `status`): `authenticated`, `user`, `source`, `saved_at`.

**Transcript** (subtitles, danmaku): `lang`, `cues` of `from`, `to` (seconds), `text`.

**Downloads**: a list of `kind`, `path`, `bytes`.

**Insights** (`insights [POST]`, creator analytics of your account or one of
your posts): `kind` (`account` / `post`), `subject`, `title`, `url`, `from` /
`to` (days covered), `totals` (headline numbers such as `views`, `likes`,
`new_followers`, `avg_watch_seconds`; rates as 0..1 fractions), `series`
(`metric` + `points` of `date`, `value`), `breakdowns` (`dimension` such as
`traffic_source`, `age`, `gender`, `region`, with `items` of `label`, `id`,
`value`, `ratio`) and `warnings` (why data is missing or partial: a paid tier,
a creator level, a follower threshold ...). `from` / `to` are the days the
series cover; when `totals` or a breakdown cover something else, it says so
in `totals_period` / `period` (for a post the totals are usually `lifetime`).

`likers POST` lists the users who liked a post (a page of Users) and
`reposts POST` its reposts / retweets / quotes / crossposts (a page of Posts).

`--raw` adds the untouched upstream object as `raw` to posts, users, comments,
collections and notifications.

## Referring to results

Every post and user carries a `url` that other commands accept back. After a
listing, `#N` (or plain `N` up to three digits) refers to its N-th item:

```sh
media xhs search 咖啡
media xhs read '#2'
media xhs comments 2 -n 50
```
