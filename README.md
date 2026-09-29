<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/media-cli-logo-dark.svg">
    <img src="docs/assets/media-cli-logo-light.svg" alt="media-cli" width="360">
  </picture>
</p>

<p align="center">
  <strong>One command line for Zhihu, Xiaohongshu, Twitter / X and Bilibili.</strong>
</p>

<p align="center">
  <a href="docs/output.md">Output format</a> ·
  <a href="docs/architecture.md">Architecture</a> ·
  <a href="README.zh-CN.md">简体中文</a>
</p>

---

`media` reads, searches, publishes and downloads on four social platforms
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
- **Short references.** After any list, `#3` means "the third item": no
  copying of long ids or links.
- **Paging that never loses its place.** `-n 200` follows pages for you;
  `--cursor` continues exactly where the last run stopped.
- **Log in the easy way.** Scan a QR code with the Zhihu, Xiaohongshu or
  Bilibili app, or paste a cookie header. Sessions stay on your machine.
- **Much works without an account.** Bilibili videos, comments, users and
  rankings; Twitter profiles, tweets and timelines; Xiaohongshu notes.
- **Downloads included.** Original-quality images, videos with their audio
  merged, audio only, or speech-recognition-ready WAV segments.
- **Behaves like a browser.** Chrome's network fingerprint, each platform's
  request signatures, and polite pacing keep requests looking ordinary.
- **Platform extras.** Bilibili coins, 一键三连, subtitles, AI summaries and
  danmaku; Twitter retweets, quotes and lists; Zhihu questions, articles and
  answers; Xiaohongshu creator notes.

## What each platform supports

| | Zhihu | Xiaohongshu | Twitter / X | Bilibili |
| --- | :-: | :-: | :-: | :-: |
| QR login | ✓ | ✓ | | ✓ |
| search posts / users | ✓ | ✓ | ✓ | ✓ |
| search topics | ✓ | ✓ | | |
| hot, feed | ✓ | ✓ | ✓ | ✓ |
| read, comments | ✓ | ✓ | ✓ | ✓ |
| comment replies | ✓ | ✓ | | ✓ |
| user, user posts | ✓ | ✓ | ✓ | ✓ |
| followers, following | ✓ | | ✓ | ✓ |
| collections, favorites | ✓ | favorites | ✓ | ✓ |
| likes | | ✓ | ✓ | |
| history | | | | ✓ |
| notifications | ✓ | ✓ | ✓ | ✓ |
| like, favorite, comment, follow | ✓ | ✓ | ✓ | ✓ |
| post, delete | ✓ | ✓ | ✓ | ✓ |
| download | ✓ | ✓ | ✓ | ✓ |

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
```

Log in for everything else:

```sh
media bili login                 # scan the QR code with the Bilibili app
media twitter login --cookie 'auth_token=...; ct0=...'
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
```

Each platform also answers to shorter names (`bilibili`, `x`, `xiaohongshu`,
`zh` ...). Link the binary as `bili`, `xhs`, `twitter` or `zhihu` to skip the
platform word entirely: `ln -s $(which media) ~/.local/bin/bili`.

## Good to know

- `MEDIA_<PLATFORM>_COOKIE` (for example `MEDIA_BILI_COOKIE`) supplies a cookie
  header without saving it. `--proxy` or `HTTPS_PROXY` routes requests through a proxy.
- Sessions live in `~/.config/media-cli/<platform>/session.json` (readable by
  you only); `media <platform> logout` removes them.
- `login --browser` reads the session from a local browser (Chrome, Edge,
  Firefox, Brave ...). To build without it, use `--no-default-features`.
- Keep bulk jobs slow: the platforms watch for automated use and may restrict
  accounts that look like bots.

## License

MIT
