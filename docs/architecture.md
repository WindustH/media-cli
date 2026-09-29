# Architecture

media-cli is one binary (`media`) that drives several social platforms through
the same command set. The code is split so that each part stays small and
knows as little as possible about the others.

```
src/main.rs                 registry: lists the platforms, nothing else
crates/media-core           platform-agnostic kernel
crates/media-zhihu          Zhihu
crates/media-xhs            Xiaohongshu
crates/media-twitter        Twitter / X
crates/media-bilibili       Bilibili
crates/media-reddit         Reddit
```

Dependencies only point downwards: the binary depends on every crate,
platform crates depend on `media-core` only, and `media-core` knows no
platform. Adding a platform means adding a crate and one `.platform::<P>()`
line in `src/main.rs`.

## media-core

| Module | Role |
| --- | --- |
| `platform` | `Platform` trait, `PlatformInfo` (static description), `Cap`, `Choices`, request types (`PageReq`, `Query`, `Draft`, `QrTicket`) |
| `ctx` | `Ctx`: what a platform works with (HTTP client with the session cookies, files, session extras, login hints, `#N` resolution) |
| `model` | normalized `Post`, `User`, `Comment`, `Collection`, `Notification`, `Page<T>`, `Action`, `AuthStatus`, `Transcript`, and `Data` (everything a command can print) |
| `cli` | shared subcommands (`CommonCommand`), per-platform clap command, dispatch to trait methods |
| `app` | top-level program, `media platforms`, multi-call (`bili` symlink) |
| `account` | `login` (QR / browser / cookie string), `logout`, `status` |
| `http` | Chrome-fingerprinted client (wreq), cookie jar, pacing, retries, streaming download |
| `store` | session file (0600), TTL cache, `#N` short-index lists |
| `paging` | `collect`: follow cursors until `--limit` items; `collect_window`: `--since` / `--until` |
| `download` | save media, merge DASH audio/video, audio-only, WAV segments (ffmpeg) |
| `file` | format detection by magic bytes, images to upload |
| `output` | human tables/cards, the `{ok, schema_version, platform, fetched_at, data}` envelope, and flat JSON Lines / CSV rows |
| `json` | `ValueExt`: dotted-path getters tolerant of upstream quirks (`v.str("a.b.0")`, `v.count("stat.like")`) |
| `text` | counts (`1.2万`), durations, truncation, HTML to text, timestamps |
| `browser` | import cookies from local browsers (`browser` feature, on by default) |

### The Platform contract

A platform crate exports one type implementing `media_core::Platform`:

- `const INFO: PlatformInfo` declares id, aliases, home page, cookie domains,
  required cookies, supported capabilities (`caps`), accepted option values
  (`choices`) and request pacing (`min_interval`).
- Methods for the shared commands (`search`, `read`, `comments`, `like`,
  `publish`, ...). Each has a default body returning `unsupported_operation`,
  so a platform implements only what it supports and lists it in `caps`;
  unsupported commands are hidden from its `--help`.
- Listing methods return one `Page<T>` per call for a `PageReq { cursor, size }`;
  the core follows `next_cursor` until `--limit` is reached. Cursors are opaque
  strings owned by the platform (offset, page number, upstream cursor ...).
- Post / user arguments arrive as strings with `#N` already resolved; the
  platform accepts its own ids and URLs (at least the `url` it puts in
  `Post.url` / `User.url`, because that is what `#N` resolves to).
- `type Extra: clap::Subcommand` holds platform-only commands; `run_extra`
  returns `Data` like everything else.

### Flow of one command

1. `app` parses arguments, picks the platform, starts a current-thread tokio runtime.
2. `cli::execute` loads the session (`MEDIA_<ID>_COOKIE` wins over the saved
   file), builds `Http` with those cookies and calls `P::new(ctx)`.
3. The shared command maps to a trait method (listings go through `paging::collect`).
4. Cookies changed by responses are written back to the session file (logged-in sessions only).
5. Lists are remembered for `#N`; `raw` payloads are dropped unless `--raw`;
   `output` prints a table or the JSON/YAML envelope.

## Inside a platform crate

Every platform follows the same layout so each file has one job:

| File | Role |
| --- | --- |
| `lib.rs` | `PlatformInfo` and the `Platform` impl: thin, delegates to the modules below |
| `api.rs` | the platform's transport: base headers, signing hook, response envelope → `Error` |
| `sign.rs` (or `sign/`) | request signatures (WBI, x-s, x-client-transaction-id ...) |
| `refs.rs` | parse ids and URLs into typed references |
| `parse.rs` | upstream JSON → core models |
| domain modules (`read.rs`, `write.rs`, `account.rs` ...) | endpoints grouped by domain |
| `extra.rs` | the `Extra` subcommand enum and its handlers |

Rules of thumb:

- Parse upstream JSON with `ValueExt` instead of mirror structs; keep the
  untouched object in the model's `raw` field.
- Map upstream failures to the shared `ErrorCode`s (login → `not_authenticated`,
  captcha → `verification_required`, ...), so scripts can react the same way everywhere.
- Long-lived upstream metadata (signing keys, GraphQL query ids) goes into
  `ctx.store.cache_get/cache_put` with a TTL.
- Write operations pause briefly (`ctx.http.pause`) where the platform is known
  to flag bursts.
