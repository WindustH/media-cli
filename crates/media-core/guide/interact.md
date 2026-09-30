# Interacting: likes, comments, follows, posts

All of these need a login and act as your account.

    media <platform> like POST [--undo]          # unlike: `unlike POST`
    media <platform> favorite POST [--folder F] [--undo]
    media <platform> comment POST "text" [--reply-to COMMENT]
    media <platform> delete-comment POST COMMENT [-y]
    media <platform> follow USER [--undo]        # unfollow: `unfollow USER`
    media <platform> post "text" [--title T] [-i IMAGE ...] [--topic T ...]
                         [--reply-to POST] [--quote POST]
    media <platform> delete POST [-y]

- `post -` reads the text from stdin: `cat draft.md | media zhihu post -`.
- `delete` and `delete-comment` ask for confirmation on a terminal; scripts
  pass `-y`.
- The result is an Action with the id and link of anything created.
- Writes pause a moment first on platforms that flag bursts.

What `post` publishes, per platform:

| Platform | `post` creates | Required | `--topic` | Others |
| --- | --- | --- | --- | --- |
| Zhihu | a pin (想法) | text or image | — | `ask`, `article` commands |
| Xiaohongshu | an image note | `--title`, `-i` | hashtags | `#tags` in the text too |
| Twitter / X | a tweet (4 images max) | text or image | hashtags | `--reply-to`, `--quote` |
| Bilibili | a dynamic | text or image | a topic id | `--quote` reposts |
| Reddit | a text, link, image or gallery post | `--title`, `--topic r/sub` or `u/you` | subreddit | a single URL makes a link post; `--quote` crossposts |
| YouTube | — | | | uploads are not supported |

Reply-to forms: Bilibili takes `ROOT` or `ROOT:PARENT`; Reddit and X take the
comment's id or link. Platform-only interactions (Bilibili `coin`,
`triple`; X `retweet`, `quote`; Reddit `downvote`, `edit`; YouTube
`dislike`; Zhihu `follow-question`) are in each platform's guide.

Be gentle: platforms restrict accounts that behave like bots. Space out
bulk actions (`--interval`), and do not like or follow in loops.
