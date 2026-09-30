<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/media-cli-logo-dark.svg">
    <img src="docs/assets/media-cli-logo-light.svg" alt="media-cli" width="360">
  </picture>
</p>

<p align="center">
  <strong>One command line for Zhihu, Xiaohongshu, Twitter / X, Bilibili, Reddit and YouTube.</strong>
</p>

<p align="center">
  <a href="docs/output.md">Output format</a> ·
  <a href="docs/architecture.md">Architecture</a> ·
  <a href="README.zh-CN.md">简体中文</a>
</p>

---

`media` reads, searches, publishes and downloads on six social platforms
with the same commands everywhere. It works in a terminal for people, and in
scripts and AI agents through a stable JSON / YAML output.

```console
$ media bili search "rust 教程" -n 3
 #  Content                                                      Author        Likes  Cmts  Views   Time
 1  Rust编程语言入门教程（Rust语言/Rust权威指南配套）【已完结】  软件工艺师    26.1k  2.6k    1.7M  2020-10-21
 2  【Rust腐蚀 新手教程】2026保姆级教程 实时更新！！！           Utaoki丶苦药   1.9k   401  155.3k  2025-01-13
 3  Rust语言 Slint教程，非tauri                                  猩猩程序员        0     0    7.5k

$ media bili read '#1'
$ media bili download '#1' --audio-only
```

## Why media-cli

- **Learn it once.** `search`, `hot`, `feed`, `read`, `comments`, `user`,
  `like`, `favorite`, `comment`, `follow`, `post`, `download` ... mean the same
  thing on every platform. `media platforms` shows what each one supports.
- **Made for agents and scripts.** Every result comes in one envelope with
  stable fields and error codes; piped output switches to YAML by itself.
  Posts, users and comments share one shape across platforms.
- **Ready for analysis.** Creator analytics (`insights`: trends, traffic
  sources, audience), who liked and reposted a post, complete comment threads,
  `--since` / `--until`, and CSV or JSON Lines output that drops straight into
  pandas or DuckDB, each row stamped with when it was fetched.
- **Short references.** After any list, `#3` means "the third item": no
  copying of long ids or links.
- **Paging that never loses its place.** `-n 200` follows pages for you;
  `--cursor` continues exactly where the last run stopped.
- **Log in the easy way.** Scan a QR code with the Zhihu, Xiaohongshu or
  Bilibili app, reuse your browser's session, or paste a cookie header.
  Sessions stay on your machine.
- **Much works without an account.** Bilibili videos, comments, users and
  rankings; Twitter profiles, tweets and timelines; Xiaohongshu notes; Reddit
  posts, comments, subreddits and users; YouTube videos, Shorts, channels,
  comments, playlists, transcripts and downloads.
- **Downloads included.** Original-quality images, videos with their audio
  merged, audio only, or speech-recognition-ready WAV segments.
- **Behaves like a browser.** Chrome's network fingerprint, each platform's
  request signatures, and polite pacing keep requests looking ordinary.
- **Platform extras.** Bilibili coins, 一键三连, subtitles, AI summaries and
  danmaku; Twitter retweets, quotes and lists; Zhihu questions, articles and
  answers; Xiaohongshu creator notes; Reddit subreddits, downvotes, crossposts
  and galleries; YouTube transcripts, Shorts, live streams, community posts,
  hashtags, playlists and related videos.

## What each platform supports

| | Zhihu | Xiaohongshu | Twitter / X | Bilibili | Reddit | YouTube |
| --- | :-: | :-: | :-: | :-: | :-: | :-: |
| QR login | ✓ | ✓ | | ✓ | | |
| search posts / users | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| search topics | ✓ | ✓ | | | ✓ | playlists |
| hot, feed | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| read, comments | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| comment replies | ✓ | ✓ | | ✓ | ✓ | ✓ |
| user, user posts | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| followers, following | ✓ | | ✓ | ✓ | | following |
| collections, favorites | ✓ | favorites | ✓ | ✓ | ✓ | ✓ |
| likes | | ✓ | ✓ | | ✓ | ✓ |
| history | | | | ✓ | | ✓ |
| notifications | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| like, favorite, comment, follow | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| post, delete | ✓ | ✓ | ✓ | ✓ | ✓ | |
| download | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

## Quick start

Build from source (Rust 1.88+, plus `cmake` and `clang` for the TLS library;
`ffmpeg` is optional and used for merging video and audio):

```sh
cargo install --path .
```

Try it without an account:

```sh
media bili search "rust 教程" -n 5
media bili comments '#1'
media x user-posts NASA -n 10
media xhs read https://www.xiaohongshu.com/explore/68cfff45000000001003c253
media reddit sub rust --sort top --time week -n 5
media youtube search "rust tutorial" --filter video -n 5
media yt transcript https://youtu.be/5C_HPTJg5ek
```

Log in for everything else:

```sh
media bili login                 # scan the QR code with the Bilibili app
media twitter login --cookie 'auth_token=...; ct0=...'
media reddit login --browser     # reuse the session of your browser
media youtube login --browser
media zhihu status
```

A few more:

```sh
media zhihu hot -n 10
media xhs search 咖啡 --sort popular
media x read https://x.com/NASA/status/2104695667180380260 --json
media bili subtitle BV1xx411c7mD
media bili download BV1xx411c7mD --audio-only --split 25
media twitter post "Hello" -i photo.jpg
media reddit comments '#1' --sort top
media reddit post "Hello from the terminal" --title "Hi" --topic r/test
media reddit follow r/rust
media youtube user @Fireship
media yt comments '#1' --sort new --all --replies -f csv > comments.csv
media youtube download https://www.youtube.com/shorts/fwBIZRq-vzY
```

Each platform also answers to shorter names (`bilibili`, `x`, `xiaohongshu`,
`zh`, `rd`, `yt` ...). Link the binary as `bili`, `xhs`, `twitter`, `zhihu`,
`reddit` or `youtube` to skip the platform word entirely: `ln -s $(which media) ~/.local/bin/bili`.

## Documentation

Everything is documented inside the binary:

```sh
media guide                 # topics: start, login, refs, output, analysis, download ...
media guide analysis        # creator insights, exports, snapshots
media guide bili            # one platform: commands, references, login, limits
media bili search --help    # a command's options, with examples
```

## Good to know

- `MEDIA_<PLATFORM>_COOKIE` (for example `MEDIA_BILI_COOKIE`) supplies a cookie
  header without saving it. `--proxy` or `HTTPS_PROXY` routes requests through a proxy.
- `--interval 5` (or `MEDIA_INTERVAL=5`) spaces requests at least five seconds
  apart, for large collection jobs.
- Sessions live in `~/.config/media-cli/<platform>/session.json` (readable by
  you only); `media <platform> logout` removes them.
- `login --browser` reads the session from a local browser (Chrome, Edge,
  Firefox, Brave ...). To build without it, use `--no-default-features`.
- Reddit refuses logged-out requests from some networks. Log in, or set
  `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `REDDIT_USERNAME` and
  `REDDIT_PASSWORD` for a personal "script" app from
  <https://www.reddit.com/prefs/apps> to use Reddit's official API.
- YouTube's creator analytics (`media youtube insights`) come from the
  official YouTube Analytics API: set `YOUTUBE_CLIENT_ID` and
  `YOUTUBE_CLIENT_SECRET` of a "Desktop app" OAuth client, run
  `media youtube oauth` once and set the `YOUTUBE_REFRESH_TOKEN` it prints.
- Keep bulk jobs slow: the platforms watch for automated use and may restrict
  accounts that look like bots.

## License

MIT
