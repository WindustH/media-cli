# Output formats

    -f, --format table|json|yaml|jsonl|csv
    --json, --yaml                      # shortcuts
    MEDIA_OUTPUT=json                   # default for every run

Default: `table` on a terminal, `yaml` when stdout is piped.

## table

Tables for lists, cards for single items, trees for comments. Progress and
hints go to stderr, so they never mix with data.

## json / yaml: the envelope

    ok: true
    schema_version: "1"
    platform: bili
    fetched_at: 2026-09-30T02:00:00Z
    data: ...

On failure `ok: false` and `error: {code, message, hint}` replace `data`, and
the exit code is non-zero: 2 for bad input, 3 when not logged in, 1 otherwise.

Error codes: `not_authenticated`, `verification_required`, `ip_blocked`,
`rate_limited`, `signature_error`, `invalid_input`, `not_found`,
`permission_denied`, `unsupported_operation`, `network_error`,
`upstream_error`, `internal_error`.

## jsonl / csv: rows for analysis

One row per item, no envelope (errors go to stderr). Every row carries
`platform` and `fetched_at`.

- CSV flattens nested fields into dotted columns (`author.name`,
  `metrics.views`); lists of plain values are joined with `|`, other lists
  stay JSON text.
- Comment threads become one row per comment with `depth` and `parent_id`.
- Insights become rows with `section` = `total`, `series` (with `date`),
  `breakdown` (with `dimension`, `label`) or `warning`.

## Shapes

Every platform maps its data onto the same shapes:

- **Post**: `id`, `kind` (`video`, `note`, `tweet`, `answer`, `article`, `pin`,
  `dynamic`, `post` ...), `title`, `text`, `url`, `author`, `created_at`,
  `metrics` (`views`, `likes`, `comments`, `shares`, `favorites`, plus
  platform counters such as `coins`, `danmaku`, `quotes`, `crossposts`),
  `media`, `tags`, `quoted`, `extra`.
- **User**: `id`, `name`, `handle`, `url`, `avatar`, `bio`, `verified`,
  `location`, `stats` (`followers`, `following`, `posts`, `likes` ...), `followed`.
- **Comment**: `id`, `author`, `text`, `created_at`, `likes`, `reply_count`,
  `reply_to`, `location`, `replies`.
- **Collection** (topics, folders, playlists, subreddits): `id`, `kind`,
  `name`, `description`, `url`, `items`, `followers`.
- **Notification**, **Action** (every write: `action`, `target`, `ok`, `id`,
  `url`), **Auth**, **Transcript** (`cues` of `from`, `to`, `text`),
  **Downloads**, **Insights** (`media guide analysis`).

`--raw` adds the untouched upstream object as `raw` to posts, users,
comments, collections, notifications and insights.
