# Getting started

`media` drives six platforms through one command set:

    media <platform> <command> [arguments] [options]

Platforms (each also answers to shorter names):

| Platform | Name | Aliases |
| --- | --- | --- |
| Zhihu | `zhihu` | `zh` |
| Xiaohongshu / RedNote | `xhs` | `xiaohongshu`, `rednote` |
| Twitter / X | `twitter` | `x`, `tw` |
| Bilibili | `bili` | `bilibili`, `b23` |
| Reddit | `reddit` | `rd` |
| YouTube | `youtube` | `yt` |

`media platforms` shows which commands each platform supports.

## First steps

Many reads work without an account:

    media bili hot -n 5                 # trending videos
    media bili read '#1'                # the first item of the last list
    media bili comments '#1' -n 20      # its comments
    media yt search "rust tutorial" -n 5
    media x user-posts NASA -n 10

Log in for everything else (see `media guide login`):

    media bili login                    # scan a QR code with the app
    media x login --browser             # reuse a logged-in browser
    media zhihu status                  # check a saved session

Write, analyse, download:

    media bili like BV1xx411c7mD
    media bili insights --days 30       # creator analytics of your account
    media bili download BV1xx411c7mD --audio-only

## The shared commands

| Group | Commands |
| --- | --- |
| Account | `login`, `logout`, `status`, `whoami` |
| Discover | `search`, `hot`, `feed` |
| Content | `read` (`show`), `comments`, `replies`, `likers`, `reposts`, `download` |
| People | `user`, `user-posts` (`posts`), `followers`, `following` |
| Your library | `collections`, `favorites` (`bookmarks`), `likes`, `history`, `notifications`, `unread` |
| Interact | `like` / `unlike`, `favorite` / `unfavorite`, `comment`, `delete-comment`, `follow` / `unfollow`, `post` (`publish`), `delete` |
| Analytics | `insights [POST]` |

Commands a platform does not support are hidden from its `--help` and
answer `unsupported_operation`. Platform-only commands (Bilibili coins,
Zhihu questions, YouTube transcripts ...) are listed in `media guide <platform>`.

## Getting help

    media --help                        # global options and platforms
    media <platform> --help             # a platform's commands
    media <platform> <command> --help   # options, with examples
    media guide                         # all guide topics
    media guide <topic>                 # one topic, e.g. `media guide analysis`

Guide topics print as plain Markdown when stdout is not a terminal, so
scripts and AI agents can read them too.
