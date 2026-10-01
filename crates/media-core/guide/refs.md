# Referring to things, and paging

## Posts and users

Commands take whatever the platform shows you: ids, links, short links, and
the `url` field of any earlier result. Each platform guide lists its forms.

After any list, `#N` (or a plain number of up to three digits) means the
N-th item of that list, for posts, users and collections (folders, lists,
activities) separately:

    media xhs search 咖啡
    media xhs read '#2'
    media xhs comments 2 -n 50
    media xhs user-posts '#1'           # after a user list
    media bili collections
    media bili favorites --folder '#1'  # after a folder list

Quote `#N` in shells where `#` starts a comment.

Omitting USER in `user-posts`, `followers` and `following` (and in
`collections`, `favorites`, `likes`) means your own account.

## How many items

- `-n / --limit N` (default 20): pages are followed until N items arrive.
- `--all` on `comments`: every comment, however many pages.
- `--replies` on `comments`: also every reply under each comment (nested
  threads down to three levels).

## Continuing later

A list that stops before the end prints `more: --cursor <c>` (and carries
`next_cursor` in structured output). Pass it back to continue exactly where
it stopped:

    media bili comments BV1xx411c7mD -n 100 --json > part1.json
    media bili comments BV1xx411c7mD -n 100 --cursor '<next_cursor>' --json > part2.json

A cursor ending in `!N` resumes in the middle of an upstream page; nothing is
skipped or repeated. Items a platform repeats across pages are dropped.

## Time windows

Lists of posts, comments and notifications take:

    --since 7d          # also 12h, 30m
    --since 2026-09-01  # a local date (its start)
    --until 2026-09-15T12:00:00+08:00

Items outside the window are dropped; on newest-first lists, a page that is
entirely older than `--since` ends the listing. User and collection lists do
not take these options.
