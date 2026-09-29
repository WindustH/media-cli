# Output for scripts and agents

Every command prints either a human view (tables and cards) or a structured
envelope. The envelope is chosen with `--json`, `--yaml` or `-f/--format`, or
`MEDIA_OUTPUT=json|yaml|table`. When stdout is not a terminal, YAML is the
default, because it is the most compact for language models.

## Envelope

Success:

```yaml
ok: true
schema_version: "1"
platform: bili
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
